use std::process::Command;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone)]
pub struct CalendarEvent {
    pub title: String,
    pub time: String,
    pub calendar: String,
}

#[derive(Debug, Clone)]
pub enum CalendarState {
    Idle,
    Loading,
    Ready(Vec<CalendarEvent>),
    Error(String),
}

pub struct CalendarCache {
    pub state: CalendarState,
    last_fetched: Option<Instant>,
}

impl CalendarCache {
    pub fn new() -> Self {
        Self {
            state: CalendarState::Idle,
            last_fetched: None,
        }
    }

    pub fn needs_refresh(&self) -> bool {
        match &self.state {
            CalendarState::Loading => false,
            CalendarState::Idle | CalendarState::Error(_) => true,
            CalendarState::Ready(_) => self
                .last_fetched
                .map(|t| t.elapsed() >= CACHE_TTL)
                .unwrap_or(true),
        }
    }

    pub fn set_loading(&mut self) {
        self.state = CalendarState::Loading;
    }

    pub fn set_result(&mut self, result: Result<Vec<CalendarEvent>, String>) {
        self.last_fetched = Some(Instant::now());
        self.state = match result {
            Ok(events) => CalendarState::Ready(events),
            Err(e) => CalendarState::Error(e),
        };
    }
}

/// Fetch upcoming events using `icalBuddy` (macOS).
/// This is a blocking call (runs a subprocess), but we run it from a background thread.
pub fn fetch(num_events: u32) -> Result<Vec<CalendarEvent>, String> {
    // Check if icalBuddy is installed
    let output = Command::new("which")
        .arg("icalBuddy")
        .output()
        .map_err(|e| format!("Failed to check for icalBuddy: {}", e))?;

    if !output.status.success() {
        return Err("icalBuddy not found. Install via: brew install ical-buddy".to_string());
    }

    let output = Command::new("icalBuddy")
        .args([
            "-n",                          // only future events
            "-ea",                         // exclude all-day events ... actually include them
            "-li", &num_events.to_string(), // limit number
            "-nc",                         // no calendar names in title
            "-nrd",                        // no relative dates
            "-df", "%H:%M",               // time format
            "-iep", "title,datetime,calendarTitle", // include these properties
            "-po", "datetime,title,calendarTitle",  // property order
            "-ps", " | ",                  // property separator
            "-b", "",                      // no bullet
            "eventsToday+7",               // events today through +7 days
        ])
        .output()
        .map_err(|e| format!("icalBuddy failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("icalBuddy error: {}", stderr));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let events = parse_icalbuddy_output(&stdout);
    Ok(events)
}

fn parse_icalbuddy_output(raw: &str) -> Vec<CalendarEvent> {
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| {
            let trimmed = line.trim();
            // Expected format: "HH:MM | Title | CalendarName"
            // or just "Title" for all-day events
            let parts: Vec<&str> = trimmed.splitn(3, " | ").collect();
            match parts.len() {
                3 => Some(CalendarEvent {
                    time: parts[0].trim().to_string(),
                    title: parts[1].trim().to_string(),
                    calendar: parts[2].trim().to_string(),
                }),
                2 => Some(CalendarEvent {
                    time: parts[0].trim().to_string(),
                    title: parts[1].trim().to_string(),
                    calendar: String::new(),
                }),
                1 => Some(CalendarEvent {
                    time: String::new(),
                    title: parts[0].trim().to_string(),
                    calendar: String::new(),
                }),
                _ => None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_icalbuddy_three_fields() {
        let raw = "09:30 | Standup | Work\n14:00 | 1:1 with manager | Work\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].time, "09:30");
        assert_eq!(events[0].title, "Standup");
        assert_eq!(events[0].calendar, "Work");
        assert_eq!(events[1].title, "1:1 with manager");
    }

    #[test]
    fn test_parse_icalbuddy_two_fields() {
        let raw = "10:00 | Team lunch\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].time, "10:00");
        assert_eq!(events[0].title, "Team lunch");
    }

    #[test]
    fn test_parse_empty() {
        let events = parse_icalbuddy_output("");
        assert!(events.is_empty());
    }

    #[test]
    fn test_cache_lifecycle() {
        let mut cache = CalendarCache::new();
        assert!(cache.needs_refresh());
        cache.set_loading();
        assert!(!cache.needs_refresh());
        cache.set_result(Ok(vec![]));
        assert!(!cache.needs_refresh());
    }
}
