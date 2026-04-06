use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};
use rusqlite::Connection;

use crate::models::task::{self, Task};
use crate::ui::input::{InputAction, InputWidget};

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Adding,
    Editing(i64),
    ConfirmDelete,
}

pub struct TasksTab {
    pub tasks: Vec<Task>,
    pub date: NaiveDate,
    list_state: ListState,
    mode: Mode,
    input: InputWidget,
}

impl TasksTab {
    pub fn new(conn: &Connection, date: NaiveDate) -> Self {
        let tasks = task::list_for_date(conn, date).unwrap_or_default();
        let mut list_state = ListState::default();
        if !tasks.is_empty() {
            list_state.select(Some(0));
        }
        Self {
            tasks,
            date,
            list_state,
            mode: Mode::Normal,
            input: InputWidget::new(),
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        !matches!(self.mode, Mode::Normal)
    }

    pub fn reload(&mut self, conn: &Connection) {
        self.tasks = task::list_for_date(conn, self.date).unwrap_or_default();
        // Keep selection in bounds
        if let Some(sel) = self.list_state.selected() {
            if self.tasks.is_empty() {
                self.list_state.select(None);
            } else if sel >= self.tasks.len() {
                self.list_state.select(Some(self.tasks.len() - 1));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> TaskAction {
        match self.mode {
            Mode::Normal => self.handle_normal(key, conn),
            Mode::Adding => self.handle_adding(key, conn),
            Mode::Editing(id) => self.handle_editing(key, conn, id),
            Mode::ConfirmDelete => self.handle_confirm_delete(key, conn),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent, conn: &Connection) -> TaskAction {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down();
                TaskAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up();
                TaskAction::None
            }
            KeyCode::Char('a') => {
                self.mode = Mode::Adding;
                self.input.clear();
                self.input.set_active(true);
                TaskAction::None
            }
            KeyCode::Char('x') | KeyCode::Char(' ') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(task) = self.tasks.get(sel) {
                        let id = task.id;
                        let _ = task::toggle_complete(conn, id);
                        self.reload(conn);
                    }
                }
                TaskAction::None
            }
            KeyCode::Char('s') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(task) = self.tasks.get(sel) {
                        if !task.completed {
                            return TaskAction::StartFocusTimer(task.id, task.title.clone());
                        }
                    }
                }
                TaskAction::None
            }
            KeyCode::Char('e') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(task) = self.tasks.get(sel) {
                        self.input.clear();
                        self.input.set_value(&task.title);
                        self.input.set_active(true);
                        self.mode = Mode::Editing(task.id);
                    }
                }
                TaskAction::None
            }
            KeyCode::Char('d') => {
                if self.list_state.selected().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
                TaskAction::None
            }
            KeyCode::Char('q') => TaskAction::Quit,
            _ => TaskAction::None,
        }
    }

    fn handle_adding(&mut self, key: KeyEvent, conn: &Connection) -> TaskAction {
        match self.input.handle_key(key) {
            InputAction::Submit(title) => {
                let _ = task::insert(conn, &title, self.date);
                self.reload(conn);
                // Select the new task (last in list)
                if !self.tasks.is_empty() {
                    self.list_state.select(Some(self.tasks.len() - 1));
                }
                self.mode = Mode::Normal;
                self.input.set_active(false);
                TaskAction::None
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
                TaskAction::None
            }
            InputAction::None => TaskAction::None,
        }
    }

    fn handle_editing(&mut self, key: KeyEvent, conn: &Connection, id: i64) -> TaskAction {
        match self.input.handle_key(key) {
            InputAction::Submit(title) => {
                let _ = task::update_title(conn, id, &title);
                self.reload(conn);
                self.mode = Mode::Normal;
                self.input.set_active(false);
                TaskAction::None
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
                TaskAction::None
            }
            InputAction::None => TaskAction::None,
        }
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent, conn: &Connection) -> TaskAction {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(task) = self.tasks.get(sel) {
                        let id = task.id;
                        let _ = task::delete(conn, id);
                        self.reload(conn);
                    }
                }
                self.mode = Mode::Normal;
                TaskAction::None
            }
            _ => {
                self.mode = Mode::Normal;
                TaskAction::None
            }
        }
    }

    fn move_down(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.tasks.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    fn move_up(&mut self) {
        if self.tasks.is_empty() {
            return;
        }
        let prev = match self.list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.list_state.select(Some(prev));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = if matches!(self.mode, Mode::Adding | Mode::Editing(_)) {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Min(3),
                    Constraint::Length(3),
                    Constraint::Length(1),
                ])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(area)
        };

        // Task list
        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .map(|t| {
                let checkbox = if t.completed { "[x]" } else { "[ ]" };
                let style = if t.completed {
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("{} ", checkbox),
                        Style::default().fg(if t.completed { Color::Green } else { Color::Gray }),
                    ),
                    Span::styled(&t.title, style),
                ]))
            })
            .collect();

        let header = format!(
            " Tasks — {} ",
            self.date.format("%A, %B %-d, %Y")
        );

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(header))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");

        frame.render_stateful_widget(list, chunks[0], &mut self.list_state);

        // Input or confirm delete overlay
        if self.mode == Mode::Adding {
            self.input.render(frame, chunks[1], " New Task ");
            let hint = Paragraph::new("Enter to save · Esc to cancel")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[chunks.len() - 1]);
        } else if matches!(self.mode, Mode::Editing(_)) {
            self.input.render(frame, chunks[1], " Edit Task ");
            let hint = Paragraph::new("Enter to save · Esc to cancel")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[chunks.len() - 1]);
        } else if self.mode == Mode::ConfirmDelete {
            let task_name = self
                .list_state
                .selected()
                .and_then(|i| self.tasks.get(i))
                .map(|t| t.title.as_str())
                .unwrap_or("this task");
            let confirm = Paragraph::new(format!(
                " Delete \"{}\"? Press y to confirm, any other key to cancel",
                task_name
            ))
            .style(Style::default().fg(Color::Red).add_modifier(Modifier::BOLD));
            frame.render_widget(confirm, chunks[chunks.len() - 1]);
        } else {
            // Status bar
            let (done, total) = self
                .tasks
                .iter()
                .fold((0, 0), |(d, t), task| {
                    (d + task.completed as usize, t + 1)
                });
            let status = if total == 0 {
                " [a]dd  [q]uit".to_string()
            } else {
                format!(
                    " {}/{} done  [a]dd  [e]dit  [x]toggle  [s]focus  [d]elete  [q]quit",
                    done, total
                )
            };
            let hint = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, chunks[chunks.len() - 1]);
        }
    }
}

pub enum TaskAction {
    None,
    StartFocusTimer(i64, String),
    Quit,
}
