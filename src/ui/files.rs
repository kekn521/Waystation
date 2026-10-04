use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let items = app.file_items();
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
