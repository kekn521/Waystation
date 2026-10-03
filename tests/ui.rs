use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use station::{
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
    app.update(input::translate(key('8'), &app).unwrap());
    assert_eq!(app.section, Section::System);
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
                station::ui::draw(f, &app);
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
            text.contains(if w < 60 { "Resize" } else { "STATION" }),
            "missing usable shell at {w}x{h}"
        );
    }
}
