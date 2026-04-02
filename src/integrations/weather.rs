use reqwest::Client;
use serde::Deserialize;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes

#[derive(Debug, Clone)]
pub struct WeatherData {
    pub location: String,
    pub condition_icon: String,
    pub description: String,
    pub temp: String,
    pub feels_like: String,
    pub humidity: String,
    pub wind: String,
}

#[derive(Debug, Clone)]
pub enum WeatherState {
    /// Not yet fetched / config disabled
    Idle,
    /// Fetch in progress
    Loading,
    /// Successfully fetched
    Ready(WeatherData),
    /// Last fetch failed
    Error(String),
}

/// Holds cached weather + last-fetch timestamp.
pub struct WeatherCache {
    pub state: WeatherState,
    last_fetched: Option<Instant>,
}

impl WeatherCache {
    pub fn new() -> Self {
        Self {
            state: WeatherState::Idle,
            last_fetched: None,
        }
    }

    /// Returns true if we should kick off a new fetch.
    pub fn needs_refresh(&self) -> bool {
        match &self.state {
            WeatherState::Loading => false,
            WeatherState::Idle | WeatherState::Error(_) => true,
            WeatherState::Ready(_) => self
                .last_fetched
                .map(|t| t.elapsed() >= CACHE_TTL)
                .unwrap_or(true),
        }
    }

    pub fn set_loading(&mut self) {
        self.state = WeatherState::Loading;
    }

    pub fn set_result(&mut self, result: Result<WeatherData, String>) {
        self.last_fetched = Some(Instant::now());
        self.state = match result {
            Ok(data) => WeatherState::Ready(data),
            Err(e) => WeatherState::Error(e),
        };
    }
}

/// Try to get lat,lon from macOS CoreLocation services.
/// Builds a minimal .app bundle (required for CoreLocation permission) in the
/// pulse config dir, compiles the Swift helper on first use, then launches it
/// via `open` so macOS grants location access.
/// Falls back to None if unavailable (no permission, non-macOS, compile failure).
/// Returns (city_name, lat, lon) if successful.
fn detect_macos_location() -> Option<(String, f64, f64)> {
    #[cfg(not(target_os = "macos"))]
    {
        return None;
    }

    #[cfg(target_os = "macos")]
    {
        use std::fs;
        use std::process::Command;

        let config_dir = crate::config::config_dir();
        let app_bundle = config_dir.join("PulseLocation.app");
        let contents_dir = app_bundle.join("Contents");
        let bin_dir = contents_dir.join("MacOS");
        let binary = bin_dir.join("PulseLocation");
        let plist = contents_dir.join("Info.plist");
        let swift_src = config_dir.join("pulse_location.swift");

        // Build the app bundle + compile on first use
        if !binary.exists() {
            fs::create_dir_all(&bin_dir).ok()?;

            fs::write(&plist, r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key>
    <string>com.pulse.location</string>
    <key>CFBundleName</key>
    <string>PulseLocation</string>
    <key>CFBundleExecutable</key>
    <string>PulseLocation</string>
    <key>NSLocationUsageDescription</key>
    <string>Pulse needs your location to show local weather.</string>
    <key>NSLocationWhenInUseUsageDescription</key>
    <string>Pulse needs your location to show local weather.</string>
</dict>
</plist>
"#).ok()?;

            fs::write(&swift_src, r#"import CoreLocation
import Foundation

class Loc: NSObject, CLLocationManagerDelegate {
    let manager = CLLocationManager()
    let geocoder = CLGeocoder()

    func start() {
        manager.delegate = self
        manager.desiredAccuracy = kCLLocationAccuracyKilometer
        manager.requestWhenInUseAuthorization()
    }

    func locationManagerDidChangeAuthorization(_ m: CLLocationManager) {
        switch m.authorizationStatus {
        case .authorizedAlways, .authorized:
            manager.startUpdatingLocation()
        case .denied, .restricted:
            CFRunLoopStop(CFRunLoopGetMain())
        case .notDetermined:
            break
        @unknown default:
            break
        }
    }

    func locationManager(_ m: CLLocationManager, didUpdateLocations l: [CLLocation]) {
        manager.stopUpdatingLocation()
        guard let location = l.last else {
            CFRunLoopStop(CFRunLoopGetMain())
            return
        }
        let coords = "\(location.coordinate.latitude),\(location.coordinate.longitude)"
        geocoder.reverseGeocodeLocation(location) { placemarks, _ in
            if let p = placemarks?.first {
                let city = p.locality ?? p.name ?? ""
                let state = p.administrativeArea ?? ""
                if !city.isEmpty {
                    let name = state.isEmpty ? city : "\(city), \(state)"
                    print("\(name)|\(coords)")
                } else {
                    print("|\(coords)")
                }
            } else {
                print("|\(coords)")
            }
            CFRunLoopStop(CFRunLoopGetMain())
        }
    }

    func locationManager(_ m: CLLocationManager, didFailWithError e: Error) {
        CFRunLoopStop(CFRunLoopGetMain())
    }
}

let d = Loc()
d.start()
CFRunLoopRunInMode(.defaultMode, 10, false)
"#).ok()?;

            let compile = Command::new("swiftc")
                .args([
                    swift_src.to_str()?,
                    "-o",
                    binary.to_str()?,
                    "-framework",
                    "CoreLocation",
                ])
                .output()
                .ok()?;

            if !compile.status.success() {
                return None;
            }
        }

        // Launch via `open` so macOS sees the app bundle identity for permissions.
        // Use a timeout to prevent hanging indefinitely if the app doesn't exit.
        let out_file = config_dir.join("pulse_location_out.txt");
        let _ = fs::remove_file(&out_file);

        let mut child = Command::new("open")
            .args([
                app_bundle.to_str()?,
                "--stdout",
                out_file.to_str()?,
                "--wait-apps",
            ])
            .spawn()
            .ok()?;

        // Wait up to 15 seconds for the location helper to finish
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        return None;
                    }
                    break;
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        return None;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
                Err(_) => return None,
            }
        }

        let raw = fs::read_to_string(&out_file).ok()?.trim().to_string();
        let _ = fs::remove_file(&out_file);

        // Output format: "City, State|lat,lon" or "|lat,lon"
        if let Some((name, coords)) = raw.split_once('|') {
            let parts: Vec<&str> = coords.split(',').collect();
            if parts.len() == 2 {
                let lat: f64 = parts[0].trim().parse().ok()?;
                let lon: f64 = parts[1].trim().parse().ok()?;
                return Some((name.trim().to_string(), lat, lon));
            }
        }
        None
    }
}

// ── Open-Meteo JSON response types ──────────────────────────────────────────

#[derive(Deserialize)]
struct OpenMeteoResponse {
    current: OpenMeteoCurrent,
}

#[derive(Deserialize)]
struct OpenMeteoCurrent {
    temperature_2m: f64,
    apparent_temperature: f64,
    relative_humidity_2m: u32,
    weather_code: u32,
    wind_speed_10m: f64,
    wind_direction_10m: f64,
}

/// Map WMO weather code to (icon, description).
fn wmo_weather(code: u32) -> (&'static str, &'static str) {
    match code {
        0 => ("☀️", "Clear sky"),
        1 => ("🌤", "Mainly clear"),
        2 => ("⛅", "Partly cloudy"),
        3 => ("☁️", "Overcast"),
        45 | 48 => ("🌫", "Fog"),
        51 | 53 | 55 => ("🌦", "Drizzle"),
        56 | 57 => ("🌧", "Freezing drizzle"),
        61 | 63 | 65 => ("🌧", "Rain"),
        66 | 67 => ("🌧", "Freezing rain"),
        71 | 73 | 75 => ("❄️", "Snowfall"),
        77 => ("🌨", "Snow grains"),
        80 | 81 | 82 => ("🌦", "Rain showers"),
        85 | 86 => ("🌨", "Snow showers"),
        95 => ("⛈", "Thunderstorm"),
        96 | 99 => ("⛈", "Thunderstorm w/ hail"),
        _ => ("🌡", "Unknown"),
    }
}

/// Map wind direction degrees to cardinal direction.
fn wind_direction(degrees: f64) -> &'static str {
    let dirs = ["N", "NE", "E", "SE", "S", "SW", "W", "NW"];
    let idx = ((degrees + 22.5) / 45.0) as usize % 8;
    dirs[idx]
}

/// Fire-and-forget async fetch — caller receives result via mpsc channel.
/// Location is detected via macOS CoreLocation, falling back to a default.
pub async fn fetch(units: &str) -> Result<WeatherData, String> {
    let detected = detect_macos_location();

    let (lat, lon) = match &detected {
        Some((_, lat, lon)) => (*lat, *lon),
        None => return Err("Could not detect location".to_string()),
    };

    let location_name = detected
        .as_ref()
        .map(|(name, _, _)| name.clone())
        .unwrap_or_default();

    let (temp_unit, speed_unit) = if units == "metric" {
        ("celsius", "kmh")
    } else {
        ("fahrenheit", "mph")
    };

    let url = format!(
        "https://api.open-meteo.com/v1/forecast?\
         latitude={}&longitude={}\
         &current=temperature_2m,apparent_temperature,relative_humidity_2m,weather_code,wind_speed_10m,wind_direction_10m\
         &temperature_unit={}&wind_speed_unit={}",
        lat, lon, temp_unit, speed_unit
    );

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent("pulse/0.1")
        .build()
        .map_err(|e| e.to_string())?;

    let response = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status()));
    }

    let resp: OpenMeteoResponse = response
        .json()
        .await
        .map_err(|e| format!("Parse failed: {}", e))?;

    let c = &resp.current;
    let (icon, description) = wmo_weather(c.weather_code);
    let deg_symbol = if units == "metric" { "C" } else { "F" };
    let speed_label = if units == "metric" { "km/h" } else { "mph" };

    Ok(WeatherData {
        location: location_name,
        condition_icon: icon.to_string(),
        description: description.to_string(),
        temp: format!("{:.0}°{}", c.temperature_2m, deg_symbol),
        feels_like: format!("{:.0}°{}", c.apparent_temperature, deg_symbol),
        humidity: format!("{}%", c.relative_humidity_2m),
        wind: format!(
            "{} {:.0}{}",
            wind_direction(c.wind_direction_10m),
            c.wind_speed_10m,
            speed_label
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wmo_weather_codes() {
        assert_eq!(wmo_weather(0).1, "Clear sky");
        assert_eq!(wmo_weather(3).1, "Overcast");
        assert_eq!(wmo_weather(61).1, "Rain");
        assert_eq!(wmo_weather(95).1, "Thunderstorm");
        assert_eq!(wmo_weather(255).1, "Unknown");
    }

    #[test]
    fn test_wind_direction() {
        assert_eq!(wind_direction(0.0), "N");
        assert_eq!(wind_direction(90.0), "E");
        assert_eq!(wind_direction(180.0), "S");
        assert_eq!(wind_direction(270.0), "W");
        assert_eq!(wind_direction(45.0), "NE");
        assert_eq!(wind_direction(225.0), "SW");
    }

    #[test]
    fn test_cache_needs_refresh_when_idle() {
        let cache = WeatherCache::new();
        assert!(cache.needs_refresh());
    }

    #[test]
    fn test_cache_no_refresh_while_loading() {
        let mut cache = WeatherCache::new();
        cache.set_loading();
        assert!(!cache.needs_refresh());
    }

    #[test]
    fn test_cache_no_refresh_when_fresh() {
        let mut cache = WeatherCache::new();
        cache.set_result(Ok(WeatherData {
            location: "Colfax, CA".into(),
            condition_icon: "☀️".into(),
            description: "Clear sky".into(),
            temp: "72°F".into(),
            feels_like: "66°F".into(),
            humidity: "29%".into(),
            wind: "SW 8mph".into(),
        }));
        assert!(!cache.needs_refresh());
    }
}
