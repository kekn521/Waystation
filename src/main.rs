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
    paths.config = std::path::absolute(&paths.config)?;
    let config = Config::load(&paths.config, &paths.home)?;
    if paths.state.is_relative() {
        paths.state = std::env::current_dir()?.join(paths.state);
    }
    if paths.config.is_relative() {
        paths.config = std::env::current_dir()?.join(paths.config);
    }
    let store = Store::open(paths.state.clone())?;
    let mut app = App::new(config, store.load()?);
    let pool = station::runtime::workers::WorkerPool::new(
        app.config.clone(),
        paths.home.clone(),
        paths.state.clone(),
    );
    refresh(&app, &pool);
    let mut last_fast = std::time::Instant::now();
    let mut last_slow = std::time::Instant::now();
    let mut jobs =
        station::runtime::jobs::Jobs::new(station::tasks::TaskManager::new(paths.state.clone()));
    let mut term = TerminalSession::enter()?;
    loop {
        if let Some(result) = jobs.try_recv() {
            match result {
                Ok(r) => {
                    if !r.message.is_empty() {
                        app.message = Some(r.message)
                    }
                    if let Some(d) = r.detail {
                        app.detail = Some(d);
                        app.detail_scroll = 0;
                    }
                    refresh(&app, &pool);
                    if r.quit {
                        break;
                    }
                }
                Err(e) => app.message = Some(e),
            }
        }
        while let Some(event) = pool.try_recv() {
            let first = app.workspace().is_none();
            app.apply_provider(event);
            if first && app.workspace().is_some() {
                refresh(&app, &pool);
            }
        }
        if last_fast.elapsed() >= Duration::from_secs(1) {
            submit(&app, &pool, station::runtime::workers::ProviderId::System);
            submit(&app, &pool, station::runtime::workers::ProviderId::Tasks);
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
                        effect => {
                            if !jobs.submit(effect) {
                                app.message =
                                    Some("An action is still finishing; try again shortly".into())
                            }
                        }
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
        Tasks,
    ] {
        submit(app, pool, id)
    }
}
