use anyhow::Result;
use clap::Parser;
use std::{path::PathBuf, time::Duration};
use waystation::{
    app::{App, Effect},
    config::{Config, Paths},
    input,
    runtime::terminal::TerminalSession,
    store::Store,
};
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
        return waystation::tasks::supervisor::run(std::path::Path::new(&args[3]));
    }
    if args.get(1).is_some_and(|s| s == "__statusline") {
        // Claude shows stdout as its status line: print nothing, and never fail.
        let mut input = vec![];
        let _ = std::io::Read::read_to_end(
            &mut std::io::Read::take(std::io::stdin(), 1024 * 1024),
            &mut input,
        );
        if let Some(state) = args.get(2) {
            waystation::usage::statusline(
                &input,
                std::path::Path::new(state),
                waystation::usage::now(),
            );
        }
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "__agent-hook") {
        // Hook output can reach the agent's context, and failures must not disturb it.
        let mut input = vec![];
        let _ = std::io::Read::read_to_end(
            &mut std::io::Read::take(std::io::stdin(), 1024 * 1024),
            &mut input,
        );
        let _ = waystation::agents::record_hook(&input);
        return Ok(());
    }
    if args.get(1).is_some_and(|s| s == "__agent-exec") {
        let resume = args.get(3).is_some_and(|s| s == "--resume");
        anyhow::ensure!(
            args.len() == 3 || (args.len() == 4 && resume),
            "Invalid agent invocation"
        );
        return waystation::agents::exec(std::path::Path::new(&args[2]), resume);
    }
    if args.get(1).is_some_and(|s| s == "__host") {
        return host_command(&args[2..]);
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
    let mut overlay = waystation::ui::overlay::Live::new(waystation::agents::AgentManager::new(
        paths.state.clone(),
    ));
    let pool = waystation::runtime::workers::WorkerPool::new(
        app.config.clone(),
        paths.home.clone(),
        paths.state.clone(),
    );
    refresh(&app, &pool);
    let mut tasks_ready = false;
    let mut last_fast = std::time::Instant::now();
    let mut last_slow = std::time::Instant::now();
    let mut jobs = waystation::runtime::jobs::Jobs::new(
        waystation::tasks::TaskManager::new(paths.state.clone()),
        waystation::agents::AgentManager::new(paths.state.clone()),
    );
    let mut term = TerminalSession::enter()?;
    let animation_started = std::time::Instant::now();
    loop {
        if let Some(result) = jobs.try_recv() {
            match result {
                Ok(r) => {
                    if let Some(agent) = r.agent {
                        app.form = None;
                        app.update(waystation::app::Action::SelectWorkspace(
                            agent.workspace.clone(),
                        ));
                        app.section = waystation::model::Section::Agents;
                        let id = agent.id;
                        app.agents.retain(|s| s.id != id);
                        app.agents.push(agent);
                        app.selection = app.agent_items().iter().position(|(_,_,a)| matches!(a, waystation::app::Action::OpenAgent(s) if *s == id)).unwrap_or(0);
                        jobs.submit(Effect::AttachAgent(id));
                    }
                    if let Some(spec) = r.attach {
                        app.section = waystation::model::Section::Agents;
                        app.message = Some(
                            match term.host(&spec, &mut app.overlay_visible, &mut |buf| {
                                overlay.draw(buf)
                            }) {
                                Ok(outcome) => waystation::agents::attach_message(
                                    outcome.status,
                                    &outcome.last_lines,
                                ),
                                Err(e) => format!("Could not open agent: {e:#}"),
                            },
                        );
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
            if event.request.id == waystation::runtime::workers::ProviderId::Tasks {
                tasks_ready = true;
            }
            let first = app.workspace().is_none();
            app.apply_provider(event);
            if first && app.workspace().is_some() {
                refresh(&app, &pool);
            }
        }
        if last_fast.elapsed() >= Duration::from_secs(1) {
            submit(
                &app,
                &pool,
                waystation::runtime::workers::ProviderId::System,
            );
            submit(&app, &pool, waystation::runtime::workers::ProviderId::Tasks);
            submit(
                &app,
                &pool,
                waystation::runtime::workers::ProviderId::Agents,
            );
            last_fast = std::time::Instant::now();
        }
        if last_slow.elapsed() >= Duration::from_secs(5) {
            refresh(&app, &pool);
            last_slow = std::time::Instant::now();
        }
        let mut hits = vec![];
        app.animation_elapsed = animation_started.elapsed();
        term.terminal
            .draw(|f| hits = waystation::ui::draw(f, &app))?;
        app.hits = hits;
        if crossterm::event::poll(Duration::from_millis(100))?
            && let Some(action) = input::translate(crossterm::event::read()?, &app)
        {
            if matches!(action, waystation::app::Action::Quit) && (!tasks_ready || jobs.busy) {
                app.message =
                    Some("Finishing background synchronization; try quit again shortly".into());
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
                        let cwd = if matches!(action, waystation::app::Action::Shell)
                            && app.section == waystation::model::Section::Files
                        {
                            app.file_dir.clone()
                        } else {
                            app.state.selected_workspace.clone()
                        }
                        .unwrap_or(paths.home.clone());
                        let result =
                            waystation::runtime::actions::resolve(&action, &app.config, &cwd)
                                .and_then(|s| {
                                    term.host(&s, &mut app.overlay_visible, &mut |buf| {
                                        overlay.draw(buf)
                                    })
                                });
                        let outcome = match result {
                            Ok(outcome) => format!("Returned · {}", outcome.status),
                            Err(e) => format!("{e:#}"),
                        };
                        app.state.activity.push(waystation::model::ActivityEntry {
                            id: uuid::Uuid::new_v4().to_string(),
                            at: std::time::SystemTime::now(),
                            workspace: app.state.selected_workspace.clone(),
                            kind: waystation::model::ActivityKind::Launch(format!("{action:?}")),
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
    pool: &waystation::runtime::workers::WorkerPool,
    id: waystation::runtime::workers::ProviderId,
) {
    pool.submit(waystation::runtime::workers::ProviderRequest {
        id,
        generation: app.generation,
        workspace: app.state.selected_workspace.clone(),
        directory: app.file_dir.clone(),
        hidden: app.hidden,
    });
}
fn refresh(app: &App, pool: &waystation::runtime::workers::WorkerPool) {
    use waystation::runtime::workers::ProviderId::*;
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
        Usage,
    ] {
        submit(app, pool, id)
    }
}

/// `__host [--state-dir DIR] -- PROGRAM ARGS...`: runs one program hosted, with the agent
/// box, and exits with its status. Used to exercise the host directly.
fn host_command(args: &[std::ffi::OsString]) -> Result<()> {
    let split = args.iter().position(|a| a == "--");
    let (options, program) = match split {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (&[][..], args),
    };
    anyhow::ensure!(
        !program.is_empty(),
        "Usage: __host [--state-dir DIR] -- PROGRAM ARGS..."
    );
    let state = match options {
        [flag, dir] if flag == "--state-dir" => std::path::PathBuf::from(dir),
        [] => Paths::discover()?.state,
        _ => anyhow::bail!("Usage: __host [--state-dir DIR] -- PROGRAM ARGS..."),
    };
    let spec = waystation::runtime::command::CommandSpec {
        program: program[0].clone(),
        args: program[1..].to_vec(),
        cwd: std::env::current_dir()?,
    };
    let mut overlay =
        waystation::ui::overlay::Live::new(waystation::agents::AgentManager::new(state));
    let mut visible = true;
    let outcome = {
        let mut term = TerminalSession::enter()?;
        term.host(&spec, &mut visible, &mut |buf| overlay.draw(buf))?
    };
    std::process::exit(outcome.status.code().unwrap_or(1));
}
