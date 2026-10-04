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
    let width = 78.min(area.width.saturating_sub(2));
    let height = 17.min(area.height);
    let rect = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, rect);
    let title = if form.kind == FormKind::Agent {
        "New agent session"
    } else {
        "Add task · save a reusable command"
    };
    let block = panel(title, true);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let tool = format!("‹ {} ›", form.tool_name());
    let workspace = form
        .workspace()
        .map(|p| format!("‹ {} ›", p.display()))
        .unwrap_or("Choose a workspace first".into());
    let command = if form.kind == FormKind::Agent {
        &tool
    } else {
        &form.command
    };
    let labels = [
        "NAME",
        if form.kind == FormKind::Agent {
            "AGENT · ←/→ choose"
        } else {
            "COMMAND · quotes keep arguments together"
        },
        "PROJECT · ←/→ choose",
    ];
    for (i, value) in [&form.name, command, &workspace].into_iter().enumerate() {
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
        if focused && (i == 0 || i == 1 && form.kind == FormKind::Task) {
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
            text = if i == 0 {
                "e.g. Implementation / Test suite"
            } else {
                "e.g. cargo test / npm run dev"
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
    if inner.height >= 11 {
        hits.push(HitRegion {
            area: Rect::new(inner.x, inner.y + 9, inner.width, 1),
            action: Action::FormSave,
        });
        let button = if form.busy {
            "Working…"
        } else if form.kind == FormKind::Agent {
            "[ Create and open ]"
        } else {
            "[ Save task ]"
        };
        frame.render_widget(
            Paragraph::new(button)
                .fg(if form.field == 3 { CRUST } else { MAUVE })
                .bg(if form.field == 3 { MAUVE } else { BASE }),
            Rect::new(inner.x + 1, inner.y + 9, inner.width.saturating_sub(2), 1),
        );
        frame.render_widget(
            Paragraph::new("Tab next · Ctrl+S save · Ctrl+U clear · Esc cancel").fg(MUTED),
            Rect::new(inner.x + 1, inner.y + 10, inner.width.saturating_sub(2), 1),
        );
    }
    if inner.height > 12 {
        let hint = form.error.as_deref().unwrap_or(if form.kind == FormKind::Agent { "F12 returns here. Your session keeps running when Station closes." } else { "Saved tasks run in this project. Use an explicit shell script for pipes or environment expansion." });
        frame.render_widget(
            Paragraph::new(safe(hint))
                .fg(if form.error.is_some() { RED } else { TEAL })
                .wrap(Wrap { trim: true }),
            Rect::new(
                inner.x + 1,
                inner.y + 12,
                inner.width.saturating_sub(2),
                inner.height - 12,
            ),
        );
    }
    hits
}
