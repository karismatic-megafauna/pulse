use chrono::{Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use rusqlite::Connection;

use crate::models::{habit, task};
use crate::ui::input::{InputAction, InputWidget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focus {
    Tasks,
    Habits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    AddingTask,
    AddingHabit,
}

pub struct DailyStartScreen {
    pub dismissed: bool,
    tasks: Vec<task::Task>,
    habits: Vec<habit::HabitWithProgress>,
    date: NaiveDate,
    focus: Focus,
    mode: Mode,
    input: InputWidget,
    pub quote_text: String,
    pub quote_author: String,
    selected_task: usize,
    selected_habit: usize,
}

impl DailyStartScreen {
    pub fn new(conn: &Connection, date: NaiveDate) -> Self {
        // Roll over incomplete tasks from yesterday
        let yesterday = date - chrono::Duration::days(1);
        let _ = task::rollover_incomplete(conn, yesterday, date);

        let tasks = task::list_for_date(conn, date).unwrap_or_default();
        let habits = habit::list_with_progress(conn, date).unwrap_or_default();
        Self {
            dismissed: false,
            tasks,
            habits,
            date,
            focus: Focus::Tasks,
            mode: Mode::Normal,
            input: InputWidget::new(),
            quote_text: String::new(),
            quote_author: String::new(),
            selected_task: 0,
            selected_habit: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) {
        match self.mode {
            Mode::Normal => self.handle_normal(key, conn),
            Mode::AddingTask => self.handle_adding_task(key, conn),
            Mode::AddingHabit => self.handle_adding_habit(key, conn),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent, conn: &Connection) {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') => {
                self.dismissed = true;
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Tasks => Focus::Habits,
                    Focus::Habits => Focus::Tasks,
                };
            }
            KeyCode::Char('j') | KeyCode::Down => match self.focus {
                Focus::Tasks => {
                    if !self.tasks.is_empty() {
                        self.selected_task = (self.selected_task + 1).min(self.tasks.len() - 1);
                    }
                }
                Focus::Habits => {
                    if !self.habits.is_empty() {
                        self.selected_habit =
                            (self.selected_habit + 1).min(self.habits.len() - 1);
                    }
                }
            },
            KeyCode::Char('k') | KeyCode::Up => match self.focus {
                Focus::Tasks => {
                    self.selected_task = self.selected_task.saturating_sub(1);
                }
                Focus::Habits => {
                    self.selected_habit = self.selected_habit.saturating_sub(1);
                }
            },
            KeyCode::Char('a') => match self.focus {
                Focus::Tasks => {
                    self.mode = Mode::AddingTask;
                    self.input.clear();
                    self.input.set_active(true);
                }
                Focus::Habits => {
                    self.mode = Mode::AddingHabit;
                    self.input.clear();
                    self.input.set_active(true);
                }
            },
            KeyCode::Char('x') | KeyCode::Char(' ') => match self.focus {
                Focus::Tasks => {
                    if let Some(t) = self.tasks.get(self.selected_task) {
                        let _ = task::toggle_complete(conn, t.id);
                        self.tasks =
                            task::list_for_date(conn, self.date).unwrap_or_default();
                    }
                }
                Focus::Habits => {
                    if let Some(h) = self.habits.get(self.selected_habit) {
                        let _ = habit::toggle_checkin(conn, h.habit.id, self.date);
                        self.habits =
                            habit::list_with_progress(conn, self.date).unwrap_or_default();
                    }
                }
            },
            _ => {}
        }
    }

    fn handle_adding_task(&mut self, key: KeyEvent, conn: &Connection) {
        match self.input.handle_key(key) {
            InputAction::Submit(title) => {
                let _ = task::insert(conn, &title, self.date);
                self.tasks = task::list_for_date(conn, self.date).unwrap_or_default();
                self.mode = Mode::Normal;
                self.input.set_active(false);
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
            }
            InputAction::None => {}
        }
    }

    fn handle_adding_habit(&mut self, key: KeyEvent, conn: &Connection) {
        match self.input.handle_key(key) {
            InputAction::Submit(raw) => {
                let parts: Vec<&str> = raw.splitn(2, '|').map(|s| s.trim()).collect();
                let title = parts.first().copied().unwrap_or("");
                let freq: u8 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                if !title.is_empty() {
                    let _ = habit::insert(conn, title, freq);
                    self.habits = habit::list_with_progress(conn, self.date).unwrap_or_default();
                }
                self.mode = Mode::Normal;
                self.input.set_active(false);
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
            }
            InputAction::None => {}
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),  // top padding
                Constraint::Length(5),  // quote
                Constraint::Length(1),  // spacer
                Constraint::Min(8),    // tasks + habits side by side
                Constraint::Length(3), // input (when active) or hint
                Constraint::Length(1),  // bottom padding
            ])
            .split(area);

        self.render_quote(frame, chunks[1]);

        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(chunks[3]);

        self.render_tasks(frame, cols[0]);
        self.render_habits(frame, cols[1]);

        match self.mode {
            Mode::AddingTask => {
                self.input.render(frame, chunks[4], " New task for today ");
            }
            Mode::AddingHabit => {
                self.input
                    .render(frame, chunks[4], " New habit (title | frequency, e.g. Workout | 3) ");
            }
            Mode::Normal => {
                let hint = Paragraph::new(Line::from(vec![
                    Span::styled(" [j/k]", Style::default().fg(Color::Cyan)),
                    Span::styled("navigate  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[x/Space]", Style::default().fg(Color::Cyan)),
                    Span::styled("toggle  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[a]", Style::default().fg(Color::Cyan)),
                    Span::styled("dd  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[Tab]", Style::default().fg(Color::Cyan)),
                    Span::styled("switch  ", Style::default().fg(Color::DarkGray)),
                    Span::styled("[Enter]", Style::default().fg(Color::Cyan)),
                    Span::styled("start your day", Style::default().fg(Color::DarkGray)),
                ]))
                .alignment(Alignment::Center);
                frame.render_widget(hint, chunks[4]);
            }
        }
    }

    fn render_quote(&self, frame: &mut Frame, area: Rect) {
        let lines = if self.quote_text.is_empty() {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "Good morning.",
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::ITALIC),
                )),
            ]
        } else {
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    format!("\"{}\"", self.quote_text),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::ITALIC),
                )),
                Line::from(Span::styled(
                    format!("  — {}", self.quote_author),
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        };

        let greeting = Local::now().format("%A, %B %-d").to_string();
        let block = Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(
                format!(" {} ", greeting),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ))
            .border_style(Style::default().fg(Color::DarkGray));

        let p = Paragraph::new(lines)
            .block(block)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true });
        frame.render_widget(p, area);
    }

    fn render_tasks(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Tasks;
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        if self.tasks.is_empty() {
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No tasks yet for today",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Press 'a' to add one",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let p = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Span::styled(" Today's Tasks ", Style::default().fg(Color::White)))
                        .border_style(border_style),
                )
                .alignment(Alignment::Center);
            frame.render_widget(p, area);
            return;
        }

        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let selected = focused && i == self.selected_task;
                let check = if t.completed { "[x]" } else { "[ ]" };
                let style = if t.completed {
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(Color::White)
                };
                let prefix = if selected { "▸ " } else { "  " };
                let mut item = ListItem::new(Line::from(vec![
                    Span::styled(
                        prefix.to_string(),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{} ", check),
                        Style::default().fg(if t.completed { Color::Green } else { Color::Gray }),
                    ),
                    Span::styled(t.title.clone(), style),
                ]));
                if selected {
                    item = item.style(Style::default().bg(Color::Rgb(40, 40, 50)));
                }
                item
            })
            .collect();

        let (done, total) = (
            self.tasks.iter().filter(|t| t.completed).count(),
            self.tasks.len(),
        );
        let title = format!(" Today's Tasks ({}/{}) ", done, total);
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, Style::default().fg(Color::White)))
                .border_style(border_style),
        );
        frame.render_widget(list, area);
    }

    fn render_habits(&self, frame: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::Habits;
        let border_style = if focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        if self.habits.is_empty() {
            let lines = vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No habits yet",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Press 'a' to add one",
                    Style::default().fg(Color::DarkGray),
                )),
            ];
            let p = Paragraph::new(lines)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(Span::styled(" Habits ", Style::default().fg(Color::White)))
                        .border_style(border_style),
                )
                .alignment(Alignment::Center);
            frame.render_widget(p, area);
            return;
        }

        let items: Vec<ListItem> = self
            .habits
            .iter()
            .enumerate()
            .map(|(i, h)| {
                let selected = focused && i == self.selected_habit;
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
                let prefix = if selected { "▸ " } else { "  " };
                let mut item = ListItem::new(Line::from(vec![
                    Span::styled(
                        prefix.to_string(),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::styled(
                        format!("{}/{} ", h.checkins_this_week, h.habit.frequency),
                        Style::default().fg(progress_color),
                    ),
                    Span::styled(h.habit.title.clone(), title_style),
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
                ]));
                if selected {
                    item = item.style(Style::default().bg(Color::Rgb(40, 40, 50)));
                }
                item
            })
            .collect();

        let completed = self.habits.iter().filter(|h| h.completed).count();
        let title = format!(" Habits ({}/{}) ", completed, self.habits.len());
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(title, Style::default().fg(Color::White)))
                .border_style(border_style),
        );
        frame.render_widget(list, area);
    }
}

// ── DB helpers for tracking last opened date ────────────────────────────────

pub fn get_last_opened_date(conn: &Connection) -> Option<NaiveDate> {
    conn.query_row(
        "SELECT value FROM app_metadata WHERE key = 'last_opened_date'",
        [],
        |row| row.get::<_, String>(0),
    )
    .ok()
    .and_then(|s| NaiveDate::parse_from_str(&s, "%Y-%m-%d").ok())
}

pub fn set_last_opened_date(conn: &Connection, date: NaiveDate) {
    let _ = conn.execute(
        "INSERT OR REPLACE INTO app_metadata (key, value) VALUES ('last_opened_date', ?1)",
        [date.format("%Y-%m-%d").to_string()],
    );
}
