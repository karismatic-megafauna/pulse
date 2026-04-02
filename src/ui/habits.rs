use chrono::{Local, NaiveDate};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use rusqlite::Connection;

use crate::models::habit::{self, HabitWithProgress};
use crate::ui::input::{InputAction, InputWidget};

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Adding,
    ConfirmDelete,
}

pub struct HabitsTab {
    habits: Vec<HabitWithProgress>,
    list_state: ListState,
    mode: Mode,
    input: InputWidget,
    today: NaiveDate,
}

impl HabitsTab {
    pub fn new(conn: &Connection) -> Self {
        let today = Local::now().date_naive();
        let habits = habit::list_all_with_progress(conn, today).unwrap_or_default();
        let mut list_state = ListState::default();
        if !habits.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            habits,
            list_state,
            mode: Mode::Normal,
            input: InputWidget::new(),
            today,
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        self.mode != Mode::Normal
    }

    pub fn reload(&mut self, conn: &Connection) {
        self.today = Local::now().date_naive();
        self.habits = habit::list_all_with_progress(conn, self.today).unwrap_or_default();
        if let Some(sel) = self.list_state.selected() {
            if self.habits.is_empty() {
                self.list_state.select(None);
            } else if sel >= self.habits.len() {
                self.list_state.select(Some(self.habits.len() - 1));
            }
        } else if !self.habits.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> HabitAction {
        match self.mode {
            Mode::Normal => self.handle_normal(key, conn),
            Mode::Adding => self.handle_adding(key, conn),
            Mode::ConfirmDelete => self.handle_confirm_delete(key, conn),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent, conn: &Connection) -> HabitAction {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down();
                HabitAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up();
                HabitAction::None
            }
            KeyCode::Char('x') | KeyCode::Char(' ') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(h) = self.habits.get(sel) {
                        if h.habit.active {
                            let _ = habit::toggle_checkin(conn, h.habit.id, self.today);
                            self.reload(conn);
                        }
                    }
                }
                HabitAction::None
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Adding;
                self.input.clear();
                self.input.set_active(true);
                HabitAction::None
            }
            KeyCode::Char('d') => {
                if self.list_state.selected().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
                HabitAction::None
            }
            KeyCode::Char('p') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(h) = self.habits.get(sel) {
                        let _ = habit::toggle_active(conn, h.habit.id);
                        self.reload(conn);
                    }
                }
                HabitAction::None
            }
            KeyCode::Char('q') => HabitAction::Quit,
            _ => HabitAction::None,
        }
    }

    fn handle_adding(&mut self, key: KeyEvent, conn: &Connection) -> HabitAction {
        match self.input.handle_key(key) {
            InputAction::Submit(raw) => {
                let parts: Vec<&str> = raw.splitn(2, '|').map(|s| s.trim()).collect();
                let title = parts.first().copied().unwrap_or("");
                let freq: u8 = parts
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(1);
                if !title.is_empty() {
                    let _ = habit::insert(conn, title, freq);
                    self.reload(conn);
                }
                self.mode = Mode::Normal;
                self.input.set_active(false);
                HabitAction::None
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
                HabitAction::None
            }
            InputAction::None => HabitAction::None,
        }
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent, conn: &Connection) -> HabitAction {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(h) = self.habits.get(sel) {
                        let _ = habit::delete(conn, h.habit.id);
                        self.reload(conn);
                    }
                }
                self.mode = Mode::Normal;
                HabitAction::None
            }
            _ => {
                self.mode = Mode::Normal;
                HabitAction::None
            }
        }
    }

    fn move_down(&mut self) {
        if self.habits.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.habits.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    fn move_up(&mut self) {
        if self.habits.is_empty() {
            return;
        }
        let prev = match self.list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(prev));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),
                if self.mode == Mode::Adding {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Length(1),
            ])
            .split(area);

        // Habit list
        let items: Vec<ListItem> = self
            .habits
            .iter()
            .map(|h| {
                if !h.habit.active {
                    let paused_style = Style::default().fg(Color::Gray);
                    return ListItem::new(Line::from(vec![
                        Span::styled("  ", paused_style),
                        Span::styled(&h.habit.title, paused_style),
                        Span::styled(" (paused)", Style::default().fg(Color::DarkGray)),
                    ]));
                }

                let slots = render_slots(h.checkins_this_week, h.habit.frequency);
                let freq_label = if h.habit.frequency == 1 {
                    String::new()
                } else {
                    format!(" {}x/wk", h.habit.frequency)
                };
                let progress = format!("  {}/{}", h.checkins_this_week, h.habit.frequency);
                let streak_str = if h.streak > 0 {
                    format!("  {}wk", h.streak)
                } else {
                    String::new()
                };
                let done_marker = if h.completed { " ✓" } else { "" };

                let title_style = if h.completed {
                    Style::default().fg(Color::Gray)
                } else {
                    Style::default().fg(Color::White)
                };
                let progress_color = if h.completed {
                    Color::Green
                } else if h.checkins_this_week > 0 {
                    Color::Yellow
                } else {
                    Color::Gray
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", slots),
                        Style::default().fg(progress_color),
                    ),
                    Span::styled(h.habit.title.clone(), title_style),
                    Span::styled(freq_label, Style::default().fg(Color::Gray)),
                    Span::styled(progress, Style::default().fg(progress_color)),
                    if h.streak > 0 {
                        Span::styled(streak_str, Style::default().fg(Color::Rgb(255, 140, 0)))
                    } else {
                        Span::raw("")
                    },
                    Span::styled(
                        done_marker,
                        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
                    ),
                ]))
            })
            .collect();

        let active_habits: Vec<_> = self.habits.iter().filter(|h| h.habit.active).collect();
        let total = active_habits.len();
        let completed = active_habits.iter().filter(|h| h.completed).count();
        let paused_count = self.habits.len() - total;
        let title = if paused_count > 0 {
            format!(" Habits ({}/{}) +{} paused ", completed, total, paused_count)
        } else {
            format!(" Habits ({}/{}) ", completed, total)
        };
        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Input for adding
        if self.mode == Mode::Adding {
            self.input
                .render(frame, chunks[1], " New Habit (title | frequency, e.g. Workout | 3) ");
        }

        // Status bar
        let status = match self.mode {
            Mode::Adding => "Enter to create · Esc to cancel".to_string(),
            Mode::ConfirmDelete => {
                let name = self
                    .list_state
                    .selected()
                    .and_then(|i| self.habits.get(i))
                    .map(|h| h.habit.title.as_str())
                    .unwrap_or("this habit");
                format!("Delete \"{}\"? y to confirm, any key to cancel", name)
            }
            Mode::Normal => {
                let pause_label = self
                    .list_state
                    .selected()
                    .and_then(|i| self.habits.get(i))
                    .map(|h| if h.habit.active { "[p]ause" } else { "[p] resume" })
                    .unwrap_or("[p]ause");
                format!(" [x]check-in  [a]dd  [d]elete  {}  [j/k]select  [q]uit", pause_label)
            }
        };
        let hint = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[2]);
    }
}

/// Render visual slots like [xx ] for 2/3 check-ins
fn render_slots(checkins: u8, frequency: u8) -> String {
    let filled = checkins.min(frequency) as usize;
    let empty = (frequency as usize).saturating_sub(filled);
    format!("[{}{}]", "x".repeat(filled), " ".repeat(empty))
}

pub enum HabitAction {
    None,
    Quit,
}
