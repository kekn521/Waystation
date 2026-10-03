use anyhow::Result;
use clap::Parser;
use station::{
    app::{App, Effect},
    config::{Config, Paths},
    input,
    runtime::terminal::TerminalSession,
    store::Store,
};
use std::{path::PathBuf, time::Duration};
#[derive(Parser)]
#[command(version, about = "A project-aware terminal workflow hub")]
struct Cli {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    state_dir: Option<PathBuf>,
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    let mut paths = Paths::discover()?;
    if let Some(p) = cli.config {
        paths.config = p;
    }
    if let Some(p) = cli.state_dir {
        paths.state = p;
    }
    let config = Config::load(&paths.config, &paths.home)?;
    let store = Store::open(paths.state)?;
    let mut app = App::new(config, store.load()?);
    let mut term = TerminalSession::enter()?;
    loop {
        let mut hits = vec![];
        term.terminal.draw(|f| hits = station::ui::draw(f, &app))?;
        app.hits = hits;
        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Some(action) = input::translate(crossterm::event::read()?, &app) {
                if app.update(action).iter().any(|e| matches!(e, Effect::Quit)) {
                    break;
                }
            }
        }
    }
    term.restore()?;
    store.merge(&app.state)?;
    Ok(())
}
