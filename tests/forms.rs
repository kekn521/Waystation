use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use station::{
    app::{Action, App, Effect},
    config::Config,
    input,
    model::{AppState, Section},
};
fn app() -> App {
    App::new(
        Config::default(),
        AppState {
            selected_workspace: Some("/tmp".into()),
            ..AppState::default()
        },
    )
}
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(
            input::translate(
                Event::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
                app,
            )
            .unwrap(),
        );
    }
}
#[test]
fn agent_form_isolates_shortcuts_and_chooses_provider() {
    let mut a = app();
    a.update(Action::NewAgent);
    type_text(&mut a, "q3 Review");
    assert_eq!(a.form.as_ref().unwrap().name, "q3 Review");
    assert_eq!(a.section, Section::Overview);
    a.update(Action::FormField(1));
    a.update(Action::FormCursor(1));
    let effects = a.update(Action::FormSave);
    assert!(
        matches!(&effects[0],Effect::CreateAgent { tool, workspace, .. } if tool == "claude" && workspace == std::path::Path::new("/tmp"))
    );
    assert!(a.form.as_ref().unwrap().busy);
    assert!(a.update(Action::FormSave).is_empty());
}
#[test]
fn task_form_saves_literal_argv_without_starting_task() {
    let mut a = app();
    a.update(Action::NewRecipe);
    type_text(&mut a, "Checks");
    a.update(Action::FormField(1));
    type_text(&mut a, "printf '%s' 'hello world'");
    let effects = a.update(Action::FormSave);
    let Effect::SaveRecipe(r) = &effects[0] else {
        panic!("must save, not execute")
    };
    assert_eq!(r.command.args, vec!["%s", "hello world"]);
    assert_eq!(r.cwd, std::path::Path::new("/tmp"));
}
#[test]
fn invalid_form_keeps_input_and_unicode_editing_is_safe() {
    let mut a = app();
    a.update(Action::NewRecipe);
    type_text(&mut a, "機器x");
    a.update(Action::FormCursor(-1));
    a.update(Action::Backspace);
    assert_eq!(a.form.as_ref().unwrap().name, "機x");
    a.update(Action::FormSave);
    assert!(a.form.as_ref().unwrap().error.is_some());
    a.update(Action::Escape);
    assert!(a.form.is_none());
}
#[test]
fn forms_render_at_small_and_wide_sizes() {
    for action in [Action::NewAgent, Action::NewRecipe] {
        let mut a = app();
        a.update(action);
        for (w, h) in [(140, 45), (80, 24), (60, 18), (59, 17), (1, 1)] {
            let mut terminal =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
            terminal
                .draw(|f| {
                    station::ui::draw(f, &a);
                })
                .unwrap();
            if w >= 59 {
                let text = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>();
                assert!(text.contains("Ctrl+S save"));
            }
        }
    }
}

#[test]
fn paste_never_triggers_navigation_or_execution() {
    let mut a = app();
    a.update(Action::NewRecipe);
    let event = Event::Paste("q3 /hello".into());
    a.update(input::translate(event, &a).unwrap());
    assert_eq!(a.form.as_ref().unwrap().name, "q3 /hello");
    a.update(Action::Paste("bad\ntext".into()));
    assert!(a.form.as_ref().unwrap().error.is_some());
    assert_eq!(a.form.as_ref().unwrap().name, "q3 /hello");
}
