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
    if args.get(1).is_some_and(|s| s == "__agent-exec") {
        anyhow::ensure!(args.len() == 3, "Invalid agent invocation");
        return station::agents::exec(std::path::Path::new(&args[2]));
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
    let mut tasks_ready = false;
    let mut last_fast = std::time::Instant::now();
    let mut last_slow = std::time::Instant::now();
    let mut jobs = station::runtime::jobs::Jobs::new(
        station::tasks::TaskManager::new(paths.state.clone()),
        station::agents::AgentManager::new(paths.state.clone()),
        paths.config.clone(),
        paths.home.clone(),
    );
    let mut term = TerminalSession::enter()?;
    loop {
        if let Some(result) = jobs.try_recv() {
            match result {
                Ok(r) => {
                    if let Some(config) = r.config {
                        app.config = config;
                    }
                    if let Some(id) = r.saved_recipe {
                        app.form = None;
                        app.section = station::model::Section::Tasks;
                        app.recipes = true;
                        app.selection = app
                            .config
                            .tasks
                            .iter()
                            .position(|r| r.id == id)
                            .unwrap_or(0);
                    }
                    if let Some(agent) = r.agent {
                        app.form = None;
                        app.update(station::app::Action::SelectWorkspace(
                            agent.workspace.clone(),
                        ));
                        app.section = station::model::Section::Agents;
                        let id = agent.id;
                        app.agents.retain(|s| s.id != id);
                        app.agents.push(agent);
                        app.selection = app.agent_items().iter().position(|(_,_,a)| matches!(a, station::app::Action::OpenAgent(s) if *s == id)).unwrap_or(0);
                        jobs.submit(Effect::AttachAgent(id));
                    }
                    if let Some(spec) = r.attach {
                        app.section = station::model::Section::Agents;
                        app.message = Some(match term.run_foreground(&spec) {
                            Ok(status) if status.success() => {
                                "Back at Station · agent sessions stay available".into()
                            }
                            Ok(status) => format!("Agent attachment ended: {status}"),
                            Err(e) => format!("Could not open agent: {e:#}"),
                        });
                    }
                    if let Some(id) = r.started {
                        app.pending_starts.insert(id);
                    }
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
                Err(e) => {
                    if let Some(form) = &mut app.form {
                        form.busy = false;
                        form.error = Some(e.clone());
                    }
                    app.message = Some(e);
                }
            }
        }
        while let Some(event) = pool.try_recv() {
            if event.request.id == station::runtime::workers::ProviderId::Tasks {
                tasks_ready = true;
            }
            let first = app.workspace().is_none();
            app.apply_provider(event);
            if first && app.workspace().is_some() {
                refresh(&app, &pool);
            }
        }
        if last_fast.elapsed() >= Duration::from_secs(1) {
            submit(&app, &pool, station::runtime::workers::ProviderId::System);
            submit(&app, &pool, station::runtime::workers::ProviderId::Tasks);
            submit(&app, &pool, station::runtime::workers::ProviderId::Agents);
            last_fast = std::time::Instant::now();
        }
        if last_slow.elapsed() >= Duration::from_secs(5) {
            refresh(&app, &pool);
            last_slow = std::time::Instant::now();
        }
        let mut hits = vec![];
        term.terminal.draw(|f| hits = station::ui::draw(f, &app))?;
        app.hits = hits;
        if crossterm::event::poll(Duration::from_millis(100))?
            && let Some(action) = input::translate(crossterm::event::read()?, &app)
        {
            if matches!(action, station::app::Action::Quit) && (!tasks_ready || jobs.busy) {
                app.message = Some("Finishing task synchronization; try quit again shortly".into());
                continue;
            }
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
                        let result = station::runtime::actions::resolve(&action, &app.config, &cwd)
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
                            if let Some(form) = &mut app.form {
                                form.busy = false;
                                form.error =
                                    Some("An action is still finishing; try again shortly".into());
                            }
                            app.message =
                                Some("An action is still finishing; try again shortly".into())
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
        Agents,
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
