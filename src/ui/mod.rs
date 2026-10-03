pub mod layout;
pub mod theme;
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
pub fn panel(title: &str, focused: bool) -> Block<'_> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(if focused { MAUVE } else { LINE }))
        .title(Line::from(format!(" {title} ")).fg(if focused { MAUVE } else { MUTED }))
        .style(Style::default().bg(BASE).fg(TEXT))
}
pub fn draw(frame: &mut Frame, app: &App) -> Vec<HitRegion> {
    let area = frame.area();
    let mut hits = vec![];
    frame.render_widget(
        Block::default().style(Style::default().bg(BASE).fg(TEXT)),
        area,
    );
    let mode = layout::mode(area.width, area.height);
    if mode == layout::LayoutMode::TooSmall {
        frame.render_widget(
            Paragraph::new("Resize to at least 60 × 18\nq quit").fg(MAUVE),
            area,
        );
        return hits;
    }
    let vertical = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(8),
        Constraint::Length(2),
    ])
    .split(area);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(" ╭─┬─╮  STATION", Style::default().fg(MAUVE).bold()),
                Span::styled(
                    "   dispatch / your terminal, connected",
                    Style::default().fg(MUTED),
                ),
            ]),
            Line::from(" ╰─┼─╯  local workspace").fg(MUTED),
        ])
        .block(
            Block::default()
                .borders(Borders::BOTTOM)
                .border_style(Style::default().fg(LINE)),
        )
        .bg(MANTLE),
        vertical[0],
    );
    let (content, navarea) = if mode == layout::LayoutMode::Wide {
        let cols =
            Layout::horizontal([Constraint::Length(17), Constraint::Min(40)]).split(vertical[1]);
        (cols[1], cols[0])
    } else {
        let rows = Layout::vertical([Constraint::Length(3), Constraint::Min(5)]).split(vertical[1]);
        (rows[1], rows[0])
    };
    if mode == layout::LayoutMode::Wide {
        frame.render_widget(Block::default().bg(MANTLE), navarea);
        for (i, s) in Section::ALL.iter().enumerate() {
            let row = Rect::new(
                navarea.x + 1,
                navarea.y + 1 + i as u16 * 2,
                navarea.width - 2,
                1,
            );
            let style = if *s == app.section {
                Style::default().bg(MAUVE).fg(CRUST)
            } else {
                Style::default().fg(MUTED)
            };
            frame.render_widget(
                Paragraph::new(format!(" {} {}", i + 1, s.name())).style(style),
                row,
            );
            hits.push(HitRegion {
                area: row,
                action: Action::Nav(*s),
            });
        }
    } else {
        let text = Section::ALL
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Span::styled(
                    format!(" {} {} ", i + 1, s.name()),
                    if *s == app.section {
                        Style::default().bg(MAUVE).fg(CRUST)
                    } else {
                        Style::default().fg(MUTED)
                    },
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(Line::from(text)).wrap(Wrap { trim: false }),
            navarea,
        );
    }
    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(if app.message.is_some() { 2 } else { 0 }),
    ])
    .margin(1)
    .split(content);
    frame.render_widget(
        Paragraph::new(if app.searching {
            format!(" / {}▏", app.query)
        } else {
            " / Jump to anything…".into()
        })
        .block(panel("commands", app.searching))
        .fg(if app.searching { MAUVE } else { MUTED }),
        rows[0],
    );
    hits.push(HitRegion {
        area: rows[0],
        action: Action::Search,
    });
    if app.section == Section::Overview && mode != layout::LayoutMode::Single {
        let cols = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
            .split(rows[1]);
        for (c, col) in cols.iter().enumerate() {
            let panes = Layout::vertical([Constraint::Percentage(58), Constraint::Percentage(42)])
                .split(*col);
            for (r, pane) in panes.iter().enumerate() {
                let idx = c + r * 2;
                let title = [
                    "Continue working",
                    "Machine pulse",
                    "Tasks & recent runs",
                    "Services & ports",
                ][idx];
                frame.render_widget(
                    Paragraph::new("\n  Loading local data…")
                        .fg(MUTED)
                        .block(panel(title, idx == app.pane)),
                    *pane,
                );
            }
        }
    } else {
        frame.render_widget(
            Paragraph::new("\n  No data yet. Add a project or task in your configuration.")
                .fg(MUTED)
                .wrap(Wrap { trim: true })
                .block(panel(app.section.name(), true)),
            rows[1],
        );
    }
    if let Some(m) = &app.message {
        frame.render_widget(
            Paragraph::new(m.as_str())
                .fg(TEAL)
                .wrap(Wrap { trim: true }),
            rows[2],
        );
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" NORMAL ", Style::default().bg(MAUVE).fg(CRUST)),
            Span::raw("  j/k move · Tab pane · Enter open · / commands · ? help · q quit"),
        ]))
        .block(
            Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(LINE)),
        )
        .bg(CRUST),
        vertical[2],
    );
    if app.help {
        let rect = Rect::new(
            area.x + 3,
            area.y + 5,
            area.width - 6,
            12.min(area.height - 6),
        );
        frame.render_widget(Clear, rect);
        frame.render_widget(Paragraph::new("1–9 sections · j/k or arrows move · Tab focus pane\n/ search commands · Enter activate · Esc close\ne editor · t shell · g Git · h Herdr · f files\nn task recipes · r rerun · x stop owned task\n. hidden files · y copy path · F5 refresh\nq quit (asks what to do with running tasks)").block(panel("Keyboard shortcuts",true)).wrap(Wrap{trim:true}),rect);
    }
    hits
}
