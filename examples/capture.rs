//! Render the actual Ratatui cell buffer to SVG for visual inspection.
use std::{fmt::Write, path::Path, time::Duration};
use waystation::{
    app::App,
    config::{Config, Paths},
    model::{AppState, Snapshot},
    providers,
    runtime::command::CommandRunner,
};

fn live_app() -> anyhow::Result<App> {
    let p = Paths::discover()?;
    let c = Config::load(&p.config, &p.home)?;
    let mut a = App::new(c, AppState::default());
    a.set_workspaces(providers::projects::discover(&a.config)?);
    if let Some(w) = a.workspace().map(Path::to_path_buf) {
        if let Ok(g) = providers::git::inspect(&w, &CommandRunner) {
            a.git = Snapshot::ready(g, 0);
        }
        a.files = providers::files::list(&w, &w, false).unwrap_or_default();
    }
    let mut sampler = providers::system::SystemSampler::default();
    sampler.sample(Path::new("/proc"), &p.home)?;
    std::thread::sleep(std::time::Duration::from_secs(1));
    a.system = Snapshot::ready(sampler.sample(Path::new("/proc"), &p.home)?, 0);
    if let Some(cpu) = a.system.value.as_ref().and_then(|s| s.cpu) {
        a.cpu_history.push_back(cpu as u64);
    }
    a.services = providers::services::collect(&CommandRunner);
    a.runs = waystation::tasks::TaskManager::new(p.state).list()?;
    Ok(a)
}

fn demo_app(root: &Path) -> anyhow::Result<App> {
    use providers::{
        git::GitState,
        projects::Workspace,
        services::{Container, Listener},
        system::SystemStats,
    };

    let mut a = App::new(Config::default(), AppState::default());
    let workspaces = ["atlas", "relay", "waystation"]
        .into_iter()
        .map(|name| {
            let id = root.join(name);
            std::fs::create_dir(&id)?;
            Ok(Workspace {
                id,
                name: name.into(),
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let selected = workspaces[0].id.clone();
    a.set_workspaces(workspaces);
    a.git = Snapshot::ready(
        GitState {
            branch: Some("main".into()),
            changed: 2,
            ahead: 1,
            worktrees: vec![selected],
            ..GitState::default()
        },
        0,
    );
    a.system = Snapshot::ready(
        SystemStats {
            cpu: Some(18.0),
            memory_used: 6 * 1024 * 1024 * 1024,
            memory_total: 16 * 1024 * 1024 * 1024,
            disk_used: 112 * 1024 * 1024 * 1024,
            disk_total: 512 * 1024 * 1024 * 1024,
            network_rate: Some((83.0 * 1024.0, 16.0 * 1024.0)),
            ..SystemStats::default()
        },
        0,
    );
    a.cpu_history.extend([8, 10, 12, 15, 13, 19, 17, 21, 18]);
    a.services.containers = Snapshot::ready(
        vec![Container {
            id: "demo-api".into(),
            name: "atlas-api".into(),
            state: "running".into(),
            ports: "127.0.0.1:3000->3000/tcp".into(),
        }],
        0,
    );
    a.services.listeners = Snapshot::ready(
        vec![Listener {
            address: "127.0.0.1:8787".into(),
            port: 8787,
            protocol: "tcp".into(),
            pid: Some(4242),
            command: Some("preview-server".into()),
            cwd: None,
        }],
        0,
    );
    a.animation_elapsed = Duration::from_secs(5);
    Ok(a)
}

fn main() -> anyhow::Result<()> {
    let args = std::env::args().collect::<Vec<_>>();
    let (mut a, _demo_root) = if args.get(4).is_some_and(|view| view == "overview-demo") {
        let root = tempfile::tempdir()?;
        (demo_app(root.path())?, Some(root))
    } else {
        (live_app()?, None)
    };
    if let Some(view) = args.get(4) {
        match view.as_str() {
            "agent-form" => {
                a.section = waystation::model::Section::Agents;
                a.update(waystation::app::Action::NewAgent);
            }
            "agents" => {
                a.section = waystation::model::Section::Agents;
            }
            _ => {}
        }
    }
    let w = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(140);
    let h = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(45);
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h))?;
    t.draw(|f| {
        waystation::ui::draw(f, &a);
    })?;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\"><rect width=\"100%\" height=\"100%\" fill=\"#24273a\"/>",
        w * 10,
        h * 20
    );
    let color = |c: ratatui::style::Color| match c {
        ratatui::style::Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
        _ => "#cad3f5".into(),
    };
    for y in 0..h {
        for x in 0..w {
            let c = &t.backend().buffer()[(x, y)];
            let label = c
                .symbol()
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            write!(
                &mut svg,
                "<rect x=\"{}\" y=\"{}\" width=\"10\" height=\"20\" fill=\"{}\"/><text x=\"{}\" y=\"{}\" fill=\"{}\" font-family=\"FiraCode Nerd Font Mono, monospace\" font-size=\"16\">{}</text>",
                x * 10,
                y * 20,
                color(c.bg),
                x * 10,
                y * 20 + 16,
                color(c.fg),
                label
            )?;
        }
    }
    svg.push_str("</svg>");
    std::fs::write(
        args.get(1)
            .map(String::as_str)
            .unwrap_or("/tmp/waystation-native.svg"),
        svg,
    )?;
    Ok(())
}
