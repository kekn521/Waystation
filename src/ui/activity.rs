use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    ui::rows(
        frame,
        area,
        "Activity · Station launches & task outcomes",
        &app.activity_items(),
        app.selection,
        true,
    )
}
