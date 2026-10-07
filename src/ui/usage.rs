//! The plan limit and token panel shown above the Agents list.
use super::theme::*;
use crate::{
    app::App,
    usage::{
        LimitWindow, StatusLine, ToolUsage, Usage, format_age, format_reset, format_tokens,
        remaining_label,
    },
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::Paragraph,
};

fn tools(usage: &Usage) -> Vec<(&'static str, &ToolUsage)> {
    [("Claude", &usage.claude), ("Codex", &usage.codex)]
        .into_iter()
        .filter_map(|(name, tool)| Some((name, tool.as_ref()?)))
        .collect()
}
/// Rows for the panel in an area this tall: three lines per tool when there is room,
/// one when space is short, none rather than crowd out the session list.
pub fn height(app: &App, available: u16) -> u16 {
    let Some(usage) = &app.usage else {
        return 0;
    };
    let n = tools(usage).len() as u16;
    if n == 0 {
        return 0;
    }
    let (full, compact) = (2 + 3 * n, 2 + n);
    if available >= full + 8 {
        full
    } else if available >= compact + 4 {
        compact
    } else {
        0
    }
}
/// Colour for what is left of a window.
fn color(remaining: f64) -> ratatui::style::Color {
    match remaining {
        p if p <= 10. => RED,
        p if p <= 30. => PEACH,
        _ => TEAL,
    }
}
fn window_line<'a>(
    name: &'a str,
    label: &'a str,
    window: Option<&LimitWindow>,
    now: i64,
    bar: usize,
) -> Line<'a> {
    let current = window.and_then(|w| w.remaining(now));
    let filled = current.map_or(0, |p| {
        ((p.clamp(0., 100.) / 100.) * bar as f64).round() as usize
    });
    let mut spans = vec![
        Span::styled(format!("{name:<7}"), Style::default().fg(TEXT).bold()),
        Span::styled(format!("{label} "), Style::default().fg(MUTED)),
        Span::styled(
            "█".repeat(filled),
            Style::default().fg(current.map_or(LINE, color)),
        ),
        Span::styled("░".repeat(bar - filled), Style::default().fg(LINE)),
        Span::styled(
            format!(" {:>9}", remaining_label(window, now)),
            Style::default().fg(current.map_or(MUTED, color)),
        ),
    ];
    if let Some(w) = window.filter(|_| current.is_some()) {
        spans.push(Span::styled(
            format!(" · resets {}", format_reset(w.resets_at, now)),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(spans)
}
fn hint(name: &str, statusline: &StatusLine) -> &'static str {
    match (name, statusline) {
        ("Claude", StatusLine::Missing) => "plan limits: install the status line below",
        ("Claude", StatusLine::Other(_)) => "plan limits need Waystation's status line (below)",
        ("Claude", _) => "plan limits appear after Claude's next reply",
        _ => "plan limits appear after Codex's next reply",
    }
}
pub fn render(frame: &mut Frame, area: Rect, app: &App) {
    let Some(usage) = &app.usage else {
        return;
    };
    let tools = tools(usage);
    let ages = tools
        .iter()
        .filter_map(|(name, tool)| {
            let updated = tool.limits.as_ref()?.updated_at;
            Some(format!("{name} {} ago", format_age(usage.now - updated)))
        })
        .collect::<Vec<_>>();
    let title = if ages.is_empty() {
        "Usage".to_string()
    } else {
        format!("Usage · {}", ages.join(" · "))
    };
    let block = super::panel(&title, false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let inner = Rect::new(
        inner.x + 1,
        inner.y,
        inner.width.saturating_sub(2),
        inner.height,
    );
    let full = inner.height as usize >= 3 * tools.len();
    let bar = (inner.width as usize).saturating_sub(40).clamp(4, 24);
    let mut lines = vec![];
    for (name, tool) in &tools {
        let tokens = &tool.tokens;
        if !full {
            let mut text = format!("{name:<6} ");
            if let Some(limits) = &tool.limits {
                text.push_str(&format!(
                    "5h {} · wk {} · ",
                    remaining_label(limits.five_hour.as_ref(), usage.now),
                    remaining_label(limits.weekly.as_ref(), usage.now)
                ));
            }
            text.push_str(&format!("7d {}", format_tokens(tokens.week)));
            lines.push(Line::from(text).fg(TEXT));
            continue;
        }
        match &tool.limits {
            Some(limits) => {
                lines.push(window_line(
                    name,
                    "5h",
                    limits.five_hour.as_ref(),
                    usage.now,
                    bar,
                ));
                lines.push(window_line(
                    "",
                    "wk",
                    limits.weekly.as_ref(),
                    usage.now,
                    bar,
                ));
            }
            None => {
                lines.push(Line::from(vec![
                    Span::styled(format!("{name:<7}"), Style::default().fg(TEXT).bold()),
                    Span::styled(hint(name, &usage.statusline), Style::default().fg(MUTED)),
                ]));
                lines.push(Line::default());
            }
        }
        lines.push(Line::from(vec![
            Span::raw(" ".repeat(7)),
            Span::styled(
                format!(
                    "tokens 5h {} · today {} · 7d {}",
                    format_tokens(tokens.five_hour),
                    format_tokens(tokens.today),
                    format_tokens(tokens.week)
                ),
                Style::default().fg(MUTED),
            ),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
