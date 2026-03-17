use chrono::{Duration, Local, NaiveDate};
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
use std::time;

use crate::ui::goals::{GoalAction, GoalsTab};
use crate::ui::logs::{LogAction, LogsTab};
use crate::ui::tasks::{TaskAction, TasksTab};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Tasks,
    Goals,
    Logs,
    // Phase 3: Dashboard,
}

const TABS: &[(&str, Tab)] = &[
    ("Tasks", Tab::Tasks),
    ("Goals", Tab::Goals),
    ("Logs", Tab::Logs),
    // ("Dashboard", Tab::Dashboard),
];

pub struct App {
    pub should_quit: bool,
    current_tab: Tab,
    /// Shared "viewing date" — navigate with < / >
    view_date: NaiveDate,
    tasks_tab: TasksTab,
    goals_tab: GoalsTab,
    logs_tab: LogsTab,
    conn: Connection,
}

impl App {
    pub fn new(conn: Connection) -> Self {
        let today = Local::now().date_naive();
        let tasks_tab = TasksTab::new(&conn, today);
        let goals_tab = GoalsTab::new(&conn, today);
        let logs_tab = LogsTab::new(&conn, today);
        Self {
            should_quit: false,
            current_tab: Tab::Tasks,
            view_date: today,
            tasks_tab,
            goals_tab,
            logs_tab,
            conn,
        }
    }

    pub fn run<B: ratatui::backend::Backend>(
        &mut self,
        terminal: &mut Terminal<B>,
    ) -> Result<()> {
        loop {
            terminal.draw(|f| self.render(f))?;

            if event::poll(time::Duration::from_millis(200))? {
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

        // Global: Tab / Shift+Tab cycles top-level tabs
        if key.code == KeyCode::Tab {
            self.cycle_tab(1);
            return;
        }
        if key.code == KeyCode::BackTab {
            self.cycle_tab(-1);
            return;
        }

        // Global: < / > navigate days (tasks + logs) or weeks (goals)
        if key.code == KeyCode::Char('<') || key.code == KeyCode::Char(',') {
            self.navigate_date(-1);
            return;
        }
        if key.code == KeyCode::Char('>') || key.code == KeyCode::Char('.') {
            self.navigate_date(1);
            return;
        }

        // Delegate to active tab
        match self.current_tab {
            Tab::Tasks => {
                if let TaskAction::Quit = self.tasks_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
            Tab::Goals => {
                if let GoalAction::Quit = self.goals_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
            Tab::Logs => {
                if let LogAction::Quit = self.logs_tab.handle_key(key, &self.conn) {
                    self.should_quit = true;
                }
            }
        }
    }

    fn cycle_tab(&mut self, direction: i32) {
        let idx = TABS
            .iter()
            .position(|(_, t)| *t == self.current_tab)
            .unwrap_or(0) as i32;
        let next = ((idx + direction).rem_euclid(TABS.len() as i32)) as usize;
        self.current_tab = TABS[next].1;
    }

    fn navigate_date(&mut self, delta: i32) {
        match self.current_tab {
            Tab::Tasks | Tab::Logs => {
                self.view_date = self.view_date + Duration::days(delta as i64);
                self.tasks_tab.date = self.view_date;
                self.tasks_tab.reload(&self.conn);
                self.logs_tab.date = self.view_date;
                self.logs_tab.reload(&self.conn);
            }
            Tab::Goals => {
                // Navigate by week
                self.view_date = self.view_date + Duration::weeks(delta as i64);
                self.goals_tab.week = self.view_date;
                self.goals_tab.reload(&self.conn);
            }
        }
    }

    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(0)])
            .split(area);

        self.render_tab_bar(frame, chunks[0]);
        self.render_content(frame, chunks[1]);
    }

    fn render_tab_bar(&self, frame: &mut Frame, area: Rect) {
        let today = Local::now().date_naive();
        let date_label = if self.view_date == today {
            "today".to_string()
        } else {
            self.view_date.format("%b %-d").to_string()
        };

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
                        format!(" pulse [{}] ", date_label),
                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                    ))
                    .title_bottom(
                        Span::styled(
                            " [Tab] switch  [,/.] prev/next day  [Ctrl+C/q] quit ",
                            Style::default().fg(Color::DarkGray),
                        ),
                    ),
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
            Tab::Goals => self.goals_tab.render(frame, area),
            Tab::Logs => self.logs_tab.render(frame, area),
        }
    }
}
