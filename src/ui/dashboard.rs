use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{block::BorderType, Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::integrations::calendar::{CalendarCache, CalendarState};
use crate::integrations::gitlab::{GitlabCache, GitlabState};
use crate::integrations::jira::{JiraCache, JiraState};
use crate::integrations::slack::{SlackCache, SlackState};
use crate::integrations::weather::{WeatherCache, WeatherState};
use crate::models::note::{self, NoteMeta};
use crate::models::{habit, task};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Panel {
    Tasks,
    Habits,
    Jira,
    GitLab,
    Slack,
    Calendar,
    Notes,
}

const PANELS: &[Panel] = &[
    Panel::Tasks,
    Panel::Habits,
    Panel::Jira,
    Panel::GitLab,
    Panel::Slack,
    Panel::Calendar,
    Panel::Notes,
];

pub struct DashboardTab {
    pub weather_cache: WeatherCache,
    pub jira_cache: JiraCache,
    pub gitlab_cache: GitlabCache,
    pub slack_cache: SlackCache,
    pub calendar_cache: CalendarCache,
    focus: Panel,
    jira_list_state: ListState,
    gitlab_list_state: ListState,
    jira_hide_done: bool,
    tasks: Vec<task::Task>,
    tasks_list_state: ListState,
    habits: Vec<habit::HabitWithProgress>,
    habits_list_state: ListState,
    notes: Vec<NoteMeta>,
    notes_list_state: ListState,
    calendar_scroll: u16,
    slack_scroll: u16,
}

impl DashboardTab {
    pub fn new() -> Self {
        let notes = note::list_notes().unwrap_or_default();
        let mut notes_list_state = ListState::default();
        if !notes.is_empty() {
            notes_list_state.select(Some(0));
        }
        Self {
            weather_cache: WeatherCache::new(),
            jira_cache: JiraCache::new(),
            gitlab_cache: GitlabCache::new(),
            slack_cache: SlackCache::new(),
            calendar_cache: CalendarCache::new(),
            focus: Panel::Tasks,
            jira_list_state: ListState::default(),
            gitlab_list_state: ListState::default(),
            jira_hide_done: true,
            tasks: vec![],
            tasks_list_state: ListState::default(),
            habits: vec![],
            habits_list_state: ListState::default(),
            notes,
            notes_list_state,
            calendar_scroll: 0,
            slack_scroll: 0,
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        false
    }

    pub fn reload_notes(&mut self) {
        self.notes = note::list_notes().unwrap_or_default();
        clamp_list_state(&mut self.notes_list_state, self.notes.len());
    }

    pub fn reload_tasks_and_habits(&mut self, conn: &Connection) {
        let today = Local::now().date_naive();
        self.tasks = task::list_for_date(conn, today).unwrap_or_default();
        clamp_list_state(&mut self.tasks_list_state, self.tasks.len());
        self.habits = habit::list_with_progress(conn, today).unwrap_or_default();
        clamp_list_state(&mut self.habits_list_state, self.habits.len());
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Char('n') => {
                self.cycle_panel(1);
                DashboardAction::None
            }
            KeyCode::Char('N') => {
                self.cycle_panel(-1);
                DashboardAction::None
            }
            KeyCode::Char('r') => DashboardAction::RefreshAll,
            KeyCode::Char('q') => DashboardAction::Quit,
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll_down();
                DashboardAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll_up();
                DashboardAction::None
            }
            KeyCode::Char('f') => {
                if self.focus == Panel::Jira {
                    self.jira_hide_done = !self.jira_hide_done;
                    self.jira_list_state.select(None);
                }
                DashboardAction::None
            }
            KeyCode::Char('x') => {
                if self.focus == Panel::Tasks {
                    if let Some(sel) = self.tasks_list_state.selected() {
                        if let Some(t) = self.tasks.get(sel) {
                            return DashboardAction::ToggleTask(t.id);
                        }
                    }
                } else if self.focus == Panel::Habits {
                    if let Some(sel) = self.habits_list_state.selected() {
                        if let Some(h) = self.habits.get(sel) {
                            return DashboardAction::ToggleHabit(h.habit.id);
                        }
                    }
                }
                DashboardAction::None
            }
            KeyCode::Char('s') => {
                if self.focus == Panel::Tasks {
                    if let Some(sel) = self.tasks_list_state.selected() {
                        if let Some(t) = self.tasks.get(sel) {
                            if !t.completed {
                                return DashboardAction::StartFocusTimer(t.id, t.title.clone());
                            }
                        }
                    }
                }
                DashboardAction::None
            }
            KeyCode::Enter => self.activate_focused(),
            _ => DashboardAction::None,
        }
    }

    fn cycle_panel(&mut self, direction: i32) {
        let idx = PANELS
            .iter()
            .position(|p| *p == self.focus)
            .unwrap_or(0) as i32;
        let next = ((idx + direction).rem_euclid(PANELS.len() as i32)) as usize;
        self.focus = PANELS[next];
    }

    fn scroll_down(&mut self) {
        match self.focus {
            Panel::Tasks => scroll_list_down(&mut self.tasks_list_state, self.tasks.len()),
            Panel::Habits => scroll_list_down(&mut self.habits_list_state, self.habits.len()),
            Panel::Jira => {
                let len = self.filtered_jira_issues().len();
                scroll_list_down(&mut self.jira_list_state, len);
            }
            Panel::GitLab => {
                if let GitlabState::Ready(mrs) = &self.gitlab_cache.state {
                    let len = mrs.len();
                    scroll_list_down(&mut self.gitlab_list_state, len);
                }
            }
            Panel::Notes => scroll_list_down(&mut self.notes_list_state, self.notes.len()),
            Panel::Calendar => {
                self.calendar_scroll = self.calendar_scroll.saturating_add(1);
            }
            Panel::Slack => {
                self.slack_scroll = self.slack_scroll.saturating_add(1);
            }
        }
    }

    fn scroll_up(&mut self) {
        match self.focus {
            Panel::Tasks => scroll_list_up(&mut self.tasks_list_state),
            Panel::Habits => scroll_list_up(&mut self.habits_list_state),
            Panel::Jira => scroll_list_up(&mut self.jira_list_state),
            Panel::GitLab => scroll_list_up(&mut self.gitlab_list_state),
            Panel::Notes => scroll_list_up(&mut self.notes_list_state),
            Panel::Calendar => {
                self.calendar_scroll = self.calendar_scroll.saturating_sub(1);
            }
            Panel::Slack => {
                self.slack_scroll = self.slack_scroll.saturating_sub(1);
            }
        }
    }

    fn activate_focused(&self) -> DashboardAction {
        match self.focus {
            Panel::Tasks => DashboardAction::SwitchToTasks,
            Panel::Habits => DashboardAction::SwitchToHabits,
            Panel::Jira => {
                let filtered = self.filtered_jira_issues();
                if let Some(sel) = self.jira_list_state.selected() {
                    if let Some(issue) = filtered.get(sel) {
                        return DashboardAction::OpenUrl(issue.url.clone());
                    }
                }
                DashboardAction::None
            }
            Panel::GitLab => {
                if let GitlabState::Ready(mrs) = &self.gitlab_cache.state {
                    if let Some(sel) = self.gitlab_list_state.selected() {
                        if let Some(mr) = mrs.get(sel) {
                            return DashboardAction::OpenUrl(mr.url.clone());
                        }
                    }
                }
                DashboardAction::None
            }
            Panel::Notes => DashboardAction::SwitchToNotes,
            Panel::Calendar | Panel::Slack => DashboardAction::None,
        }
    }

    fn filtered_jira_issues(&self) -> Vec<&crate::integrations::jira::JiraIssue> {
        if let JiraState::Ready(issues) = &self.jira_cache.state {
            if self.jira_hide_done {
                issues.iter().filter(|i| !is_done_status(&i.status)).collect()
            } else {
                issues.iter().collect()
            }
        } else {
            vec![]
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, conn: &Connection) {
        let today = Local::now().date_naive();

        // Lazy-load tasks and habits on each render (cheap DB queries)
        if self.tasks.is_empty() && self.habits.is_empty() {
            self.reload_tasks_and_habits(conn);
        }

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // top row: clock + weather + notes
                Constraint::Min(6),    // main content
                Constraint::Length(1),  // hint bar
            ])
            .split(area);

        // Top row: Clock | Weather | Notes (3 columns)
        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(40),
                Constraint::Percentage(35),
            ])
            .split(rows[0]);
        self.render_clock(frame, top_cols[0]);
        self.render_weather(frame, top_cols[1]);
        self.render_notes(frame, top_cols[2]);

        // Main: 2-column layout
        let mid_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        // Left: Tasks (35%), Habits (25%), Jira (40%)
        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(35),
                Constraint::Percentage(25),
                Constraint::Percentage(40),
            ])
            .split(mid_cols[0]);

        // Right: GitLab (40%), Slack (30%), Calendar (30%)
        let right_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(40),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
            ])
            .split(mid_cols[1]);

        self.render_tasks(frame, left_panels[0], conn, today);
        self.render_habits(frame, left_panels[1], conn, today);
        self.render_jira(frame, left_panels[2]);
        self.render_gitlab(frame, right_panels[0]);
        self.render_slack(frame, right_panels[1]);
        self.render_calendar(frame, right_panels[2]);

        // Hint bar
        let panel_name = match self.focus {
            Panel::Tasks => "Tasks",
            Panel::Habits => "Habits",
            Panel::Jira => "Jira",
            Panel::GitLab => "GitLab",
            Panel::Calendar => "Calendar",
            Panel::Slack => "Slack",
            Panel::Notes => "Notes",
        };
        let filter_label = if self.jira_hide_done { "f:active" } else { "f:off" };
        let focus_hint = if self.focus == Panel::Tasks { "  [s]focus" } else { "" };
        let hint = Paragraph::new(format!(
            " [n/N]panel  [j/k]scroll  [Enter]open  [r]efresh  [f]ilter ({}){} [q]uit  | {}",
            filter_label, focus_hint, panel_name
        ))
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, rows[2]);
    }

    fn render_clock(&self, frame: &mut Frame, area: Rect) {
        let now = Local::now();
        let lines = vec![
            Line::from(Span::styled(
                now.format("%H:%M:%S").to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                now.format("%A, %B %-d, %Y").to_string(),
                Style::default().fg(Color::White),
            )),
        ];
        let clock = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Clock "));
        frame.render_widget(clock, area);
    }

    fn render_weather(&self, frame: &mut Frame, area: Rect) {
        let content: Vec<Line> = match &self.weather_cache.state {
            WeatherState::Idle => vec![dim_line("Weather loading...")],
            WeatherState::Loading => vec![yellow_line("Fetching weather...")],
            WeatherState::Error(e) => vec![
                Line::from(Span::styled("Weather error", Style::default().fg(Color::Red))),
                dim_line(e),
            ],
            WeatherState::Ready(data) => vec![
                Line::from(vec![
                    Span::raw(format!("{}  ", data.condition_icon)),
                    Span::styled(
                        data.description.clone(),
                        Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    dim_span("Temp: "),
                    Span::styled(format!("{}  feels like {}", data.temp, data.feels_like), Style::default().fg(Color::Cyan)),
                ]),
                Line::from(vec![
                    dim_span("Humidity: "),
                    Span::raw(format!("{}   ", data.humidity)),
                    dim_span("Wind: "),
                    Span::raw(data.wind.clone()),
                ]),
            ],
        };
        let title = match &self.weather_cache.state {
            WeatherState::Ready(data) if !data.location.is_empty() => {
                format!(" Weather — {} ", data.location)
            }
            _ => " Weather ".to_string(),
        };
        let w = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title(title))
            .wrap(Wrap { trim: true });
        frame.render_widget(w, area);
    }

    fn render_tasks(&mut self, frame: &mut Frame, area: Rect, conn: &Connection, today: chrono::NaiveDate) {
        let focused = self.focus == Panel::Tasks;

        // Refresh tasks from DB
        self.tasks = task::list_for_date(conn, today).unwrap_or_default();
        clamp_list_state(&mut self.tasks_list_state, self.tasks.len());

        if self.tasks.is_empty() {
            let p = Paragraph::new(dim_line("No tasks for today"))
                .block(panel_block("Tasks", focused));
            frame.render_widget(p, area);
            return;
        }

        let (done, total) = (
            self.tasks.iter().filter(|t| t.completed).count(),
            self.tasks.len(),
        );

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .map(|t| {
                let check = if t.completed { "[x] " } else { "[ ] " };
                let style = if t.completed {
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        check,
                        Style::default().fg(if t.completed { Color::Green } else { Color::Gray }),
                    ),
                    Span::styled(t.title.clone(), style),
                ]))
            })
            .collect();

        let title = format!("Tasks ({}/{})", done, total);
        let list = List::new(items)
            .block(panel_block(&title, focused))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, area, &mut self.tasks_list_state);
    }

    fn render_habits(&mut self, frame: &mut Frame, area: Rect, conn: &Connection, today: chrono::NaiveDate) {
        let focused = self.focus == Panel::Habits;

        self.habits = habit::list_with_progress(conn, today).unwrap_or_default();
        clamp_list_state(&mut self.habits_list_state, self.habits.len());

        if self.habits.is_empty() {
            let p = Paragraph::new(dim_line("No habits yet"))
                .block(panel_block("Habits", focused));
            frame.render_widget(p, area);
            return;
        }

        let completed = self.habits.iter().filter(|h| h.completed).count();

        let items: Vec<ListItem> = self
            .habits
            .iter()
            .map(|h| {
                let progress_color = if h.completed {
                    Color::Green
                } else if h.checkins_this_week > 0 {
                    Color::Yellow
                } else {
                    Color::Gray
                };
                let title_style = if h.completed {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(Color::White)
                };
                let streak_str = if h.streak > 0 {
                    format!("  {}wk", h.streak)
                } else {
                    String::new()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(h.habit.title.clone(), title_style),
                    Span::styled(
                        format!("  {}/{}", h.checkins_this_week, h.habit.frequency),
                        Style::default().fg(progress_color),
                    ),
                    if h.streak > 0 {
                        Span::styled(streak_str, Style::default().fg(Color::Rgb(255, 140, 0)))
                    } else {
                        Span::raw("")
                    },
                    if h.completed {
                        Span::styled(" ✓", Style::default().fg(Color::Green))
                    } else {
                        Span::raw("")
                    },
                ]))
            })
            .collect();

        let title = format!("Habits ({}/{})", completed, self.habits.len());
        let list = List::new(items)
            .block(panel_block(&title, focused))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, area, &mut self.habits_list_state);
    }

    fn render_jira(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Panel::Jira;

        match &self.jira_cache.state {
            JiraState::Idle => {
                let p = Paragraph::new(dim_line("Jira not configured"))
                    .block(panel_block("Jira Issues", focused));
                frame.render_widget(p, area);
            }
            JiraState::Loading => {
                let p = Paragraph::new(yellow_line("Loading Jira issues..."))
                    .block(panel_block("Jira Issues", focused));
                frame.render_widget(p, area);
            }
            JiraState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Jira error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(panel_block("Jira Issues", focused))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            JiraState::Ready(_) => {
                let filtered = self.filtered_jira_issues();
                let items: Vec<ListItem> = filtered
                    .iter()
                    .map(|i| {
                        let status_color = match i.status.to_lowercase().as_str() {
                            s if s.contains("progress") => Color::Yellow,
                            s if s.contains("done") || s.contains("closed") => Color::Green,
                            s if s.contains("review") => Color::Magenta,
                            _ => Color::White,
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("{} ", i.key),
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(
                                truncate(&i.summary, 40),
                                Style::default().fg(Color::White),
                            ),
                            Span::styled(
                                format!("  [{}]", i.status),
                                Style::default().fg(status_color),
                            ),
                        ]))
                    })
                    .collect();

                let filter_indicator = if self.jira_hide_done { " [filtered]" } else { "" };
                let title = format!("Jira Issues ({}){}", filtered.len(), filter_indicator);
                let list = List::new(items)
                    .block(panel_block(&title, focused))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                frame.render_stateful_widget(list, area, &mut self.jira_list_state);
            }
        }
    }

    fn render_gitlab(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Panel::GitLab;

        match &self.gitlab_cache.state {
            GitlabState::Idle => {
                let p = Paragraph::new(dim_line("GitLab not configured"))
                    .block(panel_block("GitLab MRs", focused));
                frame.render_widget(p, area);
            }
            GitlabState::Loading => {
                let p = Paragraph::new(yellow_line("Loading merge requests..."))
                    .block(panel_block("GitLab MRs", focused));
                frame.render_widget(p, area);
            }
            GitlabState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("GitLab error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(panel_block("GitLab MRs", focused))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            GitlabState::Ready(mrs) => {
                use crate::integrations::gitlab::MrKind;
                let items: Vec<ListItem> = mrs
                    .iter()
                    .map(|mr| {
                        let kind_label = match mr.kind {
                            MrKind::Mine => "mine ".to_string(),
                            MrKind::Review => {
                                if mr.author.is_empty() {
                                    "review ".to_string()
                                } else {
                                    format!("{} ", mr.author)
                                }
                            }
                        };
                        let kind_color = match mr.kind {
                            MrKind::Mine => Color::Green,
                            MrKind::Review => Color::Yellow,
                        };
                        let kind_badge = Span::styled(kind_label, Style::default().fg(kind_color));
                        let draft_marker = if mr.draft { "WIP " } else { "" };
                        let conflict = if mr.has_conflicts {
                            Span::styled(" !", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                        } else {
                            Span::raw("")
                        };
                        ListItem::new(Line::from(vec![
                            kind_badge,
                            Span::styled(
                                format!("{}{}", draft_marker, truncate(&mr.title, 35)),
                                Style::default().fg(if mr.draft { Color::Gray } else { Color::White }),
                            ),
                            conflict,
                            Span::styled(
                                format!("  {}", mr.source_branch),
                                Style::default().fg(Color::Cyan),
                            ),
                        ]))
                    })
                    .collect();

                let title = format!("GitLab MRs ({})", mrs.len());
                let list = List::new(items)
                    .block(panel_block(&title, focused))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                frame.render_stateful_widget(list, area, &mut self.gitlab_list_state);
            }
        }
    }

    fn render_calendar(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Panel::Calendar;

        match &self.calendar_cache.state {
            CalendarState::Idle => {
                let p = Paragraph::new(dim_line("Calendar not configured"))
                    .block(panel_block("Upcoming", focused));
                frame.render_widget(p, area);
            }
            CalendarState::Loading => {
                let p = Paragraph::new(yellow_line("Loading events..."))
                    .block(panel_block("Upcoming", focused));
                frame.render_widget(p, area);
            }
            CalendarState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Calendar error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(panel_block("Upcoming", focused))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            CalendarState::Ready(events) => {
                let lines: Vec<Line> = if events.is_empty() {
                    vec![dim_line("No upcoming events")]
                } else {
                    events
                        .iter()
                        .map(|ev| {
                            let time_part = if ev.time.is_empty() {
                                Span::styled("all day ", Style::default().fg(Color::DarkGray))
                            } else {
                                Span::styled(format!("{} ", ev.time), Style::default().fg(Color::Yellow))
                            };
                            let cal = if ev.calendar.is_empty() {
                                Span::raw("")
                            } else {
                                Span::styled(
                                    format!("  ({})", ev.calendar),
                                    Style::default().fg(Color::DarkGray),
                                )
                            };
                            Line::from(vec![
                                time_part,
                                Span::styled(ev.title.clone(), Style::default().fg(Color::White)),
                                cal,
                            ])
                        })
                        .collect()
                };
                let title = format!("Upcoming ({})", events.len());
                let p = Paragraph::new(lines)
                    .block(panel_block(&title, focused))
                    .wrap(Wrap { trim: true })
                    .scroll((self.calendar_scroll, 0));
                frame.render_widget(p, area);
            }
        }
    }

    fn render_slack(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Panel::Slack;

        match &self.slack_cache.state {
            SlackState::Idle => {
                let p = Paragraph::new(dim_line("Slack not configured"))
                    .block(panel_block("Slack DMs", focused));
                frame.render_widget(p, area);
            }
            SlackState::Loading => {
                let p = Paragraph::new(yellow_line("Loading Slack messages..."))
                    .block(panel_block("Slack DMs", focused));
                frame.render_widget(p, area);
            }
            SlackState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Slack error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(panel_block("Slack DMs", focused))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            SlackState::Ready(messages) => {
                let lines: Vec<Line> = if messages.is_empty() {
                    vec![dim_line("No recent messages")]
                } else {
                    messages
                        .iter()
                        .map(|m| {
                            let ago = format_slack_age(&m.timestamp);
                            Line::from(vec![
                                Span::styled(
                                    format!("{}: ", m.from_user),
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    truncate(&m.text, 50),
                                    Style::default().fg(Color::White),
                                ),
                                Span::styled(
                                    format!("  {}", ago),
                                    Style::default().fg(Color::DarkGray),
                                ),
                            ])
                        })
                        .collect()
                };
                let title = format!("Slack DMs ({})", messages.len());
                let p = Paragraph::new(lines)
                    .block(panel_block(&title, focused))
                    .wrap(Wrap { trim: true })
                    .scroll((self.slack_scroll, 0));
                frame.render_widget(p, area);
            }
        }
    }

    fn render_notes(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Panel::Notes;

        if self.notes.is_empty() {
            let p = Paragraph::new(dim_line("No notes yet"))
                .block(panel_block("Notes", focused));
            frame.render_widget(p, area);
            return;
        }

        let items: Vec<ListItem> = self
            .notes
            .iter()
            .map(|n| {
                let age = format_note_age(n.modified);
                ListItem::new(Line::from(vec![
                    Span::styled(
                        n.title.clone(),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("  {}", age),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        let title = format!("Notes ({})", self.notes.len());
        let list = List::new(items)
            .block(panel_block(&title, focused))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, area, &mut self.notes_list_state);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn scroll_list_down(state: &mut ListState, len: usize) {
    if len > 0 {
        let next = match state.selected() {
            Some(i) => (i + 1).min(len - 1),
            None => 0,
        };
        state.select(Some(next));
    }
}

fn scroll_list_up(state: &mut ListState) {
    let prev = match state.selected() {
        Some(0) | None => 0,
        Some(i) => i - 1,
    };
    state.select(Some(prev));
}

fn clamp_list_state(state: &mut ListState, len: usize) {
    if let Some(sel) = state.selected() {
        if len == 0 {
            state.select(None);
        } else if sel >= len {
            state.select(Some(len - 1));
        }
    } else if len > 0 {
        state.select(Some(0));
    }
}

fn panel_block(title: &str, focused: bool) -> Block<'static> {
    if focused {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Double)
            .border_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .title(format!(" ▸ {} ", title))
    } else {
        Block::default()
            .borders(Borders::ALL)
            .title(format!(" {} ", title))
    }
}

fn dim_line(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        s.to_string(),
        Style::default().fg(Color::DarkGray),
    ))
}

fn dim_span(s: &str) -> Span<'static> {
    Span::styled(s.to_string(), Style::default().fg(Color::DarkGray))
}

fn yellow_line(s: &str) -> Line<'static> {
    Line::from(Span::styled(
        s.to_string(),
        Style::default().fg(Color::Yellow),
    ))
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

fn format_slack_age(ts: &str) -> String {
    let epoch: f64 = ts.parse().unwrap_or(0.0);
    if epoch == 0.0 {
        return String::new();
    }
    let now = chrono::Local::now().timestamp() as f64;
    let secs = (now - epoch).max(0.0) as u64;
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        86400..=604799 => format!("{}d ago", secs / 86400),
        _ => format!("{}w ago", secs / 604800),
    }
}

fn format_note_age(modified: std::time::SystemTime) -> String {
    let elapsed = modified.elapsed().unwrap_or_default();
    let secs = elapsed.as_secs();
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        86400..=604799 => format!("{}d ago", secs / 86400),
        _ => format!("{}w ago", secs / 604800),
    }
}

fn is_done_status(status: &str) -> bool {
    let s = status.to_lowercase();
    s.contains("done") || s.contains("closed") || s.contains("resolved") || s.contains("complete")
}

pub enum DashboardAction {
    None,
    RefreshAll,
    OpenUrl(String),
    SwitchToNotes,
    SwitchToTasks,
    SwitchToHabits,
    ToggleTask(i64),
    ToggleHabit(i64),
    StartFocusTimer(i64, String),
    Quit,
}
