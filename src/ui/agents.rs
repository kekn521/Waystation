use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    ui::rows(
        frame,
        area,
        "Agents · n new · Enter open · x close",
        &app.agent_items(),
        app.selection,
        true,
    )
}
