use crate::{
    app::{Action, App, HitRegion},
    ui::{self, theme::*},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    widgets::{Paragraph, Wrap},
};
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let items = app.task_items();
    let title = if app.recipes {
        "Recipes · Enter starts · n history"
    } else {
        "Tasks & recent runs · n recipes"
    };
    if items.is_empty() {
        let text = if app.recipes {
            "No recipes configured.\n\nAdd a [[tasks]] entry in ~/.config/station/config.toml:\nid = \"check\"\nlabel = \"Cargo check\"\ncommand = { program = \"cargo\", args = [\"check\"] }"
        } else {
            "No task runs yet.\n\nPress n to choose a recipe.\nTasks keep their logs and completion status after Station closes."
        };
        frame.render_widget(
            Paragraph::new(text)
                .fg(MUTED)
                .wrap(Wrap { trim: true })
                .block(ui::panel(
                    title,
                    app.pane == 2 || app.section == crate::model::Section::Tasks,
                )),
            area,
        );
        return vec![HitRegion {
            area,
            action: Action::Recipes,
        }];
    }
    ui::rows(
        frame,
        area,
        title,
        &items,
        app.selection,
        app.pane == 2 || app.section == crate::model::Section::Tasks,
    )
}
