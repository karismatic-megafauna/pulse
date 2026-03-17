use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, ListState, Paragraph},
    Frame,
};
use rusqlite::Connection;

use crate::models::goal::{self, Goal};
use crate::ui::input::{InputAction, InputWidget};

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Adding,
    ConfirmDelete,
}

pub struct GoalsTab {
    pub goals: Vec<Goal>,
    pub week: NaiveDate,
    list_state: ListState,
    mode: Mode,
    input: InputWidget,
}

impl GoalsTab {
    pub fn new(conn: &Connection, week: NaiveDate) -> Self {
        let goals = goal::list_for_week(conn, week).unwrap_or_default();
        let mut list_state = ListState::default();
        if !goals.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            goals,
            week,
            list_state,
            mode: Mode::Normal,
            input: InputWidget::new(),
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        self.mode != Mode::Normal
    }

    pub fn reload(&mut self, conn: &Connection) {
        self.goals = goal::list_for_week(conn, self.week).unwrap_or_default();
        if let Some(sel) = self.list_state.selected() {
            if self.goals.is_empty() {
                self.list_state.select(None);
            } else if sel >= self.goals.len() {
                self.list_state.select(Some(self.goals.len() - 1));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> GoalAction {
        match self.mode {
            Mode::Normal => self.handle_normal(key, conn),
            Mode::Adding => self.handle_adding(key, conn),
            Mode::ConfirmDelete => self.handle_confirm_delete(key, conn),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent, conn: &Connection) -> GoalAction {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down();
                GoalAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up();
                GoalAction::None
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Adding;
                self.input.clear();
                self.input.set_active(true);
                GoalAction::None
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.adjust_progress(conn, 10);
                GoalAction::None
            }
            KeyCode::Char('-') => {
                self.adjust_progress(conn, -10);
                GoalAction::None
            }
            KeyCode::Char(']') => {
                self.adjust_progress(conn, 5);
                GoalAction::None
            }
            KeyCode::Char('[') => {
                self.adjust_progress(conn, -5);
                GoalAction::None
            }
            KeyCode::Char('d') => {
                if self.list_state.selected().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
                GoalAction::None
            }
            KeyCode::Char('q') => GoalAction::Quit,
            _ => GoalAction::None,
        }
    }

    fn adjust_progress(&mut self, conn: &Connection, delta: i32) {
        if let Some(sel) = self.list_state.selected() {
            if let Some(g) = self.goals.get(sel) {
                let new_progress = ((g.progress as i32 + delta).clamp(0, 100)) as u8;
                let _ = goal::set_progress(conn, g.id, new_progress);
                self.reload(conn);
            }
        }
    }

    fn handle_adding(&mut self, key: KeyEvent, conn: &Connection) -> GoalAction {
        match self.input.handle_key(key) {
            InputAction::Submit(title) => {
                let _ = goal::insert(conn, &title, self.week);
                self.reload(conn);
                if !self.goals.is_empty() {
                    self.list_state.select(Some(self.goals.len() - 1));
                }
                self.mode = Mode::Normal;
                self.input.set_active(false);
                GoalAction::None
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
                GoalAction::None
            }
            InputAction::None => GoalAction::None,
        }
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent, conn: &Connection) -> GoalAction {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(g) = self.goals.get(sel) {
                        let _ = goal::delete(conn, g.id);
                        self.reload(conn);
                    }
                }
                self.mode = Mode::Normal;
                GoalAction::None
            }
            _ => {
                self.mode = Mode::Normal;
                GoalAction::None
            }
        }
    }

    fn move_down(&mut self) {
        if self.goals.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.goals.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    fn move_up(&mut self) {
        if self.goals.is_empty() {
            return;
        }
        let prev = match self.list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(prev));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let week_start = goal::week_start(self.week);
        let week_end = week_start + chrono::Duration::days(6);
        let header = format!(
            " Goals — Week of {} – {} ",
            week_start.format("%b %-d"),
            week_end.format("%b %-d, %Y")
        );

        // Split into list (left) and selected goal detail / gauge (right)
        let outer_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                if self.mode == Mode::Adding {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Length(1),
            ])
            .split(area);

        // Goal list items
        let items: Vec<ListItem> = self
            .goals
            .iter()
            .map(|g| {
                let check = if g.completed { "✓" } else { "○" };
                let style = if g.completed {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default()
                };
                let progress_bar = build_mini_bar(g.progress);
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{} ", check), style),
                    Span::styled(g.title.clone(), style),
                    Span::styled(
                        format!("  {} {}%", progress_bar, g.progress),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]))
            })
            .collect();

        // Render the list + gauge for selected goal
        let list_and_gauge = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(outer_chunks[0]);

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(header))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, list_and_gauge[0], &mut self.list_state);

        // Detail panel: gauge for selected goal
        if let Some(sel) = self.list_state.selected() {
            if let Some(g) = self.goals.get(sel) {
                let gauge = Gauge::default()
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(format!(" {} ", g.title)),
                    )
                    .gauge_style(if g.completed {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Cyan)
                    })
                    .percent(g.progress as u16)
                    .label(format!("{}%", g.progress));
                frame.render_widget(gauge, list_and_gauge[1]);
            }
        } else {
            let empty = Paragraph::new("No goal selected")
                .block(Block::default().borders(Borders::ALL).title(" Progress "))
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, list_and_gauge[1]);
        }

        // Input field when adding
        if self.mode == Mode::Adding {
            self.input.render(frame, outer_chunks[1], " New Goal ");
        }

        // Status bar
        let status = if self.mode == Mode::Adding {
            "Enter to save · Esc to cancel".to_string()
        } else if self.mode == Mode::ConfirmDelete {
            "Delete goal? Press y to confirm, any other key to cancel".to_string()
        } else {
            let done = self.goals.iter().filter(|g| g.completed).count();
            format!(
                " {}/{} complete  [a]dd  [+/-]progress  [d]elete  [q]quit",
                done,
                self.goals.len()
            )
        };
        let hint = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, outer_chunks[2]);
    }
}

fn build_mini_bar(progress: u8) -> String {
    let filled = (progress as usize * 10) / 100;
    let empty = 10 - filled;
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

pub enum GoalAction {
    None,
    Quit,
}
