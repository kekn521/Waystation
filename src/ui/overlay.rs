//! The floating agent status box drawn over programs Waystation hosts.
use super::theme::*;
use crate::{
    activity::{Activity, ActivityState},
    agents::{AgentSession, AgentStatus},
};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Widget},
};
use std::collections::HashMap;
use uuid::Uuid;

/// One agent in the box.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub icon: &'static str,
    pub color: Color,
    pub name: String,
    pub state: String,
    /// Time since the state changed, when known.
    pub age: String,
    pub detail: String,
    rank: u8,
}

fn age(seconds: i64) -> String {
    match seconds.max(0) {
        s if s < 60 => format!("{s}s"),
        s if s < 3600 => format!("{}m", s / 60),
        s if s < 86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// The box's rows: agents waiting on the user first, then running, done, and the rest.
pub fn rows(sessions: &[AgentSession], activities: &HashMap<Uuid, Activity>, now: i64) -> Vec<Row> {
    let mut rows = sessions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let activity = activities
                .get(&s.id)
                .filter(|_| s.status == AgentStatus::Running);
            let (icon, color, state, rank) = match (&s.status, activity.map(|a| a.state)) {
                (AgentStatus::Running, Some(ActivityState::NeedsInput)) => {
                    ("◐", PEACH, "needs input".into(), 0)
                }
                (AgentStatus::Running, Some(ActivityState::Running)) => {
                    ("●", GREEN, "running".into(), 1)
                }
                (AgentStatus::Running, Some(ActivityState::Done)) => ("✓", MAUVE, "done".into(), 2),
                (AgentStatus::Running, Some(ActivityState::Ready)) => {
                    ("○", TEAL, "ready".into(), 3)
                }
                // Waystation marks every agent it launches; none means it predates tracking.
                (AgentStatus::Running, None) => ("○", MUTED, "untracked".into(), 4),
                (AgentStatus::Exited(code), _) => ("○", MUTED, format!("exited {code}"), 5),
                (AgentStatus::Saved, _) => ("◌", MUTED, "saved".into(), 6),
                (AgentStatus::Unavailable, _) => ("○", MUTED, "unavailable".into(), 7),
            };
            (
                i,
                Row {
                    icon,
                    color,
                    name: s.name.clone(),
                    state,
                    age: activity.map(|a| age(now - a.at)).unwrap_or_default(),
                    detail: match activity {
                        Some(a) => a.detail.clone(),
                        None if s.status == AgentStatus::Running => "r restarts with status".into(),
                        None => String::new(),
                    },
                    rank,
                },
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(i, r)| (r.rank, *i));
    rows.into_iter().map(|(_, r)| r).collect()
}

const MAX_AGENTS: usize = 5;
const WIDTH: u16 = 38;

/// `text` cut to `width` columns, ending in `…` when cut.
fn fit(text: &str, width: usize) -> String {
    use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};
    if text.width() <= width {
        return text.into();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > width {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Draws the box in the top-right corner of `buf`, or nothing when there are no agents or
/// no room.
pub fn render(buf: &mut Buffer, rows: &[Row]) {
    let area = buf.area;
    if rows.is_empty() || area.width < WIDTH + 20 || area.height < 8 {
        return;
    }
    let shown = rows.len().min(MAX_AGENTS);
    let more = rows.len() - shown;
    let height = (shown * 2 + usize::from(more > 0) + 2) as u16;
    let height = height.min(area.height - 2);
    let rect = Rect::new(area.right() - WIDTH - 1, area.y + 1, WIDTH, height);
    let inner = (WIDTH - 4) as usize;
    let mut lines = vec![];
    for row in &rows[..shown] {
        let head = format!("{} {}", row.icon, fit(&row.name, 14));
        let tail = if row.age.is_empty() {
            row.state.clone()
        } else {
            format!("{}  {}", row.state, row.age)
        };
        let gap = inner.saturating_sub(
            unicode_width::UnicodeWidthStr::width(head.as_str())
                + unicode_width::UnicodeWidthStr::width(tail.as_str()),
        );
        lines.push(Line::from(vec![
            Span::styled(head, Style::default().fg(row.color).bold()),
            Span::raw(" ".repeat(gap.max(1))),
            Span::styled(tail, Style::default().fg(row.color)),
        ]));
        lines.push(Line::from(Span::styled(
            format!("  {}", fit(&row.detail, inner - 2)),
            Style::default().fg(MUTED),
        )));
    }
    if more > 0 {
        lines.push(Line::from(Span::styled(
            format!("+{more} more"),
            Style::default().fg(MUTED),
        )));
    }
    Clear.render(rect, buf);
    Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(MAUVE))
                .title(Line::from(" Agents · F9 ").fg(MAUVE))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        )
        .style(Style::default().bg(MANTLE).fg(TEXT))
        .render(rect, buf);
}

/// Rows kept current from the agent records, refreshed at most once a second.
pub struct Live {
    agents: crate::agents::AgentManager,
    rows: Vec<Row>,
    refreshed: Option<std::time::Instant>,
}
impl Live {
    pub fn new(agents: crate::agents::AgentManager) -> Self {
        Self {
            agents,
            rows: vec![],
            refreshed: None,
        }
    }
    pub fn draw(&mut self, buf: &mut Buffer) {
        if self
            .refreshed
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(1))
        {
            self.refreshed = Some(std::time::Instant::now());
            // A failed refresh keeps the last rows rather than blanking the box.
            if let Ok(sessions) = self.agents.list() {
                let activities = self.agents.activities(&sessions);
                self.rows = rows(&sessions, &activities, crate::usage::now());
            }
        }
        render(buf, &self.rows);
    }
}
