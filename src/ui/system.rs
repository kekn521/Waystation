use crate::{
    app::{Action, App, HitRegion},
    ui::{self, theme::*},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    widgets::{Gauge, Paragraph, Sparkline},
};
pub fn bytes(n: u64) -> String {
    if n >= 1024 * 1024 * 1024 {
        format!("{:.1} GiB", n as f64 / (1024. * 1024. * 1024.))
    } else {
        format!("{:.0} MiB", n as f64 / (1024. * 1024.))
    }
}
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let block = ui::panel(
        "Machine pulse · live",
        app.pane == 1 || app.section == crate::model::Section::System,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(s) = &app.system.value else {
        frame.render_widget(
            Paragraph::new(
                app.provider_errors
                    .get("System")
                    .map(String::as_str)
                    .unwrap_or(" Sampling local machine…"),
            )
            .fg(MUTED),
            inner,
        );
        return vec![];
    };
    let parts = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .margin(1)
    .split(inner);
    let ratio = |a: u64, b: u64| {
        if b == 0 {
            0.
        } else {
            (a as f64 / b as f64).clamp(0., 1.)
        }
    };
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(MAUVE).bg(LINE))
            .ratio(s.cpu.unwrap_or(0.) / 100.)
            .label(
                s.cpu
                    .map(|n| format!("CPU  {n:.0}%"))
                    .unwrap_or("CPU · warming up".into()),
            ),
        parts[0],
    );
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(BLUE).bg(LINE))
            .ratio(ratio(s.memory_used, s.memory_total))
            .label(format!(
                "RAM {} / {}",
                bytes(s.memory_used),
                bytes(s.memory_total)
            )),
        parts[1],
    );
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(TEAL).bg(LINE))
            .ratio(ratio(s.disk_used, s.disk_total))
            .label(format!(
                "DISK {} / {}",
                bytes(s.disk_used),
                bytes(s.disk_total)
            )),
        parts[2],
    );
    let history = app.cpu_history.iter().copied().collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(history)
            .max(100)
            .style(Style::default().fg(MAUVE)),
        parts[3],
    );
    let mut lines = vec![
        s.network_rate
            .map(|(rx, tx)| format!(" ↓ {:.1} KiB/s   ↑ {:.1} KiB/s", rx / 1024., tx / 1024.))
            .unwrap_or(" Network · warming up".into()),
        format!(" GPU {}", s.gpu.as_deref().unwrap_or("unavailable")),
        "".into(),
    ];
    if let Some(e) = app.provider_errors.get("System") {
        lines.push(format!("Stale · {e}"));
    }
    if app.section == crate::model::Section::System {
        lines.push(" PID      MEMORY    PROCESS     · Enter opens htop".into());
        lines.extend(
            s.processes
                .iter()
                .take(parts[4].height.saturating_sub(4) as usize)
                .map(|p| {
                    format!(
                        " {:<8} {:>9}  {}",
                        p.pid,
                        bytes(p.memory),
                        ui::safe(&p.name)
                    )
                }),
        );
    }
    frame.render_widget(
        Paragraph::new(ui::safe(&lines.join("\n"))).fg(MUTED),
        parts[4],
    );
    vec![HitRegion {
        area,
        action: Action::Tool("htop".into()),
    }]
}
