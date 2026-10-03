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
    refresh(&mut app);
    let mut term = TerminalSession::enter()?;
    loop {
        let mut hits = vec![];
        term.terminal.draw(|f| hits = station::ui::draw(f, &app))?;
        app.hits = hits;
        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Some(action) = input::translate(crossterm::event::read()?, &app) {
                let effects = app.update(action);
                if effects.iter().any(|e| matches!(e, Effect::Quit)) {
                    break;
                }
                if effects.iter().any(|e| matches!(e, Effect::Refresh)) {
                    refresh(&mut app)
                }
            }
        }
    }
    term.restore()?;
    store.merge(&app.state)?;
    Ok(())
}

fn refresh(app: &mut App) {
    use station::{
        model::{Availability, Snapshot},
        providers::{files, git, projects},
        runtime::command::CommandRunner,
    };
    match projects::discover(&app.config) {
        Ok(w) => app.set_workspaces(w),
        Err(e) => app.message = Some(e.to_string()),
    };
    if let Some(p) = app.workspace().map(|p| p.to_path_buf()) {
        match git::inspect(&p, &CommandRunner) {
            Ok(g) => app.git = Snapshot::ready(g, app.generation),
            Err(e) => app.git.availability = Availability::Failed(e.to_string()),
        };
        match files::list(&p, app.file_dir.as_deref().unwrap_or(&p), app.hidden) {
            Ok(f) => app.files = f,
            Err(e) => app.message = Some(e.to_string()),
        };
    }
}
