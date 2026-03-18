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
            "-li", &num_events.to_string(), // limit number
            "-nc",                         // no calendar names in title
            "-nrd",                        // no relative dates
            "-npn",                        // no property names
            "-tf", "%H:%M",               // time format (24h)
            "-df", "%a %d",               // date format (short)
            "-eed",                        // exclude end datetimes
            "-iep", "title,datetime",      // only these properties
            "-po", "datetime,title",       // datetime first
            "-ps", "///",               // property separator
            "-ss", "",                     // no section separator
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
            // Format: "Wed 18 at 09:30///Daily Standup" (timed)
            //      or "Tue 17///St. Patrick's Day"         (all-day)
            let parts: Vec<&str> = trimmed.splitn(2, "///").collect();
            if parts.len() < 2 {
                return Some(CalendarEvent {
                    time: String::new(),
                    title: trimmed.to_string(),
                    calendar: String::new(),
                });
            }

            let datetime_part = parts[0].trim();
            let title = parts[1].trim().to_string();

            // Extract time from "Wed 18 at 09:30" or mark as all-day
            let time = if let Some(at_idx) = datetime_part.find(" at ") {
                let time_str = &datetime_part[at_idx + 4..];
                // Include the day prefix for context (e.g. "Wed 18 09:30")
                let day_prefix = &datetime_part[..at_idx];
                format!("{} {}", day_prefix, time_str)
            } else {
                // All-day event — just show the day
                format!("{} all day", datetime_part)
            };

            Some(CalendarEvent {
                time,
                title,
                calendar: String::new(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timed_events() {
        let raw = "Wed 18 at 09:30///Daily Standup\nWed 18 at 14:00///1:1 with manager\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].time, "Wed 18 09:30");
        assert_eq!(events[0].title, "Daily Standup");
        assert_eq!(events[1].time, "Wed 18 14:00");
        assert_eq!(events[1].title, "1:1 with manager");
    }

    #[test]
    fn test_parse_all_day_events() {
        let raw = "Tue 17///St. Patrick's Day\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].time, "Tue 17 all day");
        assert_eq!(events[0].title, "St. Patrick's Day");
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
