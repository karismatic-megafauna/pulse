use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Sparkline, Tabs, Wrap,
    },
    Frame,
};
use rusqlite::Connection;

use crate::models::{
    journal::{self, JournalEntry},
    weight::{self, WeightEntry},
    workout::{self, Workout},
};
use crate::ui::input::{InputAction, InputWidget};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LogTab {
    Workout,
    Weight,
    Journal,
}

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Adding,
    Editing, // journal editing
    ConfirmDelete,
}

pub struct LogsTab {
    pub date: NaiveDate,
    active_log: LogTab,
    mode: Mode,

    // Workout state
    workouts: Vec<Workout>,
    workout_list_state: ListState,
    workout_input: InputWidget, // "type | duration | notes"

    // Weight state
    weight_entry: Option<WeightEntry>,
    weight_input: InputWidget,
    weight_sparkline_data: Vec<u64>,

    // Journal state
    journal_entry: Option<JournalEntry>,
    journal_input: InputWidget,
    journal_scroll: u16,
}

impl LogsTab {
    pub fn new(conn: &Connection, date: NaiveDate) -> Self {
        let workouts = workout::list_for_date(conn, date).unwrap_or_default();
        let weight_entry = weight::get_for_date(conn, date).unwrap_or(None);
        let journal_entry = journal::get_for_date(conn, date).unwrap_or(None);
        let weight_sparkline_data = load_sparkline(conn);

        let mut workout_list_state = ListState::default();
        if !workouts.is_empty() {
            workout_list_state.select(Some(0));
        }

        Self {
            date,
            active_log: LogTab::Workout,
            mode: Mode::Normal,
            workouts,
            workout_list_state,
            workout_input: InputWidget::new(),
            weight_entry,
            weight_input: InputWidget::new(),
            weight_sparkline_data,
            journal_entry,
            journal_input: InputWidget::new(),
            journal_scroll: 0,
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        self.mode != Mode::Normal
    }

    pub fn reload(&mut self, conn: &Connection) {
        self.workouts = workout::list_for_date(conn, self.date).unwrap_or_default();
        self.weight_entry = weight::get_for_date(conn, self.date).unwrap_or(None);
        self.journal_entry = journal::get_for_date(conn, self.date).unwrap_or(None);
        self.weight_sparkline_data = load_sparkline(conn);

        if let Some(sel) = self.workout_list_state.selected() {
            if self.workouts.is_empty() {
                self.workout_list_state.select(None);
            } else if sel >= self.workouts.len() {
                self.workout_list_state.select(Some(self.workouts.len() - 1));
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent, conn: &Connection) -> LogAction {
        // Tab switching within logs (left/right) when not in input mode
        if self.mode == Mode::Normal {
            match key.code {
                KeyCode::Char('h') | KeyCode::Left => {
                    self.prev_log_tab();
                    return LogAction::None;
                }
                KeyCode::Char('l') | KeyCode::Right => {
                    self.next_log_tab();
                    return LogAction::None;
                }
                KeyCode::Char('q') => return LogAction::Quit,
                _ => {}
            }
        }

        match self.active_log {
            LogTab::Workout => self.handle_workout(key, conn),
            LogTab::Weight => self.handle_weight(key, conn),
            LogTab::Journal => self.handle_journal(key, conn),
        }
    }

    fn next_log_tab(&mut self) {
        self.active_log = match self.active_log {
            LogTab::Workout => LogTab::Weight,
            LogTab::Weight => LogTab::Journal,
            LogTab::Journal => LogTab::Workout,
        };
        self.mode = Mode::Normal;
    }

    fn prev_log_tab(&mut self) {
        self.active_log = match self.active_log {
            LogTab::Workout => LogTab::Journal,
            LogTab::Weight => LogTab::Workout,
            LogTab::Journal => LogTab::Weight,
        };
        self.mode = Mode::Normal;
    }

    // ── Workout handlers ──────────────────────────────────────────────────────

    fn handle_workout(&mut self, key: KeyEvent, conn: &Connection) -> LogAction {
        match self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('j') | KeyCode::Down => {
                    self.workout_move_down();
                    LogAction::None
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.workout_move_up();
                    LogAction::None
                }
                KeyCode::Char('a') => {
                    self.mode = Mode::Adding;
                    self.workout_input.clear();
                    self.workout_input.set_active(true);
                    LogAction::None
                }
                KeyCode::Char('d') => {
                    if self.workout_list_state.selected().is_some() {
                        self.mode = Mode::ConfirmDelete;
                    }
                    LogAction::None
                }
                _ => LogAction::None,
            },
            Mode::Adding => match self.workout_input.handle_key(key) {
                InputAction::Submit(raw) => {
                    parse_and_insert_workout(conn, self.date, &raw);
                    self.reload(conn);
                    self.mode = Mode::Normal;
                    self.workout_input.set_active(false);
                    LogAction::None
                }
                InputAction::Cancel => {
                    self.mode = Mode::Normal;
                    self.workout_input.set_active(false);
                    LogAction::None
                }
                InputAction::None => LogAction::None,
            },
            Mode::ConfirmDelete => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    if let Some(sel) = self.workout_list_state.selected() {
                        if let Some(w) = self.workouts.get(sel) {
                            let _ = workout::delete(conn, w.id);
                            self.reload(conn);
                        }
                    }
                    self.mode = Mode::Normal;
                    LogAction::None
                }
                _ => {
                    self.mode = Mode::Normal;
                    LogAction::None
                }
            },
            _ => LogAction::None,
        }
    }

    fn workout_move_down(&mut self) {
        if self.workouts.is_empty() {
            return;
        }
        let next = match self.workout_list_state.selected() {
            Some(i) => (i + 1).min(self.workouts.len() - 1),
            None => 0,
        };
        self.workout_list_state.select(Some(next));
    }

    fn workout_move_up(&mut self) {
        if self.workouts.is_empty() {
            return;
        }
        let prev = match self.workout_list_state.selected() {
            Some(0) | None => 0,
            Some(i) => i - 1,
        };
        self.workout_list_state.select(Some(prev));
    }

    // ── Weight handlers ───────────────────────────────────────────────────────

    fn handle_weight(&mut self, key: KeyEvent, conn: &Connection) -> LogAction {
        match self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('a') | KeyCode::Char('e') => {
                    self.mode = Mode::Adding;
                    let current = self
                        .weight_entry
                        .as_ref()
                        .map(|e| format!("{}", e.weight))
                        .unwrap_or_default();
                    self.weight_input.value = current.clone();
                    self.weight_input.cursor = current.len();
                    self.weight_input.set_active(true);
                    LogAction::None
                }
                KeyCode::Char('d') => {
                    if let Some(entry) = &self.weight_entry {
                        let _ = weight::delete(conn, entry.id);
                        self.reload(conn);
                    }
                    LogAction::None
                }
                _ => LogAction::None,
            },
            Mode::Adding => match self.weight_input.handle_key(key) {
                InputAction::Submit(raw) => {
                    if let Ok(val) = raw.trim().parse::<f64>() {
                        let _ = weight::upsert(conn, self.date, val, None);
                        self.reload(conn);
                    }
                    self.mode = Mode::Normal;
                    self.weight_input.set_active(false);
                    LogAction::None
                }
                InputAction::Cancel => {
                    self.mode = Mode::Normal;
                    self.weight_input.set_active(false);
                    LogAction::None
                }
                InputAction::None => LogAction::None,
            },
            _ => LogAction::None,
        }
    }

    // ── Journal handlers ──────────────────────────────────────────────────────

    fn handle_journal(&mut self, key: KeyEvent, conn: &Connection) -> LogAction {
        match self.mode {
            Mode::Normal => match key.code {
                KeyCode::Char('e') | KeyCode::Enter => {
                    self.mode = Mode::Editing;
                    let current = self
                        .journal_entry
                        .as_ref()
                        .map(|e| e.content.clone())
                        .unwrap_or_default();
                    self.journal_input.value = current.clone();
                    self.journal_input.cursor = current.len();
                    self.journal_input.set_active(true);
                    LogAction::None
                }
                KeyCode::Char('d') => {
                    if let Some(entry) = &self.journal_entry {
                        let _ = journal::delete(conn, entry.id);
                        self.reload(conn);
                    }
                    LogAction::None
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    self.journal_scroll = self.journal_scroll.saturating_add(1);
                    LogAction::None
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    self.journal_scroll = self.journal_scroll.saturating_sub(1);
                    LogAction::None
                }
                _ => LogAction::None,
            },
            Mode::Editing => match self.journal_input.handle_key(key) {
                InputAction::Submit(text) => {
                    let _ = journal::upsert(conn, self.date, &text, None);
                    self.reload(conn);
                    self.journal_scroll = 0;
                    self.mode = Mode::Normal;
                    self.journal_input.set_active(false);
                    LogAction::None
                }
                InputAction::Cancel => {
                    self.mode = Mode::Normal;
                    self.journal_input.set_active(false);
                    LogAction::None
                }
                InputAction::None => LogAction::None,
            },
            _ => LogAction::None,
        }
    }

    // ── Render ────────────────────────────────────────────────────────────────

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // sub-tab bar
                Constraint::Min(0),    // content
                Constraint::Length(1), // hint
            ])
            .split(area);

        // Sub-tab bar
        let log_tabs = Tabs::new(vec!["Workout", "Weight", "Journal"])
            .block(Block::default().borders(Borders::ALL).title(format!(
                " Logs — {} ",
                self.date.format("%A, %B %-d, %Y")
            )))
            .select(match self.active_log {
                LogTab::Workout => 0,
                LogTab::Weight => 1,
                LogTab::Journal => 2,
            })
            .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(log_tabs, chunks[0]);

        match self.active_log {
            LogTab::Workout => self.render_workout(frame, chunks[1]),
            LogTab::Weight => self.render_weight(frame, chunks[1]),
            LogTab::Journal => self.render_journal(frame, chunks[1]),
        }

        // Hint bar
        let hint_text = match (&self.active_log, &self.mode) {
            (_, Mode::Adding) => "Enter to save · Esc to cancel",
            (_, Mode::Editing) => "Enter to save · Esc to cancel",
            (_, Mode::ConfirmDelete) => "y to confirm delete · any other key to cancel",
            (LogTab::Workout, _) => "[a]dd  [d]elete  [h/l] switch log  [q]quit",
            (LogTab::Weight, _) => "[a/e]log weight  [d]elete  [h/l] switch log  [q]quit",
            (LogTab::Journal, _) => "[e]edit  [d]elete  [j/k]scroll  [h/l] switch log  [q]quit",
        };
        let hint = Paragraph::new(hint_text).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[2]);
    }

    fn render_workout(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                if self.mode == Mode::Adding {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
            ])
            .split(area);

        let items: Vec<ListItem> = self
            .workouts
            .iter()
            .map(|w| {
                let dur = w
                    .duration_minutes
                    .map(|d| format!(" {}min", d))
                    .unwrap_or_default();
                let notes = w
                    .notes
                    .as_deref()
                    .map(|n| format!(" — {}", n))
                    .unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("🏋 {}", w.workout_type),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(format!("{}{}", dur, notes)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(" Workouts "))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, chunks[0], &mut self.workout_list_state);

        if self.mode == Mode::Adding {
            self.workout_input
                .render(frame, chunks[1], " Add Workout (type | duration min | notes) ");
        }
    }

    fn render_weight(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // current entry
                Constraint::Length(5),  // sparkline
                Constraint::Min(0),
                if self.mode == Mode::Adding {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
            ])
            .split(area);

        // Current weight
        let current_text = match &self.weight_entry {
            Some(e) => format!("  {} lbs", e.weight),
            None => "  Not logged today".to_string(),
        };
        let current = Paragraph::new(current_text)
            .block(Block::default().borders(Borders::ALL).title(" Today's Weight "))
            .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
        frame.render_widget(current, chunks[0]);

        // Sparkline (last 30 days)
        let spark = Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title(" 30-day trend "))
            .data(&self.weight_sparkline_data)
            .style(Style::default().fg(Color::Green))
            .bar_set(symbols::bar::NINE_LEVELS);
        frame.render_widget(spark, chunks[1]);

        if self.mode == Mode::Adding {
            self.weight_input
                .render(frame, chunks[3], " Log Weight (e.g. 175.2) ");
        }
    }

    fn render_journal(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                if self.mode == Mode::Editing {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
            ])
            .split(area);

        let content = match &self.journal_entry {
            Some(e) => {
                let mood_str = e
                    .mood
                    .map(|m| format!(" [mood: {}/5]", m))
                    .unwrap_or_default();
                format!("{}{}", e.content, mood_str)
            }
            None => "No entry yet. Press [e] to write.".to_string(),
        };

        let journal = Paragraph::new(content)
            .block(Block::default().borders(Borders::ALL).title(" Journal "))
            .wrap(Wrap { trim: false })
            .scroll((self.journal_scroll, 0))
            .style(Style::default().fg(Color::White));
        frame.render_widget(journal, chunks[0]);

        if self.mode == Mode::Editing {
            self.journal_input
                .render(frame, chunks[1], " Edit Entry (single line — expand in Phase 3) ");
        }
    }
}

fn load_sparkline(conn: &Connection) -> Vec<u64> {
    let entries = weight::list_recent(conn, 30).unwrap_or_default();
    if entries.is_empty() {
        return vec![];
    }
    let min = entries
        .iter()
        .map(|e| e.weight)
        .fold(f64::MAX, f64::min);
    // Normalize to u64 range, keep relative differences visible
    entries
        .iter()
        .map(|e| ((e.weight - min) * 10.0) as u64 + 1)
        .collect()
}

fn parse_and_insert_workout(conn: &Connection, date: NaiveDate, raw: &str) {
    let parts: Vec<&str> = raw.splitn(3, '|').map(|s| s.trim()).collect();
    let workout_type = parts.first().copied().unwrap_or("Workout");
    let duration = parts
        .get(1)
        .and_then(|s| s.parse::<u32>().ok());
    let notes = parts.get(2).copied();
    let _ = workout::insert(conn, date, workout_type, duration, notes);
}

pub enum LogAction {
    None,
    Quit,
}
