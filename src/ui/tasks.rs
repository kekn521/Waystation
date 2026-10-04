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
        "Saved tasks · Enter runs · n history · a add"
    } else {
        "Tasks & recent runs · n saved tasks · a add"
    };
    let button = Rect::new(
        area.right().saturating_sub(15).max(area.x),
        area.y,
        area.width.min(14),
        1,
    );
    let add_button = |frame: &mut Frame| {
        frame.render_widget(Paragraph::new(" ＋ Add task ").fg(CRUST).bg(MAUVE), button);
        HitRegion {
            area: button,
            action: Action::NewRecipe,
        }
    };
    if items.is_empty() {
        let text = if app.recipes {
            "No saved tasks yet.\n\nPress a to add a task: give it a name, a command, and a project.\nExample: Test suite → cargo test\nThen Enter runs it; n switches to run history and logs."
        } else {
            "No task runs yet.\n\nPress a to add a task, or n to choose a saved task.\nTasks keep their logs and completion status after Station closes."
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
        return vec![
            HitRegion {
                area,
                action: Action::NewRecipe,
            },
            add_button(frame),
        ];
    }
    let mut hits = ui::rows(
        frame,
        area,
        title,
        &items,
        app.selection,
        app.pane == 2 || app.section == crate::model::Section::Tasks,
    );
    hits.push(add_button(frame));
    hits
}
