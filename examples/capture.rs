//! Render the actual Ratatui cell buffer to SVG for visual inspection.
use station::{
    app::App,
    config::{Config, Paths},
    model::{AppState, Snapshot},
    providers,
    runtime::command::CommandRunner,
};
use std::{fmt::Write, path::Path};
fn main() -> anyhow::Result<()> {
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
    a.runs = station::tasks::TaskManager::new(p.state).list()?;
    let args = std::env::args().collect::<Vec<_>>();
    if let Some(view) = args.get(4) {
        match view.as_str() {
            "agent-form" => {
                a.section = station::model::Section::Agents;
                a.update(station::app::Action::NewAgent);
            }
            "agents" => {
                a.section = station::model::Section::Agents;
            }
            _ => {}
        }
    }
    let w = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(140);
    let h = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(45);
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h))?;
    t.draw(|f| {
        station::ui::draw(f, &a);
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
            .unwrap_or("/tmp/station-native.svg"),
        svg,
    )?;
    Ok(())
}
