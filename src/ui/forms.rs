use crate::{
    app::{Action, App, HitRegion},
    forms::FormKind,
    ui::{panel, safe, theme::*},
};
use ratatui::{
    Frame,
    layout::Rect,
    style::Stylize,
    widgets::{Clear, Paragraph, Wrap},
};
pub fn render(frame: &mut Frame, app: &App) -> Vec<HitRegion> {
    let mut hits = vec![];
    let Some(form) = &app.form else {
        return hits;
    };
    let area = frame.area();
    let agent = form.kind == FormKind::Agent;
    // Text and choice fields take three rows each; the button row follows them.
    let button_y = (form.fields() as u16 - 1) * 3;
    let width = 78.min(area.width.saturating_sub(2));
    let height = (button_y + 8).min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let block = panel(
        match form.kind {
            FormKind::Agent => "New agent session",
            FormKind::Workspace => "New workspace",
            FormKind::AddFolder => "Add existing folder",
        },
        true,
    );
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let tool = format!("‹ {} ›", form.tool_name());
    let workspace = form
        .workspace()
        .map(|p| format!("‹ {} ›", p.display()))
        .unwrap_or("Choose a workspace first".into());
    let (labels, values): (&[&str], Vec<&String>) = if agent {
        (
            &["NAME", "AGENT · ←/→ choose", "PROJECT · ←/→ choose"],
            vec![&form.name, &tool, &workspace],
        )
    } else if form.kind == FormKind::AddFolder {
        (&["FOLDER · type a path"], vec![&form.location])
    } else if form.projects.len() > 1 {
        (
            &["DIRECTORY NAME", "LOCATION · type a path · Ctrl+←/→ roots"],
            vec![&form.name, &form.location],
        )
    } else {
        (
            &["DIRECTORY NAME", "LOCATION · type a path"],
            vec![&form.name, &form.location],
        )
    };
    for (i, value) in values.into_iter().enumerate() {
        let y = inner.y + i as u16 * 3;
        if y + 1 >= inner.bottom() {
            break;
        }
        hits.push(HitRegion {
            area: Rect::new(inner.x, y, inner.width, 2),
            action: Action::FormFocus(i),
        });
        let focused = form.field == i;
        frame.render_widget(
            Paragraph::new(labels[i]).fg(if focused { TEAL } else { MUTED }),
            Rect::new(inner.x + 1, y, inner.width.saturating_sub(2), 1),
        );
        let mut text = safe(value);
        let mut scroll = 0;
        // The name, and a workspace's location, are typed.
        if focused && (i == 0 || !agent) {
            let index = text
                .char_indices()
                .nth(form.cursor)
                .map(|(i, _)| i)
                .unwrap_or(text.len());
            scroll = unicode_width::UnicodeWidthStr::width(&text[..index])
                .saturating_sub(inner.width.saturating_sub(5) as usize) as u16;
            text.insert(index, '▏');
        }
        if text.is_empty() {
            text = match (form.kind, i) {
                (FormKind::Agent, _) => "e.g. Implementation / Review",
                (FormKind::AddFolder, _) => "e.g. ~/code/my-app",
                (FormKind::Workspace, 0) => "e.g. my-new-project",
                (FormKind::Workspace, _) => "e.g. ~/code",
            }
            .into();
        }
        frame.render_widget(
            Paragraph::new(text)
                .scroll((0, scroll))
                .fg(if focused { TEXT } else { MUTED })
                .bg(if focused { SELECT } else { MANTLE }),
            Rect::new(inner.x + 1, y + 1, inner.width.saturating_sub(2), 1),
        );
    }
    if inner.height >= button_y + 2 {
        hits.push(HitRegion {
            area: Rect::new(inner.x, inner.y + button_y, inner.width, 1),
            action: Action::FormSave,
        });
        let button = if form.busy {
            "Working…"
        } else {
            match form.kind {
                FormKind::Agent => "[ Create and open ]",
                FormKind::Workspace => "[ Create workspace ]",
                FormKind::AddFolder => "[ Add folder ]",
            }
        };
        let on_button = form.field == form.fields() - 1;
        frame.render_widget(
            Paragraph::new(button)
                .fg(if on_button { CRUST } else { MAUVE })
                .bg(if on_button { MAUVE } else { BASE }),
            Rect::new(
                inner.x + 1,
                inner.y + button_y,
                inner.width.saturating_sub(2),
                1,
            ),
        );
        frame.render_widget(
            Paragraph::new("Tab next · Ctrl+S save · Ctrl+U clear · Esc cancel").fg(MUTED),
            Rect::new(
                inner.x + 1,
                inner.y + button_y + 1,
                inner.width.saturating_sub(2),
                1,
            ),
        );
    }
    if inner.height > button_y + 3 {
        let hint = form.error.as_deref().unwrap_or(match form.kind {
            FormKind::Agent => {
                "F12 or Ctrl-\\ returns here. Your session keeps running when Waystation closes."
            }
            FormKind::Workspace => {
                "Makes the directory and runs git init in it. Outside your project roots it is also pinned in config.toml."
            }
            FormKind::AddFolder => {
                "Any folder works, Git or not. Waystation pins it in config.toml unless the scan already finds it."
            }
        });
        frame.render_widget(
            Paragraph::new(safe(hint))
                .fg(if form.error.is_some() { RED } else { TEAL })
                .wrap(Wrap { trim: true }),
            Rect::new(
                inner.x + 1,
                inner.y + button_y + 3,
                inner.width.saturating_sub(2),
                inner.height - button_y - 3,
            ),
        );
    }
    hits
}
