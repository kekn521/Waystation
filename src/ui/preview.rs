//! A read-only look at the open moon's screen, beside its system; Enter or a click attaches.

use super::theme::*;
use crate::{
    agents::AgentStatus,
    app::{Action, App, HitRegion},
    model::Availability,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Style, Stylize},
    text::Line,
    widgets::{Block, Borders, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthChar;

/// The last `height` lines of `text` that hold anything, each cut to `width` columns, so
/// the newest output stays in view without wrapping.
pub fn tail(text: &str, width: u16, height: u16) -> Vec<String> {
    let mut lines = text.lines().collect::<Vec<_>>();
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let skip = lines.len().saturating_sub(usize::from(height));
    lines[skip..]
        .iter()
        .map(|line| {
            let mut used = 0;
            super::safe(line)
                .replace('\t', " ")
                .chars()
                .take_while(|c| {
                    used += c.width().unwrap_or(0);
                    used <= usize::from(width)
                })
                .collect()
        })
        .collect()
}

pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let Some(id) = app.orbit.level.agent() else {
        return vec![];
    };
    let Some(session) = app.agents.iter().find(|s| s.id == id) else {
        return vec![];
    };
    let snapshot = app
        .agent_preview
        .as_ref()
        .filter(|(previewed, _)| *previewed == id)
        .map(|(_, s)| s);
    let screen = snapshot.and_then(|s| s.value.as_deref());
    let failed = snapshot.and_then(|s| match &s.availability {
        Availability::Failed(e) => Some(e.as_str()),
        _ => None,
    });
    let title = format!(
        "{} · {} · {}{} · Enter attaches",
        session.name,
        session.tool,
        super::orbit::status_label(&session.status),
        if failed.is_some() && screen.is_some() {
            " · stale"
        } else {
            ""
        }
    );
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(Style::default().fg(LINE))
        .title(Line::from(format!(" {} ", super::safe(&title))).fg(MUTED))
        .style(Style::default().bg(BASE).fg(TEXT));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let note = |text: &str| Paragraph::new(format!(" {text}")).fg(MUTED);
    let body = match (&session.status, screen, failed) {
        (AgentStatus::Saved, ..) => note("Saved conversation · Enter resumes it"),
        (AgentStatus::Unavailable, ..) => note("This session is gone"),
        (_, Some(text), _) => Paragraph::new(tail(text, inner.width, inner.height).join("\n")),
        (_, None, Some(e)) => note(&super::safe(e)).wrap(Wrap { trim: false }),
        _ => note("Connecting…"),
    };
    frame.render_widget(body, inner);
    vec![HitRegion {
        area,
        action: Action::OrbitAttach(id),
    }]
}
