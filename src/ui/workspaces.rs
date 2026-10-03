use crate::{
    app::{Action, App, HitRegion},
    ui::{self, theme::*},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    widgets::Paragraph,
};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let parts = Layout::vertical([Constraint::Min(4), Constraint::Length(4)]).split(area);
    let items = app
        .workspaces
        .iter()
        .map(|w| {
            (
                w.name.clone(),
                w.id.display().to_string(),
                Action::SelectWorkspace(w.id.clone()),
            )
        })
        .collect::<Vec<_>>();
    let mut hits = ui::rows(
        frame,
        parts[0],
        "Continue working",
        &items,
        app.selection,
        true,
    );
    let context = if let Some(g) = &app.git.value {
        format!(
            "{} · {} changed · ↑{} ↓{}",
            g.branch.as_deref().unwrap_or("no branch"),
            g.changed,
            g.ahead,
            g.behind
        )
    } else {
        match &app.git.availability {
            crate::model::Availability::Failed(e) => e.clone(),
            _ => "Select a workspace to inspect Git".into(),
        }
    };
    frame.render_widget(
        Paragraph::new(format!(
            " e editor · t shell · g Git · h Herdr · f files\n {}",
            ui::safe(&context)
        ))
        .fg(MUTED),
        parts[1],
    );
    for (i, a) in [
        Action::Editor,
        Action::Shell,
        Action::Git,
        Action::Herdr,
        Action::Files,
    ]
    .into_iter()
    .enumerate()
    {
        hits.push(HitRegion {
            area: Rect::new(
                parts[1].x + i as u16 * 11,
                parts[1].y,
                11.min(parts[1].width.saturating_sub(i as u16 * 11)),
                1,
            ),
            action: a,
        })
    }
    hits
}
