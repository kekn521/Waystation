use crate::{
    app::{Action, App, HitRegion},
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
    let block = panel("New agent session", true);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    let tool = format!("‹ {} ›", form.tool_name());
    let workspace = form
        .workspace()
        .map(|p| format!("‹ {} ›", p.display()))
        .unwrap_or("Choose a workspace first".into());
    let labels = ["NAME", "AGENT · ←/→ choose", "PROJECT · ←/→ choose"];
    for (i, value) in [&form.name, &tool, &workspace].into_iter().enumerate() {
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
        if focused && i == 0 {
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
            text = "e.g. Implementation / Review".into();
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
        } else {
            "[ Create and open ]"
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
        let hint = form.error.as_deref().unwrap_or(
            "F12 or Ctrl-\\ returns here. Your session keeps running when Waystation closes.",
        );
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
