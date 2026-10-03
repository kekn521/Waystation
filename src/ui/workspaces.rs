use crate::{
    app::{Action, App, HitRegion},
    model::{Availability, Section},
    ui::{self, theme::*},
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    widgets::Paragraph,
};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let focus = app.section == Section::Workspaces || app.pane == 0;
    let block = ui::panel("Continue working", focus);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let parts = Layout::vertical([
        Constraint::Min(2),
        Constraint::Length(if inner.height > 7 { 4 } else { 2 }),
    ])
    .split(inner);
    let items = app
        .workspaces
        .iter()
        .map(|w| {
            let selected = app.workspace() == Some(w.id.as_path());
            let git = if selected {
                app.git
                    .value
                    .as_ref()
                    .map(|g| {
                        format!(
                            " · {} · {} changed",
                            g.branch.as_deref().unwrap_or("detached"),
                            g.changed
                        )
                    })
                    .unwrap_or_default()
            } else {
                String::new()
            };
            (
                format!(
                    "{}{}",
                    w.name,
                    if w.id.is_dir() { "" } else { " · unavailable" }
                ),
                format!("{}{git}", w.id.display()),
                Action::SelectWorkspace(w.id.clone()),
            )
        })
        .collect::<Vec<_>>();
    let mut hits = ui::row_items(frame, parts[0], &items, app.selection, focus);
    if items.is_empty() {
        frame.render_widget(
            Paragraph::new("Add project_roots or pinned_projects in config.toml").fg(MUTED),
            parts[0],
        );
    }
    let context = if let Availability::Failed(e) = &app.git.availability {
        format!(
            "Git {}",
            if app.git.value.is_some() {
                format!("stale · {e}")
            } else {
                e.clone()
            }
        )
    } else if let Some(g) = &app.git.value {
        format!(
            "{} · {} changed · ↑{} ↓{} · {} worktrees",
            g.branch.as_deref().unwrap_or("no branch"),
            g.changed,
            g.ahead,
            g.behind,
            g.worktrees.len()
        )
    } else {
        "Git context loads for the selected workspace".into()
    };
    frame.render_widget(
        Paragraph::new(format!(
            " e editor · t shell · g Git · h Herdr · f files\n {}",
            ui::safe(&context)
        ))
        .fg(MUTED),
        parts[1],
    );
    let mut x = parts[1].x;
    for (label, action) in [
        (" e editor", Action::Editor),
        (" · t shell", Action::Shell),
        (" · g Git", Action::Git),
        (" · h Herdr", Action::Herdr),
        (" · f files", Action::Files),
    ] {
        let width = (label.len() as u16).min(parts[1].right().saturating_sub(x));
        if width > 0 {
            hits.push(HitRegion {
                area: Rect::new(x, parts[1].y, width, 1),
                action,
            });
        }
        x = x.saturating_add(width);
    }
    hits
}
