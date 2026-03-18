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
            "-po", "title,datetime",       // title first, datetime indented below
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

/// Parse icalBuddy output in its default multi-line format:
///   • Title
///       Wed 18 at 09:30
///   • All-Day Event
///       Tue 17
///
/// Non-indented lines (starting with "• ") are titles.
/// Indented lines below them are the datetime.
fn parse_icalbuddy_output(raw: &str) -> Vec<CalendarEvent> {
    let mut events = Vec::new();
    let mut current_title: Option<String> = None;

    for line in raw.lines() {
        if line.is_empty() {
            continue;
        }

        let is_indented = line.starts_with(' ') || line.starts_with('\t');

        if !is_indented {
            // New event — save any pending one without a datetime
            if let Some(title) = current_title.take() {
                events.push(CalendarEvent {
                    time: String::new(),
                    title,
                    calendar: String::new(),
                });
            }
            // Strip bullet prefix
            let title = line.trim().trim_start_matches("• ").to_string();
            current_title = Some(title);
        } else if let Some(title) = current_title.take() {
            // Indented line = datetime for the current event
            let datetime = line.trim();
            let time = if let Some(at_idx) = datetime.find(" at ") {
                let day = &datetime[..at_idx];
                let clock = &datetime[at_idx + 4..];
                format!("{} {}", day, clock)
            } else {
                // All-day event
                format!("{} all day", datetime)
            };
            events.push(CalendarEvent {
                time,
                title,
                calendar: String::new(),
            });
        }
    }

    // Flush last event if it had no datetime line
    if let Some(title) = current_title {
        events.push(CalendarEvent {
            time: String::new(),
            title,
            calendar: String::new(),
        });
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_timed_events() {
        let raw = "• Daily Standup\n    Wed 18 at 09:30\n• 1:1 with manager\n    Wed 18 at 14:00\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].title, "Daily Standup");
        assert_eq!(events[0].time, "Wed 18 09:30");
        assert_eq!(events[1].title, "1:1 with manager");
        assert_eq!(events[1].time, "Wed 18 14:00");
    }

    #[test]
    fn test_parse_all_day_events() {
        let raw = "• St. Patrick's Day\n    Tue 17\n";
        let events = parse_icalbuddy_output(raw);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "St. Patrick's Day");
        assert_eq!(events[0].time, "Tue 17 all day");
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
