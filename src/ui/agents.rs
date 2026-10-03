use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    ui::rows(
        frame,
        area,
        "Agents · open in selected workspace",
        &app.agent_items(),
        app.selection,
        true,
    )
}
