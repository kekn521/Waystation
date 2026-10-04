use station::{
    app::{Action, App, Effect},
    config::{Config, TaskRecipe, ToolCommand},
    model::{AppState, Section},
};
#[test]
fn recipe_action_uses_selected_workspace() {
    let d = tempfile::tempdir().unwrap();
    let mut app = App::new(
        Config {
            tasks: vec![TaskRecipe {
                id: "check".into(),
                label: "Check".into(),
                cwd: ".".into(),
                required_ports: vec![],
                command: ToolCommand {
                    program: "printf".into(),
                    args: vec!["%s".into(), "literal; $(cmd)".into()],
                },
            }],
            ..Config::default()
        },
        AppState {
            selected_workspace: Some(d.path().into()),
            ..AppState::default()
        },
    );
    let e = app.update(Action::StartRecipe("check".into()));
    let Effect::StartTask(r) = &e[0] else {
        panic!("missing task effect")
    };
    assert_eq!(r.cwd, d.path().join("."));
    assert_eq!(r.command.args[1], "literal; $(cmd)");
    assert_eq!(app.section, Section::Tasks);
}
#[test]
fn overview_focus_activates_matching_panel() {
    let mut a = App::new(Config::default(), AppState::default());
    a.update(Action::FocusNext);
    assert!(
        matches!(a.update(Action::Activate).first(),Some(Effect::Foreground(Action::Tool(t)))if t=="htop")
    );
}
fn run_record() -> station::tasks::RunRecord {
    use station::tasks::{RunRecord, RunStatus};
    RunRecord {
        id: uuid::Uuid::new_v4(),
        recipe: TaskRecipe {
            id: "fixture".into(),
            label: "Fixture".into(),
            cwd: "/tmp".into(),
            required_ports: vec![],
            command: ToolCommand {
                program: "printf".into(),
                args: vec!["hello".into()],
            },
        },
        cwd: "/tmp".into(),
        started: std::time::SystemTime::now(),
        ended: None,
        status: RunStatus::Passed,
        exit_code: Some(0),
        supervisor: None,
        child: None,
        log_dir: "/tmp".into(),
        error: None,
    }
}
#[test]
fn task_shortcuts_require_a_highlighted_history_run() {
    let mut a = App::new(Config::default(), AppState::default());
    a.runs.push(run_record());
    for s in [
        Section::Workspaces,
        Section::Files,
        Section::Services,
        Section::Agents,
        Section::Activity,
    ] {
        a.section = s;
        assert!(a.update(Action::Rerun).is_empty());
        assert!(a.update(Action::Stop).is_empty());
        assert!(a.confirmation.is_none());
    }
    a.section = Section::Overview;
    for pane in [0, 1, 3] {
        a.pane = pane;
        assert!(a.update(Action::Rerun).is_empty());
    }
    a.section = Section::Tasks;
    a.recipes = true;
    assert!(a.update(Action::Rerun).is_empty());
    a.recipes = false;
    assert!(matches!(
        a.update(Action::Rerun).first(),
        Some(Effect::StartTask(_))
    ));
}
#[test]
fn stop_and_quit_waits_for_starting_tasks() {
    let mut a = App::new(Config::default(), AppState::default());
    let mut r = run_record();
    r.status = station::tasks::RunStatus::Starting;
    a.runs.push(r);
    assert!(a.update(Action::QuitStop).is_empty());
    assert!(a.message.unwrap().contains("starting"));
}
#[test]
fn files_error_row_and_keyboard_actions_agree() {
    use station::providers::files::FileEntry;
    let mut a = App::new(Config::default(), AppState::default());
    a.section = Section::Files;
    a.files = vec![FileEntry {
        path: "/tmp/a".into(),
        label: "a".into(),
        is_dir: false,
    }];
    a.provider_errors
        .insert("Files".into(), "permission denied".into());
    assert!(matches!(
        a.update(Action::Activate).first(),
        Some(Effect::Refresh)
    ));
    a.selection = 1;
    assert!(
        matches!(a.update(Action::Activate).first(),Some(Effect::Foreground(Action::OpenPath(p)))if p==std::path::Path::new("/tmp/a"))
    );
}
#[test]
fn quit_waits_until_new_run_is_in_a_snapshot() {
    use station::runtime::workers::*;
    let mut a = App::new(Config::default(), AppState::default());
    let r = run_record();
    a.pending_starts.insert(r.id);
    assert!(a.update(Action::Quit).is_empty());
    a.apply_provider(ProviderEvent {
        request: ProviderRequest {
            id: ProviderId::Tasks,
            generation: 0,
            workspace: None,
            directory: None,
            hidden: false,
        },
        payload: Ok(ProviderPayload::Tasks(vec![])),
    });
    assert!(!a.pending_starts.is_empty());
    a.apply_provider(ProviderEvent {
        request: ProviderRequest {
            id: ProviderId::Tasks,
            generation: 0,
            workspace: None,
            directory: None,
            hidden: false,
        },
        payload: Ok(ProviderPayload::Tasks(vec![r])),
    });
    assert!(a.pending_starts.is_empty());
    assert!(matches!(a.update(Action::Quit).first(), Some(Effect::Quit)));
}
