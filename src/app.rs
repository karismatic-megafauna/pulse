use chrono::Local;
use color_eyre::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Tabs},
    Frame, Terminal,
};
use rusqlite::Connection;
use std::time::Duration;

use crate::ui::tasks::{TaskAction, TasksTab};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    // Future: Goals, Logs, Dashboard
}

const TABS: &[(&str, Tab)] = &[
    ("Tasks", Tab::Tasks),
    // ("Goals", Tab::Goals),
    // ("Logs", Tab::Logs),
    // ("Dashboard", Tab::Dashboard),
];

pub struct App {
    pub should_quit: bool,
    current_tab: Tab,
    tasks_tab: TasksTab,
    conn: Connection,
}

impl App {
    pub fn new(conn: Connection) -> Self {
        let today = Local::now().date_naive();
        let tasks_tab = TasksTab::new(&conn, today);
        Self {
            should_quit: false,
            current_tab: Tab::Tasks,
            tasks_tab,
            conn,
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;

            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key);
                }
            }

            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Global: Ctrl+C always quits
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return;
        }

        // Tab switching with Tab key (only in normal mode for now)
        if key.code == KeyCode::Tab {
            self.cycle_tab();
            return;
        }

        // Delegate to active tab
        match self.current_tab {
            Tab::Tasks => {
                let action = self.tasks_tab.handle_key(key, &self.conn);
                if let TaskAction::Quit = action {
                    self.should_quit = true;
                }
            }
        }
    }

    fn cycle_tab(&mut self) {
        // Simple cycle — expand as tabs are added
        let idx = TABS.iter().position(|(_, t)| *t == self.current_tab).unwrap_or(0);
        let next = (idx + 1) % TABS.len();
        self.current_tab = TABS[next].1;
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // tab bar
                Constraint::Min(0),    // content
            ])
            .split(area);

        self.render_tab_bar(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let titles: Vec<Line> = TABS
            .iter()
            .map(|(name, _)| Line::from(Span::raw(*name)))
            .collect();

        let selected = TABS
            .iter()
            .position(|(_, t)| *t == self.current_tab)
            .unwrap_or(0);

        let tabs = Tabs::new(titles)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(Span::styled(
                        " pulse ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    )),
            )
            .select(selected)
            .highlight_style(
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )
            .style(Style::default().fg(Color::DarkGray));

        frame.render_widget(tabs, area);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect) {
        match self.current_tab {
            Tab::Tasks => self.tasks_tab.render(frame, area),
        }
    }
}
