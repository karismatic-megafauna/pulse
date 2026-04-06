use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// A reusable single-line text input widget.
#[derive(Debug, Default, Clone)]
pub struct InputWidget {
    pub value: String,
    pub cursor: usize,
    pub active: bool,
}

impl InputWidget {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.value.clear();
        self.cursor = 0;
    }

    pub fn set_value(&mut self, val: &str) {
        self.value = val.to_string();
        self.cursor = self.value.len();
    }

    pub fn set_active(&mut self, active: bool) {
        self.active = active;
    }

    /// Returns Some(submitted_value) if Enter was pressed, None otherwise.
    pub fn handle_key(&mut self, key: KeyEvent) -> InputAction {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                InputAction::None
            }
            KeyCode::Backspace => {
                if self.cursor > 0 {
                    // Find the previous char boundary
                    let prev = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.value.remove(prev);
                    self.cursor = prev;
                }
                InputAction::None
            }
            KeyCode::Delete => {
                if self.cursor < self.value.len() {
                    self.value.remove(self.cursor);
                }
                InputAction::None
            }
            KeyCode::Left => {
                if self.cursor > 0 {
                    self.cursor = self.value[..self.cursor]
                        .char_indices()
                        .next_back()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
                InputAction::None
            }
            KeyCode::Right => {
                if self.cursor < self.value.len() {
                    self.cursor = self.value[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.value.len());
                }
                InputAction::None
            }
            KeyCode::Home => {
                self.cursor = 0;
                InputAction::None
            }
            KeyCode::End => {
                self.cursor = self.value.len();
                InputAction::None
            }
            KeyCode::Enter => {
                let val = self.value.trim().to_string();
                if !val.is_empty() {
                    InputAction::Submit(val)
                } else {
                    InputAction::Cancel
                }
            }
            KeyCode::Esc => InputAction::Cancel,
            _ => InputAction::None,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, title: &str) {
        let style = if self.active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(style)
            .title(Span::styled(title, Style::default().add_modifier(Modifier::BOLD)));

        // Show value with cursor marker
        let before = &self.value[..self.cursor];
        let cursor_char = self.value[self.cursor..]
            .chars()
            .next()
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after = if self.cursor < self.value.len() {
            &self.value[self.cursor + cursor_char.len()..]
        } else {
            ""
        };

        let line = Line::from(vec![
            Span::raw(before),
            Span::styled(
                cursor_char,
                Style::default()
                    .bg(Color::Yellow)
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(after),
        ]);

        let para = Paragraph::new(line).block(block);
        frame.render_widget(para, area);
    }
}

#[derive(Debug)]
pub enum InputAction {
    Submit(String),
    Cancel,
    None,
}
