use crate::{
    app::{App, HitRegion},
    ui,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Stylize,
    widgets::Paragraph,
};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let parts = Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).split(area);
    let hits = ui::rows(
        frame,
        parts[0],
        "Connections · saved destinations",
        &app.connection_items(),
        app.selection,
        true,
    );
    frame.render_widget(Paragraph::new(" SSH handles authentication. Destinations are not probed.\n Conditional Match configuration is evaluated by SSH at connect time.").fg(ui::theme::MUTED),parts[1]);
    hits
}
