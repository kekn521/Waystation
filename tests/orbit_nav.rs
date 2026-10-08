use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;
use waystation::{
    agents::{AgentSession, AgentStatus},
    app::{Action, App, Effect},
    config::{Config, ToolCommand},
    model::{AppState, Availability, Section},
    orbit_nav::{Level, OrbitView, TRANSITION},
    providers::projects::Workspace,
    runtime::workers::{ProviderEvent, ProviderId, ProviderPayload, ProviderRequest},
};

fn path(name: &str) -> PathBuf {
    format!("/orbit-fixture/{name}").into()
}
fn workspace(name: &str) -> Workspace {
    Workspace {
        id: path(name),
        name: name.into(),
    }
}
fn agent(name: &str, workspace: &str) -> AgentSession {
    AgentSession {
        id: Uuid::new_v4(),
        name: name.into(),
        tool: "claude".into(),
        workspace: path(workspace),
        command: ToolCommand {
            program: "claude".into(),
            args: vec![],
        },
        executable: "/usr/bin/claude".into(),
        created: std::time::SystemTime::UNIX_EPOCH,
        conversation: None,
        status: AgentStatus::Running,
    }
}
/// Planets w0..w2 (w1 selected), with agents "a" and "b" circling w1.
fn app() -> App {
    let mut app = App::new(
        Config::default(),
        AppState {
            selected_workspace: Some(path("w1")),
            recent_workspaces: vec![path("w1"), path("w0"), path("w2")],
            ..AppState::default()
        },
    );
    app.set_workspaces(vec![workspace("w0"), workspace("w1"), workspace("w2")]);
    app.agents = vec![agent("b", "w1"), agent("a", "w1"), agent("c", "w2")];
    app.pane = 0;
    app
}
fn moon(app: &App, name: &str) -> Uuid {
    app.agents.iter().find(|s| s.name == name).unwrap().id
}
fn event(id: ProviderId, app: &App, agent: Option<Uuid>, payload: Result<ProviderPayload, String>) -> ProviderEvent {
    ProviderEvent {
        request: ProviderRequest {
            id,
            generation: app.generation,
            workspace: None,
            directory: None,
            hidden: false,
            agent,
        },
        payload,
    }
}

#[test]
fn planets_follow_selection_pins_and_recents_in_a_stable_order() {
    let names = ["w0", "w1", "w2", "w3", "w4", "w5", "w6", "w7"];
    let mut app = App::new(
        Config {
            pinned_projects: vec![path("w7"), path("w5")],
            ..Config::default()
        },
        AppState {
            selected_workspace: Some(path("w6")),
            recent_workspaces: ["w6", "w3", "w0", "w1", "w2", "w4"].map(path).to_vec(),
            ..AppState::default()
        },
    );
    app.set_workspaces(names.map(workspace).to_vec());
    let planets = |app: &App| app.planets().iter().map(|w| w.name.clone()).collect::<Vec<_>>();
    // Selected, pinned, then the most recent fill the six places; pinned lead, then by path.
    assert_eq!(planets(&app), ["w7", "w5", "w0", "w1", "w3", "w6"]);
    // Landing on a planet makes it the most recent but never reshuffles the system.
    app.update(Action::SelectWorkspace(path("w1")));
    assert_eq!(planets(&app), ["w7", "w5", "w0", "w1", "w3", "w6"]);
    // Workspaces that are not discovered never become planets.
    app.set_workspaces(vec![workspace("w0")]);
    assert_eq!(planets(&app), ["w0"]);
}

#[test]
fn moons_are_the_workspace_agents_in_name_order() {
    let app = app();
    let moons = app.moons(&path("w1"));
    assert_eq!(moons.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    assert!(app.moons(&path("w0")).is_empty());
}

#[test]
fn keyboard_zooms_from_galaxy_to_moon_and_back() {
    let mut app = app();
    app.update(Action::Move(1));
    assert_eq!(app.orbit.focus, 1);
    let effects = app.update(Action::Activate);
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert!(app.orbit.transition.is_some(), "landing animates");
    assert!(effects.is_empty(), "w1 was already selected: {effects:?}");

    app.update(Action::Move(-1));
    assert_eq!(app.orbit.focus, 1, "moon focus wraps around");
    app.update(Action::Move(1));
    let effects = app.update(Action::Activate);
    let a = moon(&app, "a");
    assert_eq!(app.orbit.level, Level::Moon(path("w1"), a));
    assert!(matches!(effects[..], [Effect::Preview]), "{effects:?}");
    assert_eq!(app.previewed_agent(), Some(a));
    assert_eq!(app.agent_preview.as_ref().map(|(id, _)| *id), Some(a));

    let effects = app.update(Action::Move(1));
    let b = moon(&app, "b");
    assert_eq!(app.orbit.level, Level::Moon(path("w1"), b), "the preview follows");
    assert!(matches!(effects[..], [Effect::Preview]));

    let effects = app.update(Action::Activate);
    assert!(matches!(effects[..], [Effect::AttachAgent(id)] if id == b), "{effects:?}");
    assert_eq!(app.section, Section::Overview, "attaching keeps the orbit");

    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert_eq!(app.orbit.focus, 1, "the moon we left stays focused");
    assert!(app.agent_preview.is_none());
    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert_eq!(app.orbit.focus, 1, "the planet we left stays focused");
    app.message = Some("hello".into());
    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert!(app.message.is_none(), "Esc in the galaxy keeps its old meaning");
}

#[test]
fn landing_on_another_planet_selects_its_workspace() {
    let mut app = app();
    let effects = app.update(Action::Activate);
    assert_eq!(app.orbit.level, Level::Workspace(path("w0")));
    assert_eq!(app.workspace(), Some(path("w0").as_path()));
    assert!(effects.iter().any(|e| matches!(e, Effect::Refresh)));
}

#[test]
fn keys_reach_the_orbit_only_when_it_has_focus() {
    let mut app = app();
    app.pane = 1;
    app.update(Action::Activate);
    assert_eq!(app.orbit.level, Level::Galaxy, "the Workspaces pane keeps Enter");

    app.pane = 0;
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::Search);
    app.update(Action::Escape);
    assert!(!app.searching);
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")), "Esc closed search first");

    app.section = Section::Agents;
    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")), "other sections keep Esc");
}

#[test]
fn clicks_focus_the_orbit_and_jump_levels() {
    let mut app = app();
    app.pane = 2;
    app.update(Action::OrbitEnter(Level::Workspace(path("w2"))));
    assert_eq!(app.pane, 0);
    assert_eq!(app.orbit.level, Level::Workspace(path("w2")));
    assert_eq!(app.workspace(), Some(path("w2").as_path()));

    let c = moon(&app, "c");
    app.pane = 3;
    let effects = app.update(Action::OrbitEnter(Level::Moon(path("w2"), c)));
    assert!(matches!(effects[..], [Effect::Preview]));
    assert_eq!(app.pane, 0);

    let effects = app.update(Action::OrbitAttach(c));
    assert!(matches!(effects[..], [Effect::AttachAgent(id)] if id == c));
    assert_eq!(app.section, Section::Overview);

    app.update(Action::OrbitOut);
    assert_eq!(app.orbit.level, Level::Workspace(path("w2")));
    app.update(Action::OrbitOut);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert_eq!(app.orbit.focus, 2);
}

#[test]
fn tab_skips_the_hidden_workspaces_pane_while_zoomed() {
    let mut app = app();
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::FocusNext);
    assert_eq!(app.pane, 2);
    app.update(Action::FocusNext);
    app.update(Action::FocusNext);
    assert_eq!(app.pane, 0);
}

#[test]
fn the_orbit_falls_back_when_bodies_disappear() {
    let mut app = app();
    let a = moon(&app, "a");
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::OrbitEnter(Level::Moon(path("w1"), a)));
    let others = app.agents.iter().filter(|s| s.id != a).cloned().collect();
    app.apply_provider(event(ProviderId::Agents, &app, None, Ok(ProviderPayload::Agents(others))));
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert!(app.agent_preview.is_none());

    app.apply_provider(event(
        ProviderId::Projects,
        &app,
        None,
        Ok(ProviderPayload::Projects(vec![workspace("w0"), workspace("w2")])),
    ));
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert!(app.orbit.focus < app.planets().len().max(1));
}

#[test]
fn selecting_another_workspace_elsewhere_returns_to_the_galaxy() {
    let mut app = app();
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::SelectWorkspace(path("w2")));
    assert_eq!(app.orbit.level, Level::Galaxy);
}

#[test]
fn n_starts_an_agent_from_a_zoomed_orbit() {
    let key = Event::Key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
    let mut app = app();
    assert!(waystation::input::translate(key.clone(), &app).is_none());
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    assert!(matches!(
        waystation::input::translate(key.clone(), &app),
        Some(Action::NewAgent)
    ));
    app.pane = 2;
    assert!(waystation::input::translate(key, &app).is_none());
}

#[test]
fn transitions_ease_and_split_the_pane() {
    let mut app = app();
    let a = moon(&app, "a");
    app.animation_elapsed = Duration::from_secs(10);
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    let start = app.animation_elapsed;
    assert_eq!(app.orbit.progress(start), 0.0);
    assert!(app.orbit.animating(start));
    assert_eq!(app.orbit.progress(start + TRANSITION), 1.0);
    assert!(!app.orbit.animating(start + TRANSITION));
    assert_eq!(app.orbit.split(start), 1.0, "galaxy to workspace uses the whole pane");

    app.update(Action::OrbitEnter(Level::Moon(path("w1"), a)));
    assert_eq!(app.orbit.split(start), 1.0);
    let middle = app.orbit.split(start + TRANSITION / 2);
    assert!(middle < 1.0 && middle > 0.4, "{middle}");
    assert_eq!(app.orbit.split(start + TRANSITION), 0.4);

    // Switching moons is not a zoom.
    app.animation_elapsed += TRANSITION;
    app.update(Action::Move(1));
    assert!(app.orbit.transition.is_none() || !app.orbit.animating(app.animation_elapsed));
    assert_eq!(OrbitView::default().progress(Duration::ZERO), 1.0);
}

#[test]
fn previews_only_land_on_the_open_moon() {
    let mut app = app();
    let (a, b) = (moon(&app, "a"), moon(&app, "b"));
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::OrbitEnter(Level::Moon(path("w1"), a)));
    let preview = |app: &App| app.agent_preview.as_ref().unwrap().1.clone();

    app.apply_provider(event(
        ProviderId::AgentPreview,
        &app,
        Some(b),
        Ok(ProviderPayload::AgentPreview(b, "late".into())),
    ));
    assert!(preview(&app).value.is_none(), "a capture for a moon we left is ignored");

    app.apply_provider(event(
        ProviderId::AgentPreview,
        &app,
        Some(a),
        Ok(ProviderPayload::AgentPreview(a, "hi".into())),
    ));
    assert_eq!(preview(&app).value.as_deref(), Some("hi"));

    app.apply_provider(event(ProviderId::AgentPreview, &app, Some(a), Err("tmux went away".into())));
    let snapshot = preview(&app);
    assert!(matches!(snapshot.availability, Availability::Failed(ref e) if e == "tmux went away"));
    assert_eq!(snapshot.value.as_deref(), Some("hi"), "the last screen stays, marked stale");
}
