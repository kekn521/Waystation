use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{Frame, layout::Rect};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let usage = ui::usage::height(app, area.height);
    let (top, area) = (
        Rect {
            height: usage,
            ..area
        },
        Rect {
            y: area.y + usage,
            height: area.height - usage,
            ..area
        },
    );
    if usage > 0 {
        ui::usage::render(frame, top, app);
    }
    ui::rows(
        frame,
        area,
        "Agents · n new · Enter open · r restart · x close",
        &app.agent_items(),
        app.selection,
        true,
    )
}
