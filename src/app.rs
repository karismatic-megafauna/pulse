use chrono::{Duration, Local, NaiveDate, Timelike};
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Tabs},
    Frame, Terminal,
};
use rusqlite::Connection;
use std::sync::mpsc;
use std::time;

/// Alliterative bird name for the current version.
/// Advance alphabetically with each release:
///   0.1.0 = Ambitious Albatross
///   0.2.0 = Boisterous Budgie
///   0.3.0 = Cunning Cormorant
///   …and so on.
const VERSION_NAME: &str = "Ambitious Albatross";
const BUILD_TIME: &str = env!("PULSE_BUILD_TIME");

use crate::config::Config;
use crate::integrations::{calendar, gitlab, jira, slack, weather};
use crate::models::focus_session;
use crate::site_blocker;
use crate::ui::daily_start::{self, DailyStartScreen};
use crate::ui::dashboard::{DashboardAction, DashboardTab};
use crate::ui::habits::{HabitAction, HabitsTab};
use crate::ui::logs::{LogAction, LogsTab};
use crate::ui::notes::{NotesAction, NotesTab};
use crate::ui::tasks::{TaskAction, TasksTab};

#[derive(Debug)]
enum BackgroundMsg {
    WeatherResult(Result<weather::WeatherData, String>),
    JiraResult(Result<Vec<jira::JiraIssue>, String>),
    GitlabResult(Result<Vec<gitlab::MergeRequest>, String>),
    SlackResult(Result<Vec<slack::SlackMessage>, String>),
    CalendarResult(Result<Vec<calendar::CalendarEvent>, String>),
    QuoteResult(Result<(String, String), String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Habits,
    Logs,
    Notes,
    Dashboard,
}

const TABS: &[(&str, Tab)] = &[
    ("Dashboard", Tab::Dashboard),
    ("Tasks", Tab::Tasks),
    ("Habits", Tab::Habits),
    ("Journal", Tab::Logs),
    ("Notes", Tab::Notes),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FocusDuration {
    Short,
    Medium,
    Long,
}

impl FocusDuration {
    fn next(self) -> Self {
        match self {
            Self::Short => Self::Medium,
            Self::Medium => Self::Long,
            Self::Long => Self::Long,
        }
    }
    fn prev(self) -> Self {
        match self {
            Self::Short => Self::Short,
            Self::Medium => Self::Short,
            Self::Long => Self::Medium,
        }
    }
}

struct FocusTimerPicker {
    task_id: i64,
    task_title: String,
    selected: FocusDuration,
}

struct ActiveTimer {
    task_id: i64,
    task_title: String,
    start: time::Instant,
    duration: time::Duration,
    started_at: String,
}

pub struct App {
    pub should_quit: bool,
    current_tab: Tab,
    view_date: NaiveDate,
    tasks_tab: TasksTab,
    habits_tab: HabitsTab,
    logs_tab: LogsTab,
    notes_tab: NotesTab,
    dashboard_tab: DashboardTab,
    daily_start: Option<DailyStartScreen>,
    conn: Connection,
    config: Config,
    bg_tx: mpsc::SyncSender<BackgroundMsg>,
    bg_rx: mpsc::Receiver<BackgroundMsg>,
    active_timer: Option<ActiveTimer>,
    timer_picker: Option<FocusTimerPicker>,
    confirm_quit: bool,
}

impl App {
    /// Returns the "effective" date, accounting for the configured new-day hour.
    /// Before that hour, it's still considered the previous day.
    fn effective_today(&self) -> NaiveDate {
        Self::effective_date(self.config.general.new_day_hour)
    }

    fn effective_date(new_day_hour: u32) -> NaiveDate {
        let now = Local::now();
        let date = now.date_naive();
        if now.hour() < new_day_hour {
            date - Duration::days(1)
        } else {
            date
        }
    }

    pub fn new(conn: Connection, config: Config) -> Self {
        let today = Self::effective_date(config.general.new_day_hour);
        let tasks_tab = TasksTab::new(&conn, today);
        let habits_tab = HabitsTab::new(&conn);
        let logs_tab = LogsTab::new(today);
        let notes_tab = NotesTab::new();
        let dashboard_tab = DashboardTab::new();
        let (bg_tx, bg_rx) = mpsc::sync_channel(16);

        // Show daily start screen if this is the first open today
        let last_opened = daily_start::get_last_opened_date(&conn);
        let daily_start = if last_opened != Some(today) {
            Some(DailyStartScreen::new(&conn, today))
        } else {
            None
        };

        Self {
            should_quit: false,
            current_tab: Tab::Dashboard,
            view_date: today,
            tasks_tab,
            habits_tab,
            logs_tab,
            notes_tab,
            dashboard_tab,
            daily_start,
            conn,
            config,
            bg_tx,
            bg_rx,
            active_timer: None,
            timer_picker: None,
            confirm_quit: false,
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        // Kick off quote fetch if daily start is showing
        if self.daily_start.is_some() {
            self.spawn_quote_fetch();
        }

        loop {
            // Check if the day has changed while the app is running
            if self.daily_start.is_none() {
                let effective = self.effective_today();
                let last_opened = daily_start::get_last_opened_date(&self.conn);
                if last_opened != Some(effective) {
                    self.daily_start = Some(DailyStartScreen::new(&self.conn, effective));
                    self.spawn_quote_fetch();
                    self.view_date = effective;
                    self.tasks_tab.date = effective;
                    self.tasks_tab.reload(&self.conn);
                    self.habits_tab.reload(&self.conn);
                }
            }

            self.kick_off_background_fetches();

            terminal.draw(|f| self.render(f))?;

            // Drain background results (non-blocking)
            while let Ok(msg) = self.bg_rx.try_recv() {
                match msg {
                    BackgroundMsg::WeatherResult(res) => {
                        self.dashboard_tab.weather_cache.set_result(res);
                    }
                    BackgroundMsg::JiraResult(res) => {
                        self.dashboard_tab.jira_cache.set_result(res);
                    }
                    BackgroundMsg::GitlabResult(res) => {
                        self.dashboard_tab.gitlab_cache.set_result(res);
                    }
                    BackgroundMsg::SlackResult(res) => {
                        self.dashboard_tab.slack_cache.set_result(res);
                    }
                    BackgroundMsg::CalendarResult(res) => {
                        self.dashboard_tab.calendar_cache.set_result(res);
                    }
                    BackgroundMsg::QuoteResult(res) => {
                        if let (Ok((text, author)), Some(ds)) = (res, &mut self.daily_start) {
                            ds.quote_text = text;
                            ds.quote_author = author;
                        }
                    }
                }
            }

            // Check if focus timer has completed
            if let Some(timer) = &self.active_timer {
                if timer.start.elapsed() >= timer.duration {
                    let task_title = timer.task_title.clone();
                    let task_id = timer.task_id;
                    let duration_secs = timer.duration.as_secs() as i64;
                    let started_at = timer.started_at.clone();
                    let ended_at = Local::now().to_rfc3339();
                    self.active_timer = None;
                    site_blocker::unblock_sites();

                    let _ = focus_session::insert_session(
                        &self.conn,
                        task_id,
                        &task_title,
                        duration_secs,
                        true,
                        &started_at,
                        &ended_at,
                    );

                    Self::fire_focus_notification(&task_title);
                }
            }

            if event::poll(time::Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    // Daily start screen intercepts all input
                    if let Some(ds) = &mut self.daily_start {
                        ds.handle_key(key, &self.conn);
                        if ds.dismissed {
                            let today = self.effective_today();
                            daily_start::set_last_opened_date(&self.conn, today);
                            // Reload tasks/goals since user may have added some
                            self.tasks_tab.reload(&self.conn);
                            self.habits_tab.reload(&self.conn);
                            self.daily_start = None;
                        }
                        continue;
                    }

                    if let Some(path) = self.handle_key(key) {
                        self.open_editor_path(terminal, &path)?;
                        self.notes_tab.reload();
                        self.dashboard_tab.reload_notes();
                        self.logs_tab.reload();
                    }
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    /// Suspend TUI, open $EDITOR on a file, restore TUI.
    fn open_editor_path<B: ratatui::backend::Backend>(
        &self,
        terminal: &mut Terminal<B>,
        path: &std::path::Path,
    ) -> Result<()> {
        use crossterm::{execute, terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen, EnterAlternateScreen}};
        use std::io;

        let editor = if !self.config.notes.editor.is_empty() {
            self.config.notes.editor.clone()
        } else {
            std::env::var("EDITOR").unwrap_or_else(|_| "vim".to_string())
        };

        // Suspend TUI
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        // Run editor
        let _ = std::process::Command::new(&editor)
            .arg(path)
            .status();

        // Restore TUI
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        terminal.hide_cursor()?;
        terminal.clear()?;

        Ok(())
    }

    fn kick_off_background_fetches(&mut self) {
        // Weather
        if self.config.weather.enabled && self.dashboard_tab.weather_cache.needs_refresh() {
            self.spawn_weather_fetch();
        }
        // Jira
        if self.config.jira.enabled && self.dashboard_tab.jira_cache.needs_refresh() {
            self.spawn_jira_fetch();
        }
        // GitLab
        if self.config.gitlab.enabled && self.dashboard_tab.gitlab_cache.needs_refresh() {
            self.spawn_gitlab_fetch();
        }
        // Slack
        if self.config.slack.enabled && self.dashboard_tab.slack_cache.needs_refresh() {
            self.spawn_slack_fetch();
        }
        // Calendar
        if self.config.calendar.enabled && self.dashboard_tab.calendar_cache.needs_refresh() {
            self.spawn_calendar_fetch();
        }
    }

    fn spawn_weather_fetch(&mut self) {
        self.dashboard_tab.weather_cache.set_loading();
        let tx = self.bg_tx.clone();
        let units = self.config.weather.units.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(weather::fetch(&units));
            let _ = tx.send(BackgroundMsg::WeatherResult(result));
        });
    }

    fn spawn_jira_fetch(&mut self) {
        self.dashboard_tab.jira_cache.set_loading();
        let tx = self.bg_tx.clone();
        let base_url = self.config.jira.base_url.clone();
        let email = self.config.jira.email.clone();
        let api_token = self.config.jira.api_token.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(jira::fetch(&base_url, &email, &api_token));
            let _ = tx.send(BackgroundMsg::JiraResult(result));
        });
    }

    fn spawn_gitlab_fetch(&mut self) {
        self.dashboard_tab.gitlab_cache.set_loading();
        let tx = self.bg_tx.clone();
        let base_url = self.config.gitlab.base_url.clone();
        let token = self.config.gitlab.private_token.clone();
        let project = self.config.gitlab.project.clone();
        let ignore_authors = self.config.gitlab.ignore_authors.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(gitlab::fetch(&base_url, &token, &project, &ignore_authors));
            let _ = tx.send(BackgroundMsg::GitlabResult(result));
        });
    }

    fn spawn_slack_fetch(&mut self) {
        self.dashboard_tab.slack_cache.set_loading();
        let tx = self.bg_tx.clone();
        let bot_token = self.config.slack.bot_token.clone();
        let users = self.config.slack.important_users.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(slack::fetch(&bot_token, &users));
            let _ = tx.send(BackgroundMsg::SlackResult(result));
        });
    }

    fn spawn_calendar_fetch(&mut self) {
        self.dashboard_tab.calendar_cache.set_loading();
        let tx = self.bg_tx.clone();
        let num_events = self.config.calendar.num_events;
        // calendar::fetch is sync (subprocess), so no tokio handle needed
        std::thread::spawn(move || {
            let result = calendar::fetch(num_events);
            let _ = tx.send(BackgroundMsg::CalendarResult(result));
        });
    }

    fn spawn_quote_fetch(&self) {
        let tx = self.bg_tx.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(async {
                let client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(5))
                    .build()
                    .map_err(|e| e.to_string())?;
                let resp = client
                    .get("https://zenquotes.io/api/random")
                    .header("Accept", "application/json")
                    .send()
                    .await
                    .map_err(|e| format!("Quote fetch failed: {}", e))?;
                let json: Vec<serde_json::Value> = resp
                    .json()
                    .await
                    .map_err(|e| format!("Quote parse failed: {}", e))?;
                let quote = json.first().ok_or("Empty response")?;
                let text = quote["q"].as_str().unwrap_or("").to_string();
                let author = quote["a"].as_str().unwrap_or("").to_string();
                Ok((text, author))
            });
            let _ = tx.send(BackgroundMsg::QuoteResult(result));
        });
    }

    fn refresh_all_integrations(&mut self) {
        if self.config.weather.enabled {
            self.spawn_weather_fetch();
        }
        if self.config.jira.enabled {
            self.spawn_jira_fetch();
        }
        if self.config.gitlab.enabled {
            self.spawn_gitlab_fetch();
        }
        if self.config.slack.enabled {
            self.spawn_slack_fetch();
        }
        if self.config.calendar.enabled {
            self.spawn_calendar_fetch();
        }
    }

    /// Returns Some(path) if a file should be opened in $EDITOR.
    fn handle_key(&mut self, key: KeyEvent) -> Option<std::path::PathBuf> {
        // Quit confirmation intercepts all input
        if self.confirm_quit {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => self.should_quit = true,
                _ => self.confirm_quit = false,
            }
            return None;
        }

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }

        // Timer picker intercepts all input
        if let Some(picker) = &mut self.timer_picker {
            match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    picker.selected = picker.selected.next();
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    picker.selected = picker.selected.prev();
                }
                KeyCode::Enter => {
                    let minutes = match picker.selected {
                        FocusDuration::Short => self.config.focus_timer.short_minutes,
                        FocusDuration::Medium => self.config.focus_timer.medium_minutes,
                        FocusDuration::Long => self.config.focus_timer.long_minutes,
                    };
                    self.active_timer = Some(ActiveTimer {
                        task_id: picker.task_id,
                        task_title: picker.task_title.clone(),
                        start: time::Instant::now(),
                        duration: time::Duration::from_secs(minutes as u64 * 60),
                        started_at: Local::now().to_rfc3339(),
                    });
                    site_blocker::block_sites(&self.config.focus_timer.blocked_sites);
                    self.timer_picker = None;
                }
                KeyCode::Esc => {
                    self.timer_picker = None;
                }
                _ => {}
            }
            return None;
        }

        let capturing = match self.current_tab {
            Tab::Tasks => self.tasks_tab.is_capturing_input(),
            Tab::Habits => self.habits_tab.is_capturing_input(),
            Tab::Logs => self.logs_tab.is_capturing_input(),
            Tab::Notes => self.notes_tab.is_capturing_input(),
            Tab::Dashboard => self.dashboard_tab.is_capturing_input(),
        };

        if !capturing {
            // Cancel active focus timer with Esc
            if key.code == KeyCode::Esc && self.active_timer.is_some() {
                if let Some(timer) = self.active_timer.take() {
                    site_blocker::unblock_sites();
                    let ended_at = Local::now().to_rfc3339();
                    let _ = focus_session::insert_session(
                        &self.conn,
                        timer.task_id,
                        &timer.task_title,
                        timer.duration.as_secs() as i64,
                        false,
                        &timer.started_at,
                        &ended_at,
                    );
                }
                return None;
            }
            if key.code == KeyCode::Tab {
                self.cycle_tab(1);
                return None;
            }
            if key.code == KeyCode::BackTab {
                self.cycle_tab(-1);
                return None;
            }
            if key.code == KeyCode::Char(',') {
                self.navigate_date(-1);
                return None;
            }
            if key.code == KeyCode::Char('.') {
                self.navigate_date(1);
                return None;
            }
        }

        match self.current_tab {
            Tab::Tasks => {
                match self.tasks_tab.handle_key(key, &self.conn) {
                    TaskAction::Quit => self.confirm_quit = true,
                    TaskAction::StartFocusTimer(id, title) => {
                        if self.active_timer.is_none() {
                            self.timer_picker = Some(FocusTimerPicker {
                                task_id: id,
                                task_title: title,
                                selected: FocusDuration::Medium,
                            });
                        }
                    }
                    TaskAction::None => {}
                }
            }
            Tab::Habits => {
                if let HabitAction::Quit = self.habits_tab.handle_key(key, &self.conn) {
                    self.confirm_quit = true;
                }
            }
            Tab::Logs => match self.logs_tab.handle_key(key) {
                LogAction::EditJournal(path) => return Some(path),
                LogAction::Quit => self.confirm_quit = true,
                LogAction::None => {}
            },
            Tab::Notes => match self.notes_tab.handle_key(key) {
                NotesAction::EditNote(filename) => {
                    return Some(crate::models::note::note_path(&filename));
                }
                NotesAction::Quit => self.confirm_quit = true,
                NotesAction::None => {}
            },
            Tab::Dashboard => match self.dashboard_tab.handle_key(key) {
                DashboardAction::RefreshAll => self.refresh_all_integrations(),
                DashboardAction::OpenUrl(url) => {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
                DashboardAction::SwitchToNotes => {
                    self.current_tab = Tab::Notes;
                    self.notes_tab.reload();
                }
                DashboardAction::SwitchToTasks => {
                    self.current_tab = Tab::Tasks;
                    self.tasks_tab.reload(&self.conn);
                }
                DashboardAction::SwitchToHabits => {
                    self.current_tab = Tab::Habits;
                    self.habits_tab.reload(&self.conn);
                }
                DashboardAction::ToggleTask(id) => {
                    let _ = crate::models::task::toggle_complete(&self.conn, id);
                }
                DashboardAction::ToggleHabit(id) => {
                    let today = Local::now().date_naive();
                    let _ = crate::models::habit::toggle_checkin(&self.conn, id, today);
                }
                DashboardAction::StartFocusTimer(id, title) => {
                    if self.active_timer.is_none() {
                        self.timer_picker = Some(FocusTimerPicker {
                            task_id: id,
                            task_title: title,
                            selected: FocusDuration::Medium,
                        });
                    }
                }
                DashboardAction::Quit => self.confirm_quit = true,
                DashboardAction::None => {}
            },
        }
        None
    }

    fn cycle_tab(&mut self, direction: i32) {
        let idx = TABS
            .iter()
            .position(|(_, t)| *t == self.current_tab)
            .unwrap_or(0) as i32;
        let next = ((idx + direction).rem_euclid(TABS.len() as i32)) as usize;
        self.current_tab = TABS[next].1;
    }

    fn navigate_date(&mut self, delta: i32) {
        match self.current_tab {
            Tab::Tasks | Tab::Logs => {
                self.view_date = self.view_date + Duration::days(delta as i64);
                self.tasks_tab.date = self.view_date;
                self.tasks_tab.reload(&self.conn);
                self.logs_tab.date = self.view_date;
                self.logs_tab.reload();
            }
            Tab::Habits | Tab::Notes | Tab::Dashboard => {}
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Daily start screen takes over the whole screen
        if let Some(ds) = &self.daily_start {
            ds.render(frame, area);
            return;
        }

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        self.render_tab_bar(frame, chunks[0]);
        self.render_content(frame, chunks[1]);

        // Timer picker popup (rendered on top of everything)
        if let Some(picker) = &self.timer_picker {
            self.render_timer_picker(frame, area, picker);
        }

        // Quit confirmation popup
        if self.confirm_quit {
            self.render_confirm_quit(frame, area);
        }
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let today = Local::now().date_naive();
        let date_label = if self.view_date == today {
            "today".to_string()
        } else {
            self.view_date.format("%b %-d").to_string()
        };

        let titles: Vec<Line> = TABS
            .iter()
            .map(|(name, _)| Line::from(Span::raw(*name)))
            .collect();

        let selected = TABS
            .iter()
            .position(|(_, t)| *t == self.current_tab)
            .unwrap_or(0);

        let (title_spans, bottom_hint) = if let Some(timer) = &self.active_timer {
            let elapsed = timer.start.elapsed();
            let remaining = timer.duration.saturating_sub(elapsed);
            let mins = remaining.as_secs() / 60;
            let secs = remaining.as_secs() % 60;

            // Truncate task title to keep it readable
            let max_title_len = 20;
            let title = if timer.task_title.len() > max_title_len {
                format!("{}...", &timer.task_title[..max_title_len])
            } else {
                timer.task_title.clone()
            };

            (
                Line::from(vec![
                    Span::styled(
                        format!(" pulse [{}] ", date_label),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("| {} {:02}:{:02} ", title, mins, secs),
                        Style::default()
                            .fg(Color::Red)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                " Tab:switch  ,/.:nav  Esc:cancel timer  q:quit ",
            )
        } else {
            (
                Line::from(Span::styled(
                    format!(" pulse [{}] ", date_label),
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                )),
                " Tab:switch  ,/.:nav  q:quit ",
            )
        };

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title_spans)
                    .title(
                        Line::from(Span::styled(
                            format!(" {} \u{00b7} {} ", VERSION_NAME, BUILD_TIME),
                            Style::default().fg(Color::DarkGray),
                        ))
                        .alignment(ratatui::layout::Alignment::Right),
                    )
                    .title_bottom(
                        Line::from(Span::styled(
                            bottom_hint,
                            Style::default().fg(Color::DarkGray),
                        ))
                        .alignment(ratatui::layout::Alignment::Right),
                    ),
            )
            .select(selected)
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(Color::DarkGray));

        frame.render_widget(tabs, area);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        match self.current_tab {
            Tab::Tasks => self.tasks_tab.render(frame, area),
            Tab::Habits => self.habits_tab.render(frame, area),
            Tab::Logs => self.logs_tab.render(frame, area),
            Tab::Notes => self.notes_tab.render(frame, area),
            Tab::Dashboard => self.dashboard_tab.render(frame, area, &self.conn),
        }
    }

    fn render_timer_picker(&self, frame: &mut Frame, area: Rect, picker: &FocusTimerPicker) {
        use ratatui::layout::Alignment;
        use ratatui::widgets::Clear;

        // Centered popup: 34 wide, 7 tall
        let popup_width = 34u16;
        let popup_height = 7u16;
        let x = area.x + area.width.saturating_sub(popup_width) / 2;
        let y = area.y + area.height.saturating_sub(popup_height) / 2;
        let popup = Rect::new(x, y, popup_width.min(area.width), popup_height.min(area.height));

        frame.render_widget(Clear, popup);

        let options = [
            (
                FocusDuration::Short,
                format!("  Short  ({} min)", self.config.focus_timer.short_minutes),
            ),
            (
                FocusDuration::Medium,
                format!("  Medium ({} min)", self.config.focus_timer.medium_minutes),
            ),
            (
                FocusDuration::Long,
                format!("  Long   ({} min)", self.config.focus_timer.long_minutes),
            ),
        ];

        let items: Vec<ListItem> = options
            .iter()
            .map(|(dur, label)| {
                let selected = *dur == picker.selected;
                let prefix = if selected { "▸" } else { " " };
                let style = if selected {
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Span::styled(format!("{}{}", prefix, label), style))
            })
            .collect();

        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    " Focus Duration ",
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                ))
                .title_bottom(
                    Line::from(Span::styled(
                        " Enter:start  Esc:cancel ",
                        Style::default().fg(Color::DarkGray),
                    ))
                    .alignment(Alignment::Right),
                ),
        );

        frame.render_widget(list, popup);
    }

    fn render_confirm_quit(&self, frame: &mut Frame, area: Rect) {
        use ratatui::layout::Alignment;
        use ratatui::widgets::Clear;

        let popup_width = 30u16;
        let popup_height = 3u16;
        let x = area.x + area.width.saturating_sub(popup_width) / 2;
        let y = area.y + area.height.saturating_sub(popup_height) / 2;
        let popup = Rect::new(x, y, popup_width.min(area.width), popup_height.min(area.height));

        frame.render_widget(Clear, popup);

        let text = Paragraph::new(" Quit? y to confirm ")
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(Span::styled(
                        " Quit ",
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    )),
            );

        frame.render_widget(text, popup);
    }

    fn fire_focus_notification(task_title: &str) {
        let title = task_title.to_string();

        // Pleasant chime
        std::thread::spawn(|| {
            let _ = std::process::Command::new("afplay")
                .arg("/System/Library/Sounds/Breeze.aiff")
                .status();
        });

        // Calm text-to-speech
        let say_title = title.clone();
        std::thread::spawn(move || {
            let _ = std::process::Command::new("say")
                .args([
                    "-v",
                    "Samantha",
                    &format!("Focus timer complete. Time to check in on: {}", say_title),
                ])
                .status();
        });

        // Modal dialog that steals focus
        std::thread::spawn(move || {
            let escaped = title.replace('\\', "\\\\").replace('"', "\\\"");
            let script = format!(
                r#"display dialog "Focus timer complete!\n\nTask: {}\n\nHow did it go?" with title "Pulse Focus Timer" buttons {{"Done"}} default button "Done" with icon note"#,
                escaped
            );
            let _ = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .status();
        });
    }
}
