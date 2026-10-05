use crate::{
    app::{Action, App, HitRegion},
    ui::{self, theme::*},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
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
        app.section == crate::model::Section::System
            || (app.section == crate::model::Section::Overview && app.pane == 2),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(s) = &app.system.value else {
        frame.render_widget(
            Paragraph::new(ui::safe(
                app.provider_errors
                    .get("System")
                    .map(String::as_str)
                    .unwrap_or(" Sampling local machine…"),
            ))
            .fg(MUTED),
            inner,
        );
        return vec![];
    };
    let inner = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let parts = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Min(0),
    ])
    .split(inner);
    let ratio = |a: u64, b: u64| {
        if b == 0 {
            0.
        } else {
            (a as f64 / b as f64).clamp(0., 1.)
        }
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("CPU / 60s   ", Style::default().fg(MUTED)),
            Span::styled(
                s.cpu
                    .map(|n| format!("{n:.0}%"))
                    .unwrap_or("warming up".into()),
                Style::default().fg(TEAL),
            ),
        ])),
        parts[0],
    );
    let history = app.cpu_history.iter().copied().collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .data(history)
            .max(100)
            .style(Style::default().fg(TEAL)),
        parts[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Memory  {} / {}",
            bytes(s.memory_used),
            bytes(s.memory_total)
        ))
        .fg(MUTED),
        parts[2],
    );
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(BLUE).bg(LINE))
            .ratio(ratio(s.memory_used, s.memory_total))
            .label(""),
        parts[3],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Disk    {} / {}",
            bytes(s.disk_used),
            bytes(s.disk_total)
        ))
        .fg(MUTED),
        parts[4],
    );
    frame.render_widget(
        Gauge::default()
            .gauge_style(Style::default().fg(MAUVE).bg(LINE))
            .ratio(ratio(s.disk_used, s.disk_total))
            .label(""),
        parts[5],
    );
    let network = s
        .network_rate
        .map(|(rx, tx)| format!("↓ {:.1} KiB/s   ↑ {:.1} KiB/s", rx / 1024., tx / 1024.))
        .unwrap_or("Network · warming up".into());
    frame.render_widget(Paragraph::new(network).fg(TEAL), parts[6]);
    let mut lines = vec![format!("GPU {}", s.gpu.as_deref().unwrap_or("unavailable"))];
    if let Some(e) = app.provider_errors.get("System") {
        lines.push(format!("Stale · {e}"));
    }
    if app.section == crate::model::Section::System {
        lines.push("".into());
        lines.push("PID      MEMORY     PROCESS · Enter opens htop".into());
        lines.extend(
            s.processes
                .iter()
                .take(parts[7].height.saturating_sub(3) as usize)
                .map(|p| format!("{:<8} {:>9}  {}", p.pid, bytes(p.memory), p.name)),
        );
    }
    frame.render_widget(
        Paragraph::new(ui::safe(&lines.join("\n"))).fg(MUTED),
        parts[7],
    );
    vec![HitRegion {
        area,
        action: Action::Tool("htop".into()),
    }]
}
