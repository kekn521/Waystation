use crate::{
    app::{Action, App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let mut items = app
        .files
        .iter()
        .map(|f| {
            (
                format!("{} {}", if f.is_dir { "▸" } else { " " }, f.label),
                if f.is_dir {
                    "directory".into()
                } else {
                    "Enter opens in editor".into()
                },
                Action::OpenPath(f.path.clone()),
            )
        })
        .collect::<Vec<_>>();
    if let Some(e) = app.provider_errors.get("Files") {
        items.insert(
            0,
            (
                "Files unavailable · F5 retries".into(),
                e.clone(),
                Action::Reload,
            ),
        );
    }
    ui::rows(
        frame,
        area,
        &format!(
            "Files / {}",
            app.file_dir
                .as_deref()
                .map_or(String::new(), |p| p.display().to_string())
        ),
        &items,
        app.selection,
        true,
    )
}
