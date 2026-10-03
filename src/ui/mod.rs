pub mod activity;
pub mod agents;
pub mod connections;
pub mod files;
pub mod layout;
pub mod services;
pub mod system;
pub mod tasks;
pub mod theme;
pub mod workspaces;
use crate::{
    app::{Action, App, HitRegion},
    model::Section,
};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap},
};
use theme::*;
pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                '�'
            } else {
                c
            }
        })
        .collect()
}
pub fn rows(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: &[(String, String, Action)],
    selected: usize,
    focused: bool,
) -> Vec<HitRegion> {
    let mut hits = vec![];
    let block = panel(title, focused);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    hits.extend(row_items(frame, inner, items, selected, focused));
    hits
}
pub fn row_items(
    frame: &mut Frame,
    inner: Rect,
    items: &[(String, String, Action)],
    selected: usize,
    focused: bool,
) -> Vec<HitRegion> {
    let mut hits = vec![];
    if items.is_empty() {
        frame.render_widget(Paragraph::new("\n Nothing here yet.").fg(MUTED), inner);
        return hits;
    }
    let capacity = (inner.height / 2).max(1) as usize;
    let selected = selected.min(items.len() - 1);
    let offset = selected.saturating_sub(capacity - 1);
    for (i, (label, detail, action)) in items.iter().enumerate().skip(offset).take(capacity) {
        let row = Rect::new(
            inner.x,
            inner.y + ((i - offset) * 2) as u16,
            inner.width,
            2.min(inner.height),
        );
        let style = if i == selected && focused {
            Style::default().bg(SELECT).fg(MAUVE)
        } else {
            Style::default().fg(TEXT)
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(format!(
                    " {} {}",
                    if i == selected { "❯" } else { "·" },
                    safe(label).replace(['\n', '\t'], " ")
                )),
                Line::from(format!("   {}", safe(detail))).fg(MUTED),
            ])
            .style(style),
            row,
        );
        hits.push(HitRegion {
            area: row,
            action: action.clone(),
        });
    }
    hits
}
pub fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { MAUVE } else { LINE }))
        .title(Line::from(format!(" {} ", safe(title))).fg(if focused { MAUVE } else { MUTED }))
        .style(Style::default().bg(BASE).fg(TEXT))
}
pub fn draw(frame: &mut Frame, app: &App) -> Vec<HitRegion> {
    let area = frame.area();
    let mut hits = vec![];
    frame.render_widget(Block::default().bg(BASE).fg(TEXT), area);
    let mut mode = layout::mode(area.width, area.height);
    if app.config.theme.compact && mode == layout::LayoutMode::Wide {
        mode = layout::LayoutMode::Compact;
    }
    if mode == layout::LayoutMode::TooSmall {
        frame.render_widget(
            Paragraph::new("Resize to at least 60 × 18\nq quit").fg(MAUVE),
            area,
        );
        return hits;
    }
    let narrow = mode == layout::LayoutMode::Single;
    let vertical = Layout::vertical([
        Constraint::Length(if narrow { 3 } else { 4 }),
        Constraint::Min(5),
        Constraint::Length(2),
    ])
    .split(area);
    let context = app
        .workspace()
        .map(|p| {
            p.file_name()
                .unwrap_or(p.as_os_str())
                .to_string_lossy()
                .into_owned()
        })
        .unwrap_or("select a workspace".into());
    let header = vec![
        Line::from(vec![
            Span::styled(" ╭─┬─╮  S T A T I O N", Style::default().fg(MAUVE).bold()),
            Span::styled(
                if narrow {
                    ""
                } else {
                    "     dispatch / your terminal, connected"
                },
                Style::default().fg(MUTED),
            ),
        ]),
        Line::from(vec![
            Span::styled(" ╰─┼─╯  ", Style::default().fg(MAUVE)),
            Span::styled(safe(&context), Style::default().fg(TEAL)),
            Span::styled(
                format!("  · {}", app.section.name()),
                Style::default().fg(MUTED),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(header).bg(MANTLE).block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(LINE)),
        ),
        vertical[0],
    );
    if !narrow && area.width > 100 {
        frame.render_widget(
            Paragraph::new("● LOCAL  /  MACCHIATO").fg(TEAL).bg(MANTLE),
            Rect::new(area.right() - 24, area.y + 2, 23, 1),
        );
    }
    let (content, nav) = if mode == layout::LayoutMode::Wide {
        let c =
            Layout::horizontal([Constraint::Length(18), Constraint::Min(40)]).split(vertical[1]);
        (c[1], c[0])
    } else {
        let r = Layout::vertical([Constraint::Length(2), Constraint::Min(3)]).split(vertical[1]);
        (r[1], r[0])
    };
    frame.render_widget(Block::default().bg(MANTLE), nav);
    if mode == layout::LayoutMode::Wide {
        for (i, section) in Section::ALL.iter().enumerate() {
            let group = i / 3;
            let y = nav.y + 2 + i as u16 * 2 + group as u16 * 2;
            if i % 3 == 0 {
                frame.render_widget(
                    Paragraph::new([" WORK", " OPERATE", " EXPLORE"][group]).fg(BLUE),
                    Rect::new(nav.x + 1, y - 1, nav.width - 2, 1),
                );
            }
            let row = Rect::new(nav.x + 1, y, nav.width - 2, 1);
            let style = if *section == app.section {
                Style::default().bg(MAUVE).fg(CRUST).bold()
            } else {
                Style::default().fg(MUTED)
            };
            frame.render_widget(
                Paragraph::new(format!(" {} {}", i + 1, section.name())).style(style),
                row,
            );
            hits.push(HitRegion {
                area: row,
                action: Action::Nav(*section),
            });
        }
        if nav.height > 28 {
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(vec![
                        Span::styled(" ▰ ", Style::default().fg(MAUVE)),
                        Span::styled("▰ ", Style::default().fg(BLUE)),
                        Span::styled("▰ ", Style::default().fg(TEAL)),
                        Span::styled("▰", Style::default().fg(PEACH)),
                    ]),
                    Line::from(" Catppuccin"),
                    Line::from(" Macchiato"),
                ])
                .fg(MUTED),
                Rect::new(nav.x + 1, nav.bottom() - 4, nav.width - 2, 3),
            );
        }
    } else {
        let labels = if narrow {
            [
                "Hub", "Work", "AI", "Task", "Svc", "SSH", "File", "Sys", "Log",
            ]
        } else {
            [
                "Overview",
                "Workspaces",
                "Agents",
                "Tasks",
                "Services",
                "Connections",
                "Files",
                "System",
                "Activity",
            ]
        };
        let mut x = nav.x;
        let mut y = nav.y;
        for (i, label) in labels.iter().enumerate() {
            let text = format!("{} {} ", i + 1, label);
            let width = unicode_width::UnicodeWidthStr::width(text.as_str()) as u16;
            if x + width > nav.right() {
                x = nav.x;
                y += 1;
            }
            if y >= nav.bottom() {
                break;
            }
            let row = Rect::new(x, y, width, 1);
            let style = if Section::ALL[i] == app.section {
                Style::default().bg(MAUVE).fg(CRUST)
            } else {
                Style::default().fg(MUTED)
            };
            frame.render_widget(Paragraph::new(text).style(style), row);
            hits.push(HitRegion {
                area: row,
                action: Action::Nav(Section::ALL[i]),
            });
            x += width;
        }
    }
    let parts = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(if narrow { 0 } else { 1 }),
        Constraint::Min(3),
        Constraint::Length(if app.message.is_some() { 1 } else { 0 }),
    ])
    .horizontal_margin(if narrow { 0 } else { 1 })
    .split(content);
    let prompt = if app.searching {
        format!(" / {}▏", safe(&app.query))
    } else {
        " / Jump to anything…".into()
    };
    frame.render_widget(
        Paragraph::new(prompt)
            .fg(if app.searching { MAUVE } else { MUTED })
            .block(panel("commands", app.searching)),
        parts[0],
    );
    hits.push(HitRegion {
        area: parts[0],
        action: Action::Search,
    });
    let failures = app
        .runs
        .iter()
        .take(20)
        .filter(|r| r.status == crate::tasks::RunStatus::Failed)
        .count();
    let attention = if failures > 0 {
        format!(" ! {failures} failed runs · open Tasks for logs")
    } else {
        format!(
            " {} workspaces  ·  {} task runs  ·  local observations",
            app.workspaces.len(),
            app.runs.len()
        )
    };
    frame.render_widget(
        Paragraph::new(attention).fg(if failures > 0 { PEACH } else { MUTED }),
        parts[1],
    );
    if failures > 0 {
        hits.push(HitRegion {
            area: parts[1],
            action: Action::Nav(Section::Tasks),
        });
    }
    if app.searching {
        let items = app
            .matches()
            .into_iter()
            .map(|i| (i.label, i.detail, i.action))
            .collect::<Vec<_>>();
        hits.extend(rows(
            frame,
            parts[2],
            "Jump to anything",
            &items,
            app.selection,
            true,
        ));
    } else if app.section == Section::Overview {
        if narrow {
            hits.extend(render_pane(frame, parts[2], app, app.pane));
        } else {
            let cols = Layout::horizontal([Constraint::Percentage(61), Constraint::Percentage(39)])
                .spacing(1)
                .split(parts[2]);
            for (c, col) in cols.iter().enumerate() {
                let panes =
                    Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
                        .spacing(1)
                        .split(*col);
                for (r, pane) in panes.iter().enumerate() {
                    hits.extend(render_pane(frame, *pane, app, c + r * 2));
                }
            }
        }
    } else {
        hits.extend(match app.section {
            Section::Workspaces => workspaces::render(frame, parts[2], app),
            Section::Agents => agents::render(frame, parts[2], app),
            Section::Tasks => tasks::render(frame, parts[2], app),
            Section::Services => services::render(frame, parts[2], app),
            Section::Connections => connections::render(frame, parts[2], app),
            Section::Files => files::render(frame, parts[2], app),
            Section::System => system::render(frame, parts[2], app),
            Section::Activity => activity::render(frame, parts[2], app),
            Section::Overview => vec![],
        });
    }
    if let Some(message) = &app.message {
        frame.render_widget(Paragraph::new(safe(message)).fg(TEAL), parts[3]);
    }
    let footer = if narrow {
        " j/k move · Tab pane · / search · ? help · q quit"
    } else if app.section == Section::Tasks {
        " j/k move · Enter logs · n recipes · r rerun · x stop · q quit"
    } else {
        " j/k move · e editor · t shell · g Git · h Herdr · f files · / commands · ? help · q quit"
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                if app.searching {
                    " SEARCH "
                } else {
                    " NORMAL "
                },
                Style::default().bg(MAUVE).fg(CRUST),
            ),
            Span::raw(footer),
        ]))
        .bg(CRUST)
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(LINE)),
        ),
        vertical[2],
    );
    if let Some((title, text)) = &app.detail {
        let rect = Rect::new(area.x + 2, area.y + 2, area.width - 4, area.height - 4);
        frame.render_widget(Clear, rect);
        frame.render_widget(
            Paragraph::new(safe(text))
                .scroll((app.detail_scroll, 0))
                .block(panel(title, true))
                .wrap(Wrap { trim: false }),
            rect,
        );
        hits.clear();
    }
    if let Some(confirm) = &app.confirmation {
        let rect = Rect::new(
            area.x + 3,
            area.y + area.height / 3,
            area.width - 6,
            (confirm.choices.len() as u16 * 2 + 4).min(area.height - 4),
        );
        frame.render_widget(Clear, rect);
        let choices = confirm
            .choices
            .iter()
            .enumerate()
            .map(|(i, (label, _))| (label.clone(), String::new(), Action::ConfirmChoice(i)))
            .collect::<Vec<_>>();
        hits = rows(
            frame,
            rect,
            &safe(&confirm.title),
            &choices,
            app.modal_selection,
            true,
        );
    }
    if app.help {
        let rect = Rect::new(
            area.x + 2,
            area.y + 3,
            area.width - 4,
            12.min(area.height - 4),
        );
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new("1–9 sections · j/k or arrows move · Tab focus pane\n/ search commands · Enter activate · Esc close\ne editor · t shell · g Git · h Herdr · f files\nn task recipes · r rerun · x stop owned task\n. hidden files · y copy path · F5 refresh\nq quit · keep running / stop owned tasks / cancel").block(panel("Keyboard shortcuts · Esc close",true)).wrap(Wrap{trim:true}),rect);
        hits.clear();
    }
    if app.config.theme.accent == "blue" {
        for cell in &mut frame.buffer_mut().content {
            if cell.fg == MAUVE {
                cell.fg = BLUE
            }
            if cell.bg == MAUVE {
                cell.bg = BLUE
            }
        }
    }
    hits
}
fn render_pane(frame: &mut Frame, area: Rect, app: &App, index: usize) -> Vec<HitRegion> {
    match index {
        0 => workspaces::render(frame, area, app),
        1 => system::render(frame, area, app),
        2 => tasks::render(frame, area, app),
        _ => services::render(frame, area, app),
    }
}
