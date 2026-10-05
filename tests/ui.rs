use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use waystation::{
    app::{Action, App},
    config::Config,
    input,
    model::{AppState, Section},
};
fn key(c: char) -> Event {
    Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
}
#[test]
fn text_entry_does_not_activate_shortcuts() {
    let mut app = App::new(Config::default(), AppState::default());
    app.update(Action::Search);
    for c in "q3h".chars() {
        let a = input::translate(key(c), &app).unwrap();
        app.update(a);
    }
    assert_eq!(app.query, "q3h");
    assert_eq!(app.section, Section::Overview);
    assert!(app.searching);
    app.update(Action::Escape);
    assert!(!app.searching);
}
#[test]
fn keys_switch_sections_and_help() {
    let mut app = App::new(Config::default(), AppState::default());
    app.update(input::translate(key('7'), &app).unwrap());
    assert_eq!(app.section, Section::System);
    app.update(input::translate(key('8'), &app).unwrap());
    assert_eq!(app.section, Section::Activity);
    // There is no 9th section and the removed task keys do nothing.
    assert!(input::translate(key('9'), &app).is_none());
    for c in ['a', 'n', 'r'] {
        assert!(
            input::translate(key(c), &app).is_none(),
            "key {c} must be inert"
        );
    }
    app.update(input::translate(key('3'), &app).unwrap());
    assert_eq!(app.section, Section::Agents);
    assert!(matches!(
        input::translate(key('n'), &app),
        Some(Action::NewAgent)
    ));
    app.update(Action::Help);
    assert!(app.help);
    app.update(Action::Escape);
    assert!(!app.help);
}
#[test]
fn shell_is_usable_at_each_terminal_size() {
    for (w, h) in [
        (140, 45),
        (120, 38),
        (100, 32),
        (89, 30),
        (80, 24),
        (60, 18),
        (59, 17),
    ] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        let app = App::new(Config::default(), AppState::default());
        terminal
            .draw(|f| {
                waystation::ui::draw(f, &app);
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect::<String>();
        assert!(
            text.contains(if w < 60 {
                "Resize"
            } else {
                "W A Y S T A T I O N"
            }),
            "missing usable shell at {w}x{h}"
        );
    }
}
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        waystation::ui::draw(f, app);
    })
    .unwrap();
    t.backend()
        .buffer()
        .content
        .chunks(w as usize)
        .map(|row| {
            row.iter()
                .map(|c| c.symbol())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn narrow_overview_uses_focused_pane() {
    let mut a = App::new(Config::default(), AppState::default());
    a.pane = 2;
    assert!(screen(&a, 60, 18).contains("Machine pulse"));
}
#[test]
fn complete_snapshots() {
    for (w, h) in [
        (140, 45),
        (120, 38),
        (100, 32),
        (89, 30),
        (80, 24),
        (60, 18),
        (59, 17),
    ] {
        let mut a = App::new(Config::default(), AppState::default());
        a.workspaces = vec![waystation::providers::projects::Workspace {
            id: "/projects/機器-learning-with-a-long-name".into(),
            name: "機器-learning-with-a-long-name".into(),
        }];
        a.state.selected_workspace = Some(a.workspaces[0].id.clone());
        a.provider_errors
            .insert("System".into(), "permission denied".into());
        let mut snapshots = vec![];
        for section in Section::ALL {
            a.section = section;
            snapshots.push(format!("--- {} ---\n{}", section.name(), screen(&a, w, h)));
        }
        a.searching = true;
        a.query = "herdr".into();
        snapshots.push(screen(&a, w, h));
        a.searching = false;
        a.help = true;
        snapshots.push(screen(&a, w, h));
        a.help = false;
        a.confirmation = Some(waystation::app::Confirmation {
            title: "Stop tunnel?".into(),
            choices: vec![
                ("Cancel".into(), Action::Escape),
                ("Stop".into(), Action::ConfirmStop(uuid::Uuid::nil())),
            ],
        });
        snapshots.push(screen(&a, w, h));
        a.confirmation = None;
        a.detail = Some((
            "Tunnel run logs".into(),
            "Permission denied\nESC is sanitized: \x1b]52;c;hidden".into(),
        ));
        snapshots.push(screen(&a, w, h));
        a.detail = None;
        a.update(Action::NewAgent);
        snapshots.push(screen(&a, w, h));
        a.update(Action::Escape);
        insta::assert_snapshot!(format!("station_{w}x{h}"), snapshots.join("\n\n"));
    }
}
#[test]
fn quit_confirmation_is_visible_below_minimum_size() {
    let mut a = App::new(Config::default(), AppState::default());
    a.confirmation = Some(waystation::app::Confirmation {
        title: "Background processes are still running".into(),
        choices: vec![
            ("Keep running and quit".into(), Action::QuitKeep),
            ("Cancel".into(), Action::Escape),
        ],
    });
    assert!(screen(&a, 59, 17).contains("Keep running and quit"));
}
