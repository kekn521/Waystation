use crate::{
    app::{Action, App, HitRegion},
    model::Availability,
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn items(app: &App) -> Vec<(String, String, Action)> {
    let mut rows = vec![];
    for (name, availability, stale) in [
        (
            "Docker",
            &app.services.containers.availability,
            app.services.containers.value.is_some(),
        ),
        (
            "Listeners",
            &app.services.listeners.availability,
            app.services.listeners.value.is_some(),
        ),
    ] {
        if let Availability::Failed(e) = availability
            && stale
        {
            rows.push((
                format!("{name} stale · F5 retries"),
                e.clone(),
                Action::Reload,
            ));
        }
    }

    if let Some(cs) = &app.services.containers.value {
        for c in cs {
            rows.push((
                format!("▣ {} · {}", c.name, c.state),
                c.ports.clone(),
                Action::DockerLogs(c.id.clone()),
            ));
        }
    } else if let Availability::Failed(e) = &app.services.containers.availability {
        rows.push(("Docker unavailable".into(), e.clone(), Action::Reload));
    }
    if let Some(ls) = &app.services.listeners.value {
        for l in ls {
            let label = format!("{}  {}", l.protocol.to_uppercase(), l.address);
            let detail = l
                .command
                .as_deref()
                .unwrap_or("Process details unavailable")
                .to_string();
            let action = l
                .cwd
                .clone()
                .map(Action::SelectWorkspace)
                .unwrap_or(Action::ShowText(format!("{label} · {detail}")));
            rows.push((label, detail, action));
        }
    } else if let Availability::Failed(e) = &app.services.listeners.availability {
        rows.push(("Listeners unavailable".into(), e.clone(), Action::Reload));
    }
    rows
}
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    ui::rows(
        frame,
        area,
        "Services & ports · read only",
        &items(app),
        app.selection,
        app.section == crate::model::Section::Services
            || (app.section == crate::model::Section::Overview && app.pane == 3),
    )
}
