use color_eyre::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::panic;

mod app;
mod config;
mod db;
mod integrations;
mod models;
mod reports;
mod site_blocker;
mod ui;

use app::App;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::install()?;

    let cfg = config::load_config()?;
    let conn = db::open_connection()?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let mut app = App::new(conn, cfg);
    let result = app.run(&mut terminal);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Check for errors before exiting
    result?;

    // Exit immediately to avoid panics from background threads still using the
    // Tokio runtime handle during shutdown.
    std::process::exit(0);
}
