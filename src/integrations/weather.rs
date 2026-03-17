use reqwest::Client;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(30 * 60); // 30 minutes
// wttr.in format: condition_symbol|description|temp_actual|temp_feels|humidity|wind
const WTTR_FORMAT: &str = "%c|%C|%t|%f|%h|%w";

#[derive(Debug, Clone)]
pub struct WeatherData {
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

/// Fire-and-forget async fetch — caller receives result via mpsc channel.
pub async fn fetch(location: &str, units: &str) -> Result<WeatherData, String> {
    if location.is_empty() {
        return Err("No location configured — set weather.location in config.toml".to_string());
    }

    let unit_param = if units == "metric" { "m" } else { "u" };
    let url = format!(
        "https://wttr.in/{}?format={}&{}",
        urlencoding::encode(location),
        WTTR_FORMAT,
        unit_param
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

    let text = response
        .text()
        .await
        .map_err(|e| format!("Read failed: {}", e))?;

    parse_wttr_response(&text)
}

fn parse_wttr_response(raw: &str) -> Result<WeatherData, String> {
    let line = raw.lines().next().unwrap_or("").trim();
    let parts: Vec<&str> = line.splitn(6, '|').collect();

    if parts.len() < 6 {
        return Err(format!("Unexpected wttr.in response: {:?}", raw));
    }

    Ok(WeatherData {
        condition_icon: parts[0].trim().to_string(),
        description: parts[1].trim().to_string(),
        temp: parts[2].trim().to_string(),
        feels_like: parts[3].trim().to_string(),
        humidity: parts[4].trim().to_string(),
        wind: parts[5].trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_wttr_response_valid() {
        let raw = "⛅️|Partly cloudy|+68°F|+65°F|72%|↙9mph";
        let data = parse_wttr_response(raw).unwrap();
        assert_eq!(data.condition_icon, "⛅️");
        assert_eq!(data.description, "Partly cloudy");
        assert_eq!(data.temp, "+68°F");
        assert_eq!(data.humidity, "72%");
    }

    #[test]
    fn test_parse_wttr_response_too_few_parts() {
        let raw = "⛅️|Partly cloudy|+68°F";
        assert!(parse_wttr_response(raw).is_err());
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
            condition_icon: "☀️".into(),
            description: "Sunny".into(),
            temp: "75°F".into(),
            feels_like: "73°F".into(),
            humidity: "40%".into(),
            wind: "5mph".into(),
        }));
        assert!(!cache.needs_refresh());
    }
}
