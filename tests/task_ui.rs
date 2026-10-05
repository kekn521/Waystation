use station::{
    app::{Action, App, Effect},
    config::{Config, TaskRecipe, ToolCommand, TunnelRecipe},
    model::{ActivityEntry, ActivityKind, AppState, Section},
};
#[test]
fn tunnel_action_uses_selected_workspace_and_lands_on_connections() {
    let d = tempfile::tempdir().unwrap();
    let mut app = App::new(
        Config {
            tunnels: vec![TunnelRecipe {
                id: "fixture".into(),
                host: "example.invalid".into(),
                bind: "127.0.0.1".into(),
                local_port: 15432,
                remote_host: "db.internal".into(),
                remote_port: 5432,
            }],
            ..Config::default()
        },
        AppState {
            selected_workspace: Some(d.path().into()),
            ..AppState::default()
        },
    );
    // StartTunnel resolves config.tunnels through TaskRecipe::from_tunnel;
    // only the effect is produced, so no subprocess is ever spawned here.
    let e = app.update(Action::StartTunnel("fixture".into()));
    let Effect::StartTask(r) = &e[0] else {
        panic!("missing start effect")
    };
    assert_eq!(r.id, "tunnel:fixture");
    assert_eq!(r.cwd, d.path());
    assert_eq!(r.command.program, "ssh");
    assert_eq!(app.section, Section::Connections);
    assert!(app.runs.is_empty(), "an effect alone must not record a run");
}
#[test]
fn overview_focus_activates_matching_panel_and_the_orbit_pane_is_inert() {
    let mut a = App::new(Config::default(), AppState::default());
    assert_eq!(a.pane, 1, "default pane is Workspaces");
    a.update(Action::FocusNext);
    assert_eq!(a.pane, 2, "Tab moves to System");
    assert!(
        matches!(a.update(Action::Activate).first(),Some(Effect::Foreground(Action::Tool(t)))if t=="htop")
    );
    // Tab cycles all four panes; the orbit pane owns no actions.
    a.update(Action::FocusNext);
    assert_eq!(a.pane, 3);
    a.update(Action::FocusNext);
    assert_eq!(a.pane, 0);
    assert_eq!(a.selection, 0);
    assert!(
        a.section_items().is_empty(),
        "the orbit pane lists no actions"
    );
    assert!(
        a.update(Action::Activate).is_empty(),
        "Activate is a no-op there"
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
fn live_identity() -> station::tasks::identity::ProcessIdentity {
    // This very test process is running and matches its own /proc identity.
    station::tasks::identity::read(std::path::Path::new("/proc"), std::process::id()).unwrap()
}
#[test]
fn stop_confirms_only_on_a_selected_tunnel_run_in_connections() {
    let mut a = App::new(Config::default(), AppState::default());
    let mut legacy = run_record();
    legacy.status = station::tasks::RunStatus::Running;
    legacy.supervisor = Some(live_identity());
    let mut tunnel = run_record();
    tunnel.recipe.id = "tunnel:fixture".into();
    tunnel.status = station::tasks::RunStatus::Running;
    tunnel.supervisor = Some(live_identity());
    let tunnel_id = tunnel.id;
    a.runs.push(legacy.clone());
    a.runs.push(tunnel);
    a.section = Section::Connections;
    // Only the tunnel run row is listed; the legacy task run never appears.
    let items = a.connection_items();
    assert_eq!(items.len(), 1, "non-tunnel runs stay out of Connections");
    assert!(items[0].0.starts_with("Fixture"));
    // Enter on the tunnel row opens its logs.
    a.selection = 0;
    assert!(matches!(
        a.update(Action::Activate).first(),
        Some(Effect::ReadLog(id)) if *id == tunnel_id
    ));
    // x on the selected tunnel run asks before stopping.
    assert!(a.update(Action::Stop).is_empty());
    let confirmation = a.confirmation.clone().expect("stop confirmation");
    assert!(confirmation.title.contains("Fixture"));
    assert!(
        confirmation
            .choices
            .iter()
            .any(|(label, _)| label.contains("Stop this tunnel"))
    );
    assert!(matches!(
        a.update(Action::ConfirmChoice(1)).first(),
        Some(Effect::StopTask(id)) if *id == tunnel_id
    ));
    // Outside Connections the same key goes to agent-close behavior instead,
    // never to a run: with nothing selected there is no confirmation.
    let mut b = App::new(Config::default(), AppState::default());
    b.runs.push(legacy);
    b.section = Section::Activity;
    assert!(b.update(Action::Stop).is_empty());
    assert!(b.confirmation.is_none());
}
#[test]
fn legacy_task_history_stays_out_of_search_connections_and_activity() {
    let mut a = App::new(Config::default(), AppState::default());
    let r = run_record(); // recipe.id "fixture": a pre-removal task run
    let run_id = r.id;
    a.runs.push(r.clone());
    a.state.activity.push(ActivityEntry {
        id: run_id.to_string(),
        at: r.started,
        workspace: Some("/tmp".into()),
        kind: ActivityKind::TaskRun(run_id.to_string()),
        outcome: "Fixture · Passed".into(),
    });
    // Connections lists no launcher or log row for a non-tunnel run.
    assert!(
        a.connection_items()
            .iter()
            .all(|(_, _, act)| !matches!(act, Action::RunLog(id) if *id == run_id))
    );
    // Search carries no task/recipe actions and never offers this run's log.
    let search = a.search_items();
    assert!(
        search
            .iter()
            .all(|i| !matches!(i.action, Action::StartTask(_) | Action::RunLog(_)))
    );
    a.searching = true;
    a.query = "fixture".into();
    assert!(a.matches().iter().all(|m| !m.label.contains("Fixture")));
    // Activity hides legacy non-tunnel TaskRun entries.
    a.section = Section::Activity;
    assert!(a.activity_items().is_empty());
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
