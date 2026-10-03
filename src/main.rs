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
    let args = std::env::args_os().collect::<Vec<_>>();
    if args.get(1).is_some_and(|s| s == "__supervise") {
        anyhow::ensure!(
            args.len() == 4 && args[2] == "--run-dir",
            "Invalid private supervisor invocation"
        );
        return station::tasks::supervisor::run(std::path::Path::new(&args[3]));
    }
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
    let pool = station::runtime::workers::WorkerPool::new(app.config.clone(), paths.home.clone());
    refresh(&app, &pool);
    let mut last_fast = std::time::Instant::now();
    let mut last_slow = std::time::Instant::now();
    let mut term = TerminalSession::enter()?;
    loop {
        while let Some(event) = pool.try_recv() {
            let first = app.workspace().is_none();
            app.apply_provider(event);
            if first && app.workspace().is_some() {
                refresh(&app, &pool);
            }
        }
        if last_fast.elapsed() >= Duration::from_secs(1) {
            submit(&app, &pool, station::runtime::workers::ProviderId::System);
            last_fast = std::time::Instant::now();
        }
        if last_slow.elapsed() >= Duration::from_secs(5) {
            refresh(&app, &pool);
            last_slow = std::time::Instant::now();
        }
        let mut hits = vec![];
        term.terminal.draw(|f| hits = station::ui::draw(f, &app))?;
        app.hits = hits;
        if crossterm::event::poll(Duration::from_millis(100))? {
            if let Some(action) = input::translate(crossterm::event::read()?, &app) {
                let effects = app.update(action);
                if effects.iter().any(|e| matches!(e, Effect::Quit)) {
                    break;
                }
                for effect in effects {
                    match effect {
                        Effect::Refresh => refresh(&app, &pool),
                        Effect::Foreground(action) => {
                            let cwd = if matches!(action, station::app::Action::Shell)
                                && app.section == station::model::Section::Files
                            {
                                app.file_dir.clone()
                            } else {
                                app.state.selected_workspace.clone()
                            }
                            .unwrap_or(paths.home.clone());
                            let result =
                                station::runtime::actions::resolve(&action, &app.config, &cwd)
                                    .and_then(|s| term.run_foreground(&s));
                            let outcome = match result {
                                Ok(status) => format!("Returned · {status}"),
                                Err(e) => format!("{e:#}"),
                            };
                            app.state.activity.push(station::model::ActivityEntry {
                                id: uuid::Uuid::new_v4().to_string(),
                                at: std::time::SystemTime::now(),
                                workspace: app.state.selected_workspace.clone(),
                                kind: station::model::ActivityKind::Launch(format!("{action:?}")),
                                outcome: outcome.clone(),
                            });
                            app.message = Some(outcome);
                            refresh(&app, &pool);
                        }
                        Effect::DockerLogs(id) => {
                            app.message = Some(
                                station::providers::services::logs(&id)
                                    .unwrap_or_else(|e| e.to_string()),
                            );
                        }
                        Effect::Copy(p) => {
                            app.message = Some(
                                station::runtime::actions::copy_path(&p)
                                    .map(|_| "Path copied".into())
                                    .unwrap_or_else(|e| e.to_string()),
                            )
                        }
                        Effect::Quit => {}
                    }
                }
            }
        }
    }
    term.restore()?;
    store.merge(&app.state)?;
    Ok(())
}

fn submit(
    app: &App,
    pool: &station::runtime::workers::WorkerPool,
    id: station::runtime::workers::ProviderId,
) {
    pool.submit(station::runtime::workers::ProviderRequest {
        id,
        generation: app.generation,
        workspace: app.state.selected_workspace.clone(),
        directory: app.file_dir.clone(),
        hidden: app.hidden,
    });
}
fn refresh(app: &App, pool: &station::runtime::workers::WorkerPool) {
    use station::runtime::workers::ProviderId::*;
    for id in [
        Projects,
        Git,
        Files,
        System,
        Connections,
        Sessions,
        Services,
    ] {
        submit(app, pool, id)
    }
}
