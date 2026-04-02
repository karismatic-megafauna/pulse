# Pulse

A personal productivity dashboard for the terminal, built with Rust and [Ratatui](https://github.com/ratatui/ratatui).

Pulse brings together your tasks, habits, journal, notes, and work integrations (Jira, GitLab, Slack, Calendar) into a single TUI that you can glance at throughout the day.

## Features

### Daily Start Screen

Every day on first launch, Pulse greets you with a daily start screen showing:

- An inspirational quote
- Yesterday's incomplete tasks, automatically rolled over to today
- Your habits for the week with check-in status

Dismiss it with `Enter` or `q` to jump into your day. The "new day" boundary is configurable (default: 4 AM) so late-night sessions don't trigger a new day prematurely.

### Dashboard

The central hub with seven navigable panels:

- **Tasks** -- today's task list, toggle completion with `x`
- **Habits** -- weekly habit progress with check-in slots
- **Jira** -- your assigned open issues (filterable, press `f` to hide done)
- **GitLab** -- your authored and review-requested merge requests
- **Slack** -- latest DMs from important users with relative timestamps
- **Calendar** -- upcoming events from macOS Calendar
- **Notes** -- recent notes with quick access

Cycle between panels with `n`/`N`, scroll with `j`/`k`, and press `Enter` to open items in the browser or jump to the relevant tab.

### Tasks

Daily task lists tied to a specific date. Navigate between dates with `,` and `.`.

- Add tasks with `a`, complete with `x`, delete with `d`
- Start a focus timer on any task with `s`

### Habits

Weekly habit tracking with configurable frequency goals (e.g., "Exercise 3x/week").

- Visual progress slots: `[xx ]` shows 2 of 3 check-ins done
- Streak tracking across consecutive weeks
- Pause/resume habits with `p`

### Journal

One markdown file per day, stored in `~/.config/pulse/journals/`. Press `e` to open in your `$EDITOR`. Navigate between dates with `,` and `.`. Rendered with inline markdown formatting when viewing.

### Notes

A collection of freeform markdown notes stored in `~/.config/pulse/notes/`. Includes a live markdown preview pane. Create, edit, and delete notes from within the app.

### Focus Timer

A Pomodoro-style timer with three configurable durations (default 15/25/45 minutes):

1. Select a task and press `s`
2. Pick a duration (Short, Medium, Long)
3. Timer runs in the background with a countdown display
4. Optionally blocks distracting websites during the session via macOS packet filter (`pf`)
5. Sessions are recorded to the database for historical tracking

Cancel anytime with `Esc`.

### Weather

Auto-detects your location via macOS CoreLocation (compiles a small Swift helper on first use) and fetches current conditions from the [Open-Meteo API](https://open-meteo.com/) -- no API key required. Shows temperature, feels-like, humidity, wind, and a condition icon. Falls back gracefully if location access is denied.

### Integrations

All integrations are optional and disabled by default. Enable them in your config file.

| Integration | What it shows | Requirements |
|-------------|--------------|--------------|
| **Jira** | Assigned open issues | Jira API token |
| **GitLab** | Your MRs + MRs awaiting your review | GitLab personal access token |
| **Slack** | Latest DMs from configured important users | Slack bot token |
| **Calendar** | Upcoming events from macOS Calendar | `icalBuddy` (`brew install ical-buddy`) |
| **Weather** | Current local conditions | macOS (for auto-location) |

## Installation

### Prerequisites

- **Rust** (1.70+) -- install via [rustup](https://rustup.rs/)
- **macOS** recommended (Calendar and weather location features are macOS-specific; everything else works cross-platform)

### Optional dependencies

- `icalBuddy` -- for Calendar integration: `brew install ical-buddy`
- A terminal editor (`$EDITOR`) -- for editing journal entries and notes (defaults to `vim`)

### Build & run

```sh
git clone <repo-url> && cd pulse
cargo build --release
./target/release/pulse
```

Or run directly:

```sh
cargo run --release
```

## Configuration

Pulse stores its configuration and data in `~/.config/pulse/`. On first run, a default `config.toml` is created. You can also copy the example:

```sh
cp config.example.toml ~/.config/pulse/config.toml
```

### Full config reference

```toml
[general]
weight_unit = "lbs"       # "lbs" or "kg"
new_day_hour = 4          # Hour (0-23) when a new day starts (default: 4 AM)

[weather]
enabled = true
units = "imperial"        # "imperial" (F, mph) or "metric" (C, km/h)
# Location is auto-detected from macOS CoreLocation

[jira]
enabled = false
base_url = "https://yourcompany.atlassian.net"
email = "you@company.com"
api_token = "your-api-token"

[gitlab]
enabled = false
base_url = "https://gitlab.com"
private_token = "your-private-token"
project = ""              # Optional: scope to a specific project (e.g., "group/project")
ignore_authors = []       # Optional: hide MRs from these authors (e.g., bots)

[slack]
enabled = false
bot_token = "xoxb-your-bot-token"
important_users = ["U12345678", "U87654321"]  # Slack user IDs to watch

[calendar]
enabled = false
num_events = 5            # Number of upcoming events to show

[notes]
editor = ""               # Leave empty to use $EDITOR, or set explicitly ("nvim", "code", etc.)

[focus_timer]
short_minutes = 15
medium_minutes = 25
long_minutes = 45
blocked_sites = ["reddit.com", "www.reddit.com"]  # Sites to block during focus sessions
```

## Keyboard Shortcuts

### Global

| Key | Action |
|-----|--------|
| `Tab` | Next tab |
| `Shift+Tab` | Previous tab |
| `,` | Previous date |
| `.` | Next date |
| `Ctrl+C` | Quit |
| `Esc` | Cancel focus timer / dismiss dialogs |

### Dashboard

| Key | Action |
|-----|--------|
| `n` / `N` | Next / previous panel |
| `j` / `k` | Scroll down / up in focused panel |
| `x` | Toggle task or habit in focused panel |
| `s` | Start focus timer on selected task |
| `f` | Toggle Jira "hide done" filter |
| `r` | Refresh all integrations |
| `Enter` | Open selected item |

### Tasks

| Key | Action |
|-----|--------|
| `a` | Add task |
| `x` / `Space` | Toggle completion |
| `s` | Start focus timer |
| `d` | Delete task |
| `j` / `k` | Move selection |

### Habits

| Key | Action |
|-----|--------|
| `a` | Add habit (format: `Title \| Frequency`) |
| `x` / `Space` | Check in for today |
| `p` | Pause / resume habit |
| `d` | Delete habit |
| `j` / `k` | Move selection |

### Journal

| Key | Action |
|-----|--------|
| `e` / `Enter` | Edit in `$EDITOR` |
| `j` / `k` | Scroll |

### Notes

| Key | Action |
|-----|--------|
| `n` | New note |
| `e` / `Enter` | Edit in `$EDITOR` |
| `d` | Delete note |
| `j` / `k` | Move selection |
| `J` / `K` | Scroll preview |

## Data Storage

| What | Where |
|------|-------|
| Database (tasks, habits, focus sessions) | `~/.config/pulse/pulse.db` |
| Journal entries | `~/.config/pulse/journals/YYYY-MM-DD.md` |
| Notes | `~/.config/pulse/notes/*.md` |
| Configuration | `~/.config/pulse/config.toml` |

## License

MIT -- see [LICENSE](LICENSE) for details.
