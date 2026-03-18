use chrono::{Duration, Local, NaiveDate};
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Tabs},
    Frame, Terminal,
};
use rusqlite::Connection;
use std::sync::mpsc;
use std::time;

use crate::config::Config;
use crate::integrations::{calendar, gitlab, jira, slack, weather};
use crate::ui::dashboard::{DashboardAction, DashboardTab};
use crate::ui::goals::{GoalAction, GoalsTab};
use crate::ui::logs::{LogAction, LogsTab};
use crate::ui::tasks::{TaskAction, TasksTab};

#[derive(Debug)]
enum BackgroundMsg {
    WeatherResult(Result<weather::WeatherData, String>),
    JiraResult(Result<Vec<jira::JiraIssue>, String>),
    GitlabResult(Result<Vec<gitlab::MergeRequest>, String>),
    SlackResult(Result<Vec<slack::SlackMessage>, String>),
    CalendarResult(Result<Vec<calendar::CalendarEvent>, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Goals,
    Logs,
    Dashboard,
}

const TABS: &[(&str, Tab)] = &[
    ("Dashboard", Tab::Dashboard),
    ("Tasks", Tab::Tasks),
    ("Goals", Tab::Goals),
    ("Logs", Tab::Logs),
];

pub struct App {
    pub should_quit: bool,
    current_tab: Tab,
    view_date: NaiveDate,
    tasks_tab: TasksTab,
    goals_tab: GoalsTab,
    logs_tab: LogsTab,
    dashboard_tab: DashboardTab,
    conn: Connection,
    config: Config,
    bg_tx: mpsc::SyncSender<BackgroundMsg>,
    bg_rx: mpsc::Receiver<BackgroundMsg>,
}

impl App {
    pub fn new(conn: Connection, config: Config) -> Self {
        let today = Local::now().date_naive();
        let tasks_tab = TasksTab::new(&conn, today);
        let goals_tab = GoalsTab::new(&conn, today);
        let logs_tab = LogsTab::new(&conn, today);
        let dashboard_tab = DashboardTab::new();
        let (bg_tx, bg_rx) = mpsc::sync_channel(16);
        Self {
            should_quit: false,
            current_tab: Tab::Dashboard,
            view_date: today,
            tasks_tab,
            goals_tab,
            logs_tab,
            dashboard_tab,
            conn,
            config,
            bg_tx,
            bg_rx,
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        loop {
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
                }
            }

            if event::poll(time::Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            if self.should_quit {
                break;
            }
        }
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
        let location = self.config.weather.location.clone();
        let units = self.config.weather.units.clone();
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(weather::fetch(&location, &units));
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
        let handle = tokio::runtime::Handle::current();
        std::thread::spawn(move || {
            let result = handle.block_on(gitlab::fetch(&base_url, &token));
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

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        let capturing = match self.current_tab {
            Tab::Tasks => self.tasks_tab.is_capturing_input(),
            Tab::Goals => self.goals_tab.is_capturing_input(),
            Tab::Logs => self.logs_tab.is_capturing_input(),
            Tab::Dashboard => self.dashboard_tab.is_capturing_input(),
        };

        if !capturing {
            if key.code == KeyCode::Tab {
                self.cycle_tab(1);
                return;
            }
            if key.code == KeyCode::BackTab {
                self.cycle_tab(-1);
                return;
            }
            if key.code == KeyCode::Char(',') {
                self.navigate_date(-1);
                return;
            }
            if key.code == KeyCode::Char('.') {
                self.navigate_date(1);
                return;
            }
        }

        match self.current_tab {
            Tab::Tasks => {
                if let TaskAction::Quit = self.tasks_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
            Tab::Goals => {
                if let GoalAction::Quit = self.goals_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
            Tab::Logs => {
                if let LogAction::Quit = self.logs_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
            Tab::Dashboard => match self.dashboard_tab.handle_key(key) {
                DashboardAction::RefreshAll => self.refresh_all_integrations(),
                DashboardAction::OpenUrl(url) => {
                    let _ = std::process::Command::new("open").arg(&url).spawn();
                }
                DashboardAction::Quit => self.should_quit = true,
                DashboardAction::None => {}
            },
        }
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
                self.logs_tab.reload(&self.conn);
            }
            Tab::Goals => {
                self.view_date = self.view_date + Duration::weeks(delta as i64);
                self.goals_tab.week = self.view_date;
                self.goals_tab.reload(&self.conn);
            }
            Tab::Dashboard => {}
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        self.render_tab_bar(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
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

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        format!(" pulse [{}] ", date_label),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .title(
                        ratatui::widgets::block::Title::from(Span::styled(
                            " Tab:switch  ,/.:nav  q:quit ",
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
            Tab::Goals => self.goals_tab.render(frame, area),
            Tab::Logs => self.logs_tab.render(frame, area),
            Tab::Dashboard => self.dashboard_tab.render(frame, area, &self.conn),
        }
    }
}
