use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::integrations::calendar::{CalendarCache, CalendarState};
use crate::integrations::gitlab::{GitlabCache, GitlabState};
use crate::integrations::jira::{JiraCache, JiraState};
use crate::integrations::slack::{SlackCache, SlackState};
use crate::integrations::weather::{WeatherCache, WeatherState};
use crate::models::{task, weight, workout};

/// Which scrollable panel is focused for Enter-to-open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Jira,
    Gitlab,
}

pub struct DashboardTab {
    pub weather_cache: WeatherCache,
    pub jira_cache: JiraCache,
    pub gitlab_cache: GitlabCache,
    pub slack_cache: SlackCache,
    pub calendar_cache: CalendarCache,
    focus: Focus,
    jira_list_state: ListState,
    gitlab_list_state: ListState,
}

impl DashboardTab {
    pub fn new() -> Self {
        Self {
            weather_cache: WeatherCache::new(),
            jira_cache: JiraCache::new(),
            gitlab_cache: GitlabCache::new(),
            slack_cache: SlackCache::new(),
            calendar_cache: CalendarCache::new(),
            focus: Focus::Jira,
            jira_list_state: ListState::default(),
            gitlab_list_state: ListState::default(),
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        false
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Char('r') => DashboardAction::RefreshAll,
            KeyCode::Char('q') => DashboardAction::Quit,
            // Switch focus between Jira and GitLab panels
            KeyCode::Char('h') | KeyCode::Left => {
                self.focus = Focus::Jira;
                DashboardAction::None
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.focus = Focus::Gitlab;
                DashboardAction::None
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_focus_down();
                DashboardAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_focus_up();
                DashboardAction::None
            }
            // Enter opens the selected issue/MR in the browser
            KeyCode::Enter => self.open_selected(),
            _ => DashboardAction::None,
        }
    }

    fn move_focus_down(&mut self) {
        match self.focus {
            Focus::Jira => {
                if let JiraState::Ready(issues) = &self.jira_cache.state {
                    let len = issues.len();
                    if len > 0 {
                        let next = match self.jira_list_state.selected() {
                            Some(i) => (i + 1).min(len - 1),
                            None => 0,
                        };
                        self.jira_list_state.select(Some(next));
                    }
                }
            }
            Focus::Gitlab => {
                if let GitlabState::Ready(mrs) = &self.gitlab_cache.state {
                    let len = mrs.len();
                    if len > 0 {
                        let next = match self.gitlab_list_state.selected() {
                            Some(i) => (i + 1).min(len - 1),
                            None => 0,
                        };
                        self.gitlab_list_state.select(Some(next));
                    }
                }
            }
        }
    }

    fn move_focus_up(&mut self) {
        match self.focus {
            Focus::Jira => {
                if let JiraState::Ready(_) = &self.jira_cache.state {
                    let prev = match self.jira_list_state.selected() {
                        Some(0) | None => 0,
                        Some(i) => i - 1,
                    };
                    self.jira_list_state.select(Some(prev));
                }
            }
            Focus::Gitlab => {
                if let GitlabState::Ready(_) = &self.gitlab_cache.state {
                    let prev = match self.gitlab_list_state.selected() {
                        Some(0) | None => 0,
                        Some(i) => i - 1,
                    };
                    self.gitlab_list_state.select(Some(prev));
                }
            }
        }
    }

    fn open_selected(&self) -> DashboardAction {
        match self.focus {
            Focus::Jira => {
                if let JiraState::Ready(issues) = &self.jira_cache.state {
                    if let Some(sel) = self.jira_list_state.selected() {
                        if let Some(issue) = issues.get(sel) {
                            return DashboardAction::OpenUrl(issue.url.clone());
                        }
                    }
                }
            }
            Focus::Gitlab => {
                if let GitlabState::Ready(mrs) = &self.gitlab_cache.state {
                    if let Some(sel) = self.gitlab_list_state.selected() {
                        if let Some(mr) = mrs.get(sel) {
                            return DashboardAction::OpenUrl(mr.url.clone());
                        }
                    }
                }
            }
        }
        DashboardAction::None
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, conn: &Connection) {
        let today = Local::now().date_naive();

        // Layout: top row (clock + weather), middle (integrations), bottom (summary), hint
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // clock + weather
                Constraint::Min(6),    // integrations (scrollable lists)
                Constraint::Length(5), // today summary
                Constraint::Length(1), // hint bar
            ])
            .split(area);

        // Top row: clock | weather
        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(rows[0]);
        self.render_clock(frame, top_cols[0]);
        self.render_weather(frame, top_cols[1]);

        // Middle: integrations in a 2-column layout
        // Left column: Jira + Calendar    Right column: GitLab + Slack
        let mid_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(rows[1]);

        let left_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(mid_cols[0]);

        let right_panels = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(mid_cols[1]);

        self.render_jira(frame, left_panels[0]);
        self.render_calendar(frame, left_panels[1]);
        self.render_gitlab(frame, right_panels[0]);
        self.render_slack(frame, right_panels[1]);

        // Bottom: today summary
        self.render_summary(frame, rows[2], conn, today);

        // Hint bar
        let hint = Paragraph::new(
            " [r]efresh all  [j/k] scroll  [h/l] focus Jira/GitLab  [Enter] open in browser  [q]uit",
        )
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, rows[3]);
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
            WeatherState::Idle => vec![dim_line("Set weather.location in config.toml")],
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
        let w = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title(" Weather "))
            .wrap(Wrap { trim: true });
        frame.render_widget(w, area);
    }

    fn render_jira(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Jira;
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        match &self.jira_cache.state {
            JiraState::Idle => {
                let p = Paragraph::new(dim_line("Jira not configured"))
                    .block(Block::default().borders(Borders::ALL).title(" Jira Issues ").border_style(border_style));
                frame.render_widget(p, area);
            }
            JiraState::Loading => {
                let p = Paragraph::new(yellow_line("Loading Jira issues..."))
                    .block(Block::default().borders(Borders::ALL).title(" Jira Issues ").border_style(border_style));
                frame.render_widget(p, area);
            }
            JiraState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Jira error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(Block::default().borders(Borders::ALL).title(" Jira Issues ").border_style(border_style))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            JiraState::Ready(issues) => {
                let items: Vec<ListItem> = issues
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

                let title = format!(" Jira Issues ({}) ", issues.len());
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(title).border_style(border_style))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                frame.render_stateful_widget(list, area, &mut self.jira_list_state);
            }
        }
    }

    fn render_gitlab(&mut self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Gitlab;
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default()
        };

        match &self.gitlab_cache.state {
            GitlabState::Idle => {
                let p = Paragraph::new(dim_line("GitLab not configured"))
                    .block(Block::default().borders(Borders::ALL).title(" GitLab MRs ").border_style(border_style));
                frame.render_widget(p, area);
            }
            GitlabState::Loading => {
                let p = Paragraph::new(yellow_line("Loading merge requests..."))
                    .block(Block::default().borders(Borders::ALL).title(" GitLab MRs ").border_style(border_style));
                frame.render_widget(p, area);
            }
            GitlabState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("GitLab error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(Block::default().borders(Borders::ALL).title(" GitLab MRs ").border_style(border_style))
                .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
            GitlabState::Ready(mrs) => {
                let items: Vec<ListItem> = mrs
                    .iter()
                    .map(|mr| {
                        let draft_marker = if mr.draft { "WIP " } else { "" };
                        let conflict = if mr.has_conflicts {
                            Span::styled(" !", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
                        } else {
                            Span::raw("")
                        };
                        ListItem::new(Line::from(vec![
                            Span::styled(
                                format!("{}{}", draft_marker, truncate(&mr.title, 40)),
                                Style::default().fg(if mr.draft { Color::DarkGray } else { Color::White }),
                            ),
                            conflict,
                            Span::styled(
                                format!("  {}", mr.source_branch),
                                Style::default().fg(Color::Cyan),
                            ),
                        ]))
                    })
                    .collect();

                let title = format!(" GitLab MRs ({}) ", mrs.len());
                let list = List::new(items)
                    .block(Block::default().borders(Borders::ALL).title(title).border_style(border_style))
                    .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                frame.render_stateful_widget(list, area, &mut self.gitlab_list_state);
            }
        }
    }

    fn render_calendar(&self, frame: &mut Frame, area: Rect) {
        match &self.calendar_cache.state {
            CalendarState::Idle => {
                let p = Paragraph::new(dim_line("Calendar not configured"))
                    .block(Block::default().borders(Borders::ALL).title(" Upcoming "));
                frame.render_widget(p, area);
            }
            CalendarState::Loading => {
                let p = Paragraph::new(yellow_line("Loading events..."))
                    .block(Block::default().borders(Borders::ALL).title(" Upcoming "));
                frame.render_widget(p, area);
            }
            CalendarState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Calendar error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(Block::default().borders(Borders::ALL).title(" Upcoming "))
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
                let p = Paragraph::new(lines)
                    .block(Block::default().borders(Borders::ALL).title(format!(" Upcoming ({}) ", events.len())))
                    .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
        }
    }

    fn render_slack(&self, frame: &mut Frame, area: Rect) {
        match &self.slack_cache.state {
            SlackState::Idle => {
                let p = Paragraph::new(dim_line("Slack not configured"))
                    .block(Block::default().borders(Borders::ALL).title(" Slack DMs "));
                frame.render_widget(p, area);
            }
            SlackState::Loading => {
                let p = Paragraph::new(yellow_line("Loading Slack messages..."))
                    .block(Block::default().borders(Borders::ALL).title(" Slack DMs "));
                frame.render_widget(p, area);
            }
            SlackState::Error(e) => {
                let p = Paragraph::new(vec![
                    Line::from(Span::styled("Slack error", Style::default().fg(Color::Red))),
                    dim_line(e),
                ])
                .block(Block::default().borders(Borders::ALL).title(" Slack DMs "))
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
                            Line::from(vec![
                                Span::styled(
                                    format!("{}: ", m.from_user),
                                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    truncate(&m.text, 60),
                                    Style::default().fg(Color::White),
                                ),
                            ])
                        })
                        .collect()
                };
                let p = Paragraph::new(lines)
                    .block(Block::default().borders(Borders::ALL).title(format!(" Slack DMs ({}) ", messages.len())))
                    .wrap(Wrap { trim: true });
                frame.render_widget(p, area);
            }
        }
    }

    fn render_summary(&self, frame: &mut Frame, area: Rect, conn: &Connection, today: chrono::NaiveDate) {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(34),
                Constraint::Percentage(33),
                Constraint::Percentage(33),
            ])
            .split(area);

        // Tasks
        let (tasks_done, tasks_total) = task::count_for_date(conn, today).unwrap_or((0, 0));
        let pct = if tasks_total > 0 { (tasks_done * 100) / tasks_total } else { 0 };
        let task_color = match pct {
            100 => Color::Green,
            50..=99 => Color::Yellow,
            _ => Color::White,
        };
        let tasks_w = Paragraph::new(vec![
            Line::from(Span::styled(
                format!("{}/{}", tasks_done, tasks_total),
                Style::default().fg(task_color).add_modifier(Modifier::BOLD),
            )),
            Line::from(dim_span(&format!("{}% complete", pct))),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Tasks "));
        frame.render_widget(tasks_w, cols[0]);

        // Workout
        let workouts = workout::list_for_date(conn, today).unwrap_or_default();
        let workout_lines: Vec<Line> = if workouts.is_empty() {
            vec![dim_line("No workout")]
        } else {
            workouts.iter().map(|w| {
                let dur = w.duration_minutes.map(|d| format!(" {}min", d)).unwrap_or_default();
                Line::from(Span::styled(format!("{}{}", w.workout_type, dur), Style::default().fg(Color::Cyan)))
            }).collect()
        };
        let workout_w = Paragraph::new(workout_lines)
            .block(Block::default().borders(Borders::ALL).title(" Workout "));
        frame.render_widget(workout_w, cols[1]);

        // Weight
        let weight_entry = weight::get_for_date(conn, today).unwrap_or(None);
        let weight_lines: Vec<Line> = match weight_entry {
            Some(e) => vec![Line::from(Span::styled(
                format!("{} lbs", e.weight),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            ))],
            None => vec![dim_line("Not logged")],
        };
        let weight_w = Paragraph::new(weight_lines)
            .block(Block::default().borders(Borders::ALL).title(" Weight "));
        frame.render_widget(weight_w, cols[2]);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

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

pub enum DashboardAction {
    None,
    RefreshAll,
    OpenUrl(String),
    Quit,
}
