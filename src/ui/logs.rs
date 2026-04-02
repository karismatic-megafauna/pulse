use chrono::NaiveDate;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use std::fs;
use std::path::PathBuf;

use crate::config::config_dir;
use crate::ui::markdown;

fn journals_dir() -> PathBuf {
    config_dir().join("journals")
}

fn ensure_journals_dir() {
    let dir = journals_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
}

fn journal_path(date: NaiveDate) -> PathBuf {
    journals_dir().join(format!("{}.md", date.format("%Y-%m-%d")))
}

fn read_journal(date: NaiveDate) -> Option<String> {
    let path = journal_path(date);
    fs::read_to_string(&path).ok().filter(|s| !s.trim().is_empty())
}

fn ensure_journal_file(date: NaiveDate) -> PathBuf {
    ensure_journals_dir();
    let path = journal_path(date);
    if !path.exists() {
        let header = format!("# {}\n\n", date.format("%A, %B %-d, %Y"));
        let _ = fs::write(&path, header);
    }
    path
}

pub struct LogsTab {
    pub date: NaiveDate,
    content: Option<String>,
    scroll: u16,
}

impl LogsTab {
    pub fn new(date: NaiveDate) -> Self {
        let content = read_journal(date);
        Self {
            date,
            content,
            scroll: 0,
        }
    }

    pub fn is_capturing_input(&self) -> bool {
        false
    }

    pub fn reload(&mut self) {
        self.content = read_journal(self.date);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> LogAction {
        match key.code {
            KeyCode::Char('e') | KeyCode::Enter => {
                let path = ensure_journal_file(self.date);
                LogAction::EditJournal(path)
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.scroll = self.scroll.saturating_add(1);
                LogAction::None
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.scroll = self.scroll.saturating_sub(1);
                LogAction::None
            }
            KeyCode::Char('q') => LogAction::Quit,
            _ => LogAction::None,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // title bar
                Constraint::Min(3),   // journal content
                Constraint::Length(1), // hint
            ])
            .split(area);

        // Title bar
        let title_bar = Paragraph::new(Line::from(Span::styled(
            format!(" Journal — {} ", self.date.format("%A, %B %-d, %Y")),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(title_bar, chunks[0]);

        // Journal content with markdown rendering
        match &self.content {
            Some(text) => {
                let styled_lines = markdown::render_markdown(text);
                let journal = Paragraph::new(styled_lines)
                    .block(Block::default().borders(Borders::ALL))
                    .wrap(Wrap { trim: false })
                    .scroll((self.scroll, 0));
                frame.render_widget(journal, chunks[1]);
            }
            None => {
                let empty = Paragraph::new(Span::styled(
                    "No entry yet. Press [e] to write.",
                    Style::default().fg(Color::DarkGray),
                ))
                .block(Block::default().borders(Borders::ALL));
                frame.render_widget(empty, chunks[1]);
            }
        }

        // Hint bar
        let hint =
            Paragraph::new(" [e/Enter]edit in nvim  [j/k]scroll  [,/.]change date  [q]uit")
                .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hint, chunks[2]);
    }
}

pub enum LogAction {
    None,
    EditJournal(PathBuf),
    Quit,
}
