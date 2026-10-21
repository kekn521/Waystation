use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use waystation::{
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
fn invalid_form_keeps_input_and_unicode_editing_is_safe() {
    let mut a = app();
    a.update(Action::NewAgent);
    type_text(&mut a, "機器x");
    a.update(Action::FormCursor(-1));
    a.update(Action::Backspace);
    assert_eq!(a.form.as_ref().unwrap().name, "機x");
    // Editing a multi-byte name must not corrupt it; saving an empty name is
    // what validation rejects.
    a.update(Action::FormClear);
    a.update(Action::FormSave);
    assert!(a.form.as_ref().unwrap().error.is_some());
    assert!(a.form.as_ref().unwrap().name.is_empty());
    a.update(Action::Escape);
    assert!(a.form.is_none());
}
#[test]
fn forms_render_at_small_and_wide_sizes() {
    let mut a = app();
    a.update(Action::NewAgent);
    for (w, h) in [(140, 45), (80, 24), (60, 18), (59, 17), (1, 1)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                waystation::ui::draw(f, &a);
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

#[test]
fn paste_never_triggers_navigation_or_execution() {
    let mut a = app();
    a.update(Action::NewAgent);
    let event = Event::Paste("q3 /hello".into());
    a.update(input::translate(event, &a).unwrap());
    assert_eq!(a.form.as_ref().unwrap().name, "q3 /hello");
    a.update(Action::Paste("bad\ntext".into()));
    assert!(a.form.as_ref().unwrap().error.is_some());
    assert_eq!(a.form.as_ref().unwrap().name, "q3 /hello");
}

#[test]
fn r_asks_before_restarting_the_selected_agent() {
    let mut a = app();
    a.section = Section::Agents;
    let mut with_conversation: waystation::agents::AgentSession =
        serde_json::from_value(serde_json::json!({
            "id": uuid::Uuid::new_v4(), "name": "slides", "tool": "claude", "workspace": "/tmp",
            "command": {"program": "claude", "args": []}, "executable": "/usr/bin/claude",
            "created": {"secs_since_epoch": 1, "nanos_since_epoch": 0}, "conversation": "c-1",
        }))
        .unwrap();
    with_conversation.status = waystation::agents::AgentStatus::Running;
    let id = with_conversation.id;
    a.agents = vec![with_conversation];
    a.selection = a
        .agent_items()
        .iter()
        .position(|(_, _, action)| matches!(action, Action::OpenAgent(s) if *s == id))
        .unwrap();
    let action = input::translate(
        Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        &a,
    )
    .unwrap();
    assert!(a.update(action).is_empty());
    let title = &a.confirmation.as_ref().expect("asks first").title;
    assert!(
        title.contains("slides") && title.contains("same conversation"),
        "{title}"
    );
    let effects = a.update(Action::ConfirmChoice(1));
    assert!(
        matches!(effects[..], [Effect::RestartAgent(x)] if x == id),
        "{effects:?}"
    );

    a.agents[0].conversation = None;
    let action = input::translate(
        Event::Key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE)),
        &a,
    )
    .unwrap();
    a.update(action);
    let title = &a.confirmation.as_ref().unwrap().title;
    assert!(title.contains("fresh"), "{title}");
}

fn app_with_roots(roots: &[&str]) -> App {
    App::new(
        Config {
            project_roots: roots.iter().map(Into::into).collect(),
            ..Config::default()
        },
        AppState {
            selected_workspace: Some("/tmp".into()),
            ..AppState::default()
        },
    )
}

#[test]
fn workspace_form_creates_in_the_chosen_root() {
    let mut a = app_with_roots(&["/roots/code", "/roots/play"]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, " fresh ");
    a.update(Action::FormField(1));
    assert_eq!(a.form.as_ref().unwrap().location, "/roots/code");
    a.update(Action::FormChoice(1));
    assert_eq!(a.form.as_ref().unwrap().location, "/roots/play");
    let effects = a.update(Action::FormSave);
    assert!(
        matches!(&effects[..], [Effect::CreateWorkspace { root, name }]
            if root == std::path::Path::new("/roots/play") && name == "fresh"),
        "{effects:?}"
    );
    assert!(a.form.as_ref().unwrap().busy);
}

#[test]
fn workspace_form_has_three_stops_and_enter_walks_to_the_button() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, "fresh");
    assert!(a.update(Action::Activate).is_empty());
    assert!(a.update(Action::Activate).is_empty());
    assert_eq!(a.form.as_ref().unwrap().field, 2);
    assert!(matches!(
        &a.update(Action::Activate)[..],
        [Effect::CreateWorkspace { .. }]
    ));
    a.form.as_mut().unwrap().busy = false;
    a.update(Action::FormField(1));
    assert_eq!(
        a.form.as_ref().unwrap().field,
        0,
        "wraps after three fields"
    );
}

#[test]
fn workspace_form_explains_bad_names_and_missing_roots() {
    let mut a = app_with_roots(&[]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, "fresh");
    assert!(a.update(Action::FormSave).is_empty());
    let error = a.form.as_ref().unwrap().error.clone().unwrap();
    assert!(error.contains("location"), "{error}");
    a.update(Action::FormField(1));
    type_text(&mut a, "code/elsewhere");
    assert!(a.update(Action::FormSave).is_empty());
    let error = a.form.as_ref().unwrap().error.clone().unwrap();
    assert!(error.contains("full path"), "{error}");

    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, "a/b");
    assert!(a.update(Action::FormSave).is_empty());
    assert!(a.form.as_ref().unwrap().error.is_some());
    assert!(!a.form.as_ref().unwrap().busy);
    assert_eq!(a.form.as_ref().unwrap().name, "a/b");
}

#[test]
fn n_in_workspaces_opens_the_new_workspace_form() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.section = Section::Workspaces;
    let action = input::translate(
        Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE)),
        &a,
    )
    .unwrap();
    assert!(matches!(action, Action::NewWorkspace));
}

#[test]
fn workspace_form_renders_at_small_and_wide_sizes() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::NewWorkspace);
    for (w, h) in [(140, 45), (80, 24), (60, 18), (59, 17), (1, 1)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                waystation::ui::draw(f, &a);
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
            assert!(text.contains("New workspace"), "{w}x{h}");
            assert!(text.contains("/roots/code"), "{w}x{h}");
        }
    }
}

#[test]
fn a_created_workspace_is_listed_and_selected_at_once() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.section = Section::Workspaces;
    a.set_workspaces(vec![
        waystation::providers::projects::Workspace::new("/roots/code/alpha".into()),
        waystation::providers::projects::Workspace::new("/roots/code/zeta".into()),
    ]);
    a.update(Action::NewWorkspace);
    let made = std::path::PathBuf::from("/roots/code/fresh");
    let effects = a.adopt_new_workspace(made.clone(), false);
    assert!(a.form.is_none());
    assert_eq!(a.workspace(), Some(made.as_path()));
    assert_eq!(a.state.recent_workspaces.first(), Some(&made));
    assert!(matches!(&a.section_items()[a.selection].2,
        Action::SelectWorkspace(p) if *p == made));
    assert!(effects.iter().any(|e| matches!(e, Effect::Refresh)));
    // A rescan that finds it does not list it twice.
    a.set_workspaces(vec![
        waystation::providers::projects::Workspace::new("/roots/code/alpha".into()),
        waystation::providers::projects::Workspace::new(made.clone()),
        waystation::providers::projects::Workspace::new("/roots/code/zeta".into()),
    ]);
    assert_eq!(a.workspaces.iter().filter(|w| w.id == made).count(), 1);
}

#[test]
fn workspace_location_takes_a_typed_path() {
    let mut a = app_with_roots(&["/roots/code", "/roots/play"]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, "fresh");
    a.update(Action::FormField(1));
    // Arrows edit the path rather than switch roots.
    a.update(Action::FormCursor(-1));
    a.update(Action::FormCursor(1));
    assert_eq!(a.form.as_ref().unwrap().location, "/roots/code");
    a.update(Action::FormClear);
    type_text(&mut a, " /elsewhere/projects ");
    let effects = a.update(Action::FormSave);
    assert!(
        matches!(&effects[..], [Effect::CreateWorkspace { root, name }]
            if root == std::path::Path::new("/elsewhere/projects") && name == "fresh"),
        "{effects:?}"
    );

    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::NewWorkspace);
    type_text(&mut a, "fresh");
    a.update(Action::FormField(1));
    a.update(Action::FormClear);
    type_text(&mut a, "~/side");
    let effects = a.update(Action::FormSave);
    let home = std::env::home_dir().unwrap();
    assert!(
        matches!(&effects[..], [Effect::CreateWorkspace { root, .. }] if *root == home.join("side")),
        "{effects:?}"
    );
}

#[test]
fn ctrl_arrows_step_through_roots_in_the_workspace_form() {
    let mut a = app_with_roots(&["/roots/code", "/roots/play"]);
    a.update(Action::NewWorkspace);
    a.update(Action::FormField(1));
    let action = input::translate(
        Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
        &a,
    )
    .unwrap();
    assert!(matches!(action, Action::FormChoice(1)));
    a.update(action);
    assert_eq!(a.form.as_ref().unwrap().location, "/roots/play");
}

#[test]
fn a_workspace_pinned_on_creation_survives_rescans() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.set_workspaces(vec![waystation::providers::projects::Workspace::new(
        "/roots/code/alpha".into(),
    )]);
    a.update(Action::NewWorkspace);
    let made = std::path::PathBuf::from("/elsewhere/fresh");
    a.adopt_new_workspace(made.clone(), true);
    // This run's scans still use the startup config, which lacks the pin.
    a.set_workspaces(vec![waystation::providers::projects::Workspace::new(
        "/roots/code/alpha".into(),
    )]);
    assert_eq!(a.workspaces.iter().filter(|w| w.id == made).count(), 1);
    assert_eq!(a.workspaces[0].id, made, "listed with the pins first");
}

#[test]
fn a_in_workspaces_adds_a_typed_folder() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.section = Section::Workspaces;
    let action = input::translate(
        Event::Key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)),
        &a,
    )
    .unwrap();
    assert!(matches!(action, Action::AddFolder));
    a.update(action);
    let form = a.form.as_ref().unwrap();
    assert_eq!(form.kind, waystation::forms::FormKind::AddFolder);
    assert_eq!(form.location, "/roots/code/");
    assert_eq!(form.fields(), 2);
    // Typing continues the path, and Enter walks to the button before adding.
    type_text(&mut a, "notes");
    assert!(a.update(Action::Activate).is_empty());
    let effects = a.update(Action::Activate);
    assert!(
        matches!(&effects[..], [Effect::AddWorkspace(dir)] if dir == std::path::Path::new("/roots/code/notes")),
        "{effects:?}"
    );
    assert!(a.form.as_ref().unwrap().busy);
}

#[test]
fn add_folder_wants_a_full_path() {
    let mut a = app_with_roots(&[]);
    a.update(Action::AddFolder);
    assert!(a.update(Action::FormSave).is_empty());
    assert!(a.form.as_ref().unwrap().error.is_some());
    type_text(&mut a, "code/notes");
    assert!(a.update(Action::FormSave).is_empty());
    let error = a.form.as_ref().unwrap().error.clone().unwrap();
    assert!(error.contains("full path"), "{error}");
    a.update(Action::FormClear);
    type_text(&mut a, "~/notes");
    let home = std::env::home_dir().unwrap();
    assert!(matches!(
        &a.update(Action::FormSave)[..],
        [Effect::AddWorkspace(dir)] if *dir == home.join("notes")
    ));
}

#[test]
fn an_added_folder_is_listed_selected_and_kept() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::AddFolder);
    let dir = std::path::PathBuf::from("/elsewhere/notes");
    a.adopt_new_workspace(dir.clone(), true);
    assert!(a.form.is_none());
    assert_eq!(a.workspace(), Some(dir.as_path()));
    a.set_workspaces(vec![]);
    assert!(a.workspaces.iter().any(|w| w.id == dir));
}

#[test]
fn add_folder_form_renders_at_small_and_wide_sizes() {
    let mut a = app_with_roots(&["/roots/code"]);
    a.update(Action::AddFolder);
    for (w, h) in [(140, 45), (80, 24), (60, 18), (1, 1)] {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        terminal
            .draw(|f| {
                waystation::ui::draw(f, &a);
            })
            .unwrap();
        if w >= 60 {
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("Add existing folder"), "{w}x{h}");
            assert!(text.contains("/roots/code/"), "{w}x{h}");
            assert!(text.contains("Add folder"), "{w}x{h}");
        }
    }
}
