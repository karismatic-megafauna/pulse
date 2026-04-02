use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

use crate::models::note::{self, NoteMeta};
use crate::ui::input::{InputAction, InputWidget};
use crate::ui::markdown;

#[derive(Debug, PartialEq)]
enum Mode {
    Normal,
    Creating,
    ConfirmDelete,
}

pub struct NotesTab {
    notes: Vec<NoteMeta>,
    list_state: ListState,
    preview_content: Option<String>,
    preview_scroll: u16,
    mode: Mode,
    input: InputWidget,
}

impl NotesTab {
    pub fn new() -> Self {
        let notes = note::list_notes().unwrap_or_default();
        let mut list_state = ListState::default();
        if !notes.is_empty() {
            list_state.select(Some(0));
        }
        let preview_content = notes
            .first()
            .and_then(|n| note::read_note(&n.filename).ok());
        Self {
            notes,
            list_state,
            preview_content,
            preview_scroll: 0,
            mode: Mode::Normal,
            input: InputWidget::new(),
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        self.mode != Mode::Normal
    }

    pub fn reload(&mut self) {
        self.notes = note::list_notes().unwrap_or_default();
        if let Some(sel) = self.list_state.selected() {
            if self.notes.is_empty() {
                self.list_state.select(None);
                self.preview_content = None;
            } else if sel >= self.notes.len() {
                self.list_state.select(Some(self.notes.len() - 1));
            }
        }
        self.load_preview();
    }

    fn load_preview(&mut self) {
        self.preview_content = self
            .list_state
            .selected()
            .and_then(|i| self.notes.get(i))
            .and_then(|n| note::read_note(&n.filename).ok());
        self.preview_scroll = 0;
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> NotesAction {
        match self.mode {
            Mode::Normal => self.handle_normal(key),
            Mode::Creating => self.handle_creating(key),
            Mode::ConfirmDelete => self.handle_confirm_delete(key),
        }
    }

    fn handle_normal(&mut self, key: KeyEvent) -> NotesAction {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.move_down();
                self.load_preview();
                NotesAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.move_up();
                self.load_preview();
                NotesAction::None
            }
            // Scroll preview
            KeyCode::Char('J') => {
                self.preview_scroll = self.preview_scroll.saturating_add(3);
                NotesAction::None
            }
            KeyCode::Char('K') => {
                self.preview_scroll = self.preview_scroll.saturating_sub(3);
                NotesAction::None
            }
            KeyCode::Char('n') => {
                self.mode = Mode::Creating;
                self.input.clear();
                self.input.set_active(true);
                NotesAction::None
            }
            KeyCode::Enter | KeyCode::Char('e') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(n) = self.notes.get(sel) {
                        return NotesAction::EditNote(n.filename.clone());
                    }
                }
                NotesAction::None
            }
            KeyCode::Char('d') => {
                if self.list_state.selected().is_some() {
                    self.mode = Mode::ConfirmDelete;
                }
                NotesAction::None
            }
            KeyCode::Char('q') => NotesAction::Quit,
            _ => NotesAction::None,
        }
    }

    fn handle_creating(&mut self, key: KeyEvent) -> NotesAction {
        match self.input.handle_key(key) {
            InputAction::Submit(name) => {
                if let Ok(filename) = note::create_note(&name) {
                    self.mode = Mode::Normal;
                    self.input.set_active(false);
                    self.reload();
                    // Select the new note and open it
                    if let Some(pos) = self.notes.iter().position(|n| n.filename == filename) {
                        self.list_state.select(Some(pos));
                        self.load_preview();
                    }
                    return NotesAction::EditNote(filename);
                }
                self.mode = Mode::Normal;
                self.input.set_active(false);
                NotesAction::None
            }
            InputAction::Cancel => {
                self.mode = Mode::Normal;
                self.input.set_active(false);
                NotesAction::None
            }
            InputAction::None => NotesAction::None,
        }
    }

    fn handle_confirm_delete(&mut self, key: KeyEvent) -> NotesAction {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(sel) = self.list_state.selected() {
                    if let Some(n) = self.notes.get(sel) {
                        let _ = note::delete_note(&n.filename);
                        self.reload();
                    }
                }
                self.mode = Mode::Normal;
                NotesAction::None
            }
            _ => {
                self.mode = Mode::Normal;
                NotesAction::None
            }
        }
    }

    fn move_down(&mut self) {
        if self.notes.is_empty() {
            return;
        }
        let next = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.notes.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(next));
    }

    fn move_up(&mut self) {
        if self.notes.is_empty() {
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
                if self.mode == Mode::Creating {
                    Constraint::Length(3)
                } else {
                    Constraint::Length(0)
                },
                Constraint::Length(1),
            ])
            .split(area);

        // Split main area into list (left) and preview (right)
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
            .split(chunks[0]);

        // Note list
        let items: Vec<ListItem> = self
            .notes
            .iter()
            .map(|n| {
                let age = format_age(n.modified);
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

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Notes ({}) ", self.notes.len())),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("▶ ");
        frame.render_stateful_widget(list, cols[0], &mut self.list_state);

        // Preview pane with markdown rendering
        match &self.preview_content {
            Some(content) => {
                let styled_lines = markdown::render_markdown(content);
                let preview = Paragraph::new(styled_lines)
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title(" Preview ")
                            .border_style(Style::default().fg(Color::Cyan)),
                    )
                    .wrap(Wrap { trim: false })
                    .scroll((self.preview_scroll, 0));
                frame.render_widget(preview, cols[1]);
            }
            None => {
                let empty = Paragraph::new(Span::styled(
                    "No note selected",
                    Style::default().fg(Color::DarkGray),
                ))
                .block(Block::default().borders(Borders::ALL).title(" Preview "));
                frame.render_widget(empty, cols[1]);
            }
        }

        // Input for creating
        if self.mode == Mode::Creating {
            self.input.render(frame, chunks[1], " New Note Name ");
        }

        // Status bar
        let status = match self.mode {
            Mode::Creating => "Enter to create · Esc to cancel".to_string(),
            Mode::ConfirmDelete => {
                let name = self
                    .list_state
                    .selected()
                    .and_then(|i| self.notes.get(i))
                    .map(|n| n.title.as_str())
                    .unwrap_or("this note");
                format!("Delete \"{}\"? y to confirm, any key to cancel", name)
            }
            Mode::Normal => {
                " [n]ew  [e/Enter]edit  [d]elete  [j/k]select  [J/K]scroll preview  [q]uit"
                    .to_string()
            }
        };
        let hint = Paragraph::new(status).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[2]);
    }
}

fn format_age(modified: std::time::SystemTime) -> String {
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

pub enum NotesAction {
    None,
    EditNote(String), // filename to open in $EDITOR
    Quit,
}
