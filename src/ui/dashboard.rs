use chrono::Local;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::integrations::weather::{WeatherCache, WeatherState};
use crate::models::{task, weight, workout};

pub struct DashboardTab {
    pub weather_cache: WeatherCache,
}

impl DashboardTab {
    pub fn new() -> Self {
        Self {
            weather_cache: WeatherCache::new(),
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        false
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DashboardAction {
        match key.code {
            KeyCode::Char('r') => DashboardAction::RefreshWeather,
            KeyCode::Char('q') => DashboardAction::Quit,
            _ => DashboardAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, conn: &Connection) {
        let today = Local::now().date_naive();

        // Top row: clock | weather
        // Bottom row: today's summary
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // top widgets
                Constraint::Min(5),     // summary
                Constraint::Length(1),  // hint
            ])
            .split(area);

        let top_cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(rows[0]);

        self.render_clock(frame, top_cols[0]);
        self.render_weather(frame, top_cols[1]);
        self.render_summary(frame, rows[1], conn, today);

        let hint = Paragraph::new(" [r]efresh weather  [q]quit")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, rows[2]);
    }

    fn render_clock(&self, frame: &mut Frame, area: Rect) {
        let now = Local::now();
        let time_str = now.format("%H:%M:%S").to_string();
        let date_str = now.format("%A, %B %-d, %Y").to_string();

        let lines = vec![
            Line::from(Span::styled(
                time_str,
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                date_str,
                Style::default().fg(Color::White),
            )),
        ];

        let clock = Paragraph::new(lines)
            .block(Block::default().borders(Borders::ALL).title(" Clock "));
        frame.render_widget(clock, area);
    }

    fn render_weather(&self, frame: &mut Frame, area: Rect) {
        let content: Vec<Line> = match &self.weather_cache.state {
            WeatherState::Idle => vec![Line::from(Span::styled(
                "Weather not configured — set weather.location in config.toml",
                Style::default().fg(Color::DarkGray),
            ))],
            WeatherState::Loading => vec![Line::from(Span::styled(
                "Fetching weather…",
                Style::default().fg(Color::Yellow),
            ))],
            WeatherState::Error(e) => vec![
                Line::from(Span::styled(
                    "Failed to fetch weather",
                    Style::default().fg(Color::Red),
                )),
                Line::from(Span::styled(
                    e.clone(),
                    Style::default().fg(Color::DarkGray),
                )),
            ],
            WeatherState::Ready(data) => vec![
                Line::from(vec![
                    Span::styled(
                        format!("{}  ", data.condition_icon),
                        Style::default(),
                    ),
                    Span::styled(
                        data.description.clone(),
                        Style::default()
                            .fg(Color::White)
                            .add_modifier(Modifier::BOLD),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Temp: ", Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{}  feels like {}", data.temp, data.feels_like),
                        Style::default().fg(Color::Cyan),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Humidity: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(format!("{}   ", data.humidity)),
                    Span::styled("Wind: ", Style::default().fg(Color::DarkGray)),
                    Span::raw(data.wind.clone()),
                ]),
            ],
        };

        let weather = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title(" Weather "))
            .wrap(Wrap { trim: true });
        frame.render_widget(weather, area);
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

        // Tasks summary
        let (tasks_done, tasks_total) = task::count_for_date(conn, today).unwrap_or((0, 0));
        let pct = if tasks_total > 0 {
            (tasks_done * 100) / tasks_total
        } else {
            0
        };
        let task_color = match pct {
            100 => Color::Green,
            50..=99 => Color::Yellow,
            _ => Color::White,
        };
        let tasks_widget = Paragraph::new(vec![
            Line::from(Span::styled(
                format!("{}/{}", tasks_done, tasks_total),
                Style::default()
                    .fg(task_color)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                format!("{}% complete", pct),
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(Block::default().borders(Borders::ALL).title(" Today's Tasks "));
        frame.render_widget(tasks_widget, cols[0]);

        // Workout summary
        let workouts = workout::list_for_date(conn, today).unwrap_or_default();
        let workout_lines: Vec<Line> = if workouts.is_empty() {
            vec![Line::from(Span::styled(
                "No workout logged",
                Style::default().fg(Color::DarkGray),
            ))]
        } else {
            workouts
                .iter()
                .map(|w| {
                    let dur = w
                        .duration_minutes
                        .map(|d| format!(" {}min", d))
                        .unwrap_or_default();
                    Line::from(Span::styled(
                        format!("{}{}",  w.workout_type, dur),
                        Style::default().fg(Color::Cyan),
                    ))
                })
                .collect()
        };
        let workout_widget = Paragraph::new(workout_lines)
            .block(Block::default().borders(Borders::ALL).title(" Workout "));
        frame.render_widget(workout_widget, cols[1]);

        // Weight summary
        let weight_entry = weight::get_for_date(conn, today).unwrap_or(None);
        let weight_lines: Vec<Line> = match weight_entry {
            Some(e) => vec![
                Line::from(Span::styled(
                    format!("{} lbs", e.weight),
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
            ],
            None => vec![Line::from(Span::styled(
                "Not logged today",
                Style::default().fg(Color::DarkGray),
            ))],
        };
        let weight_widget = Paragraph::new(weight_lines)
            .block(Block::default().borders(Borders::ALL).title(" Weight "));
        frame.render_widget(weight_widget, cols[2]);
    }
}

pub enum DashboardAction {
    None,
    RefreshWeather,
    Quit,
}
