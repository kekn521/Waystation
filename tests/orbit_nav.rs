use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{path::PathBuf, time::Duration};
use uuid::Uuid;
use waystation::{
    agents::{AgentSession, AgentStatus},
    app::{Action, App, Effect},
    config::{Config, ToolCommand},
    model::{AppState, Section},
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
fn event(id: ProviderId, app: &App, payload: Result<ProviderPayload, String>) -> ProviderEvent {
    ProviderEvent {
        request: ProviderRequest {
            id,
            generation: app.generation,
            workspace: None,
            directory: None,
            hidden: false,
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
    let planets = |app: &App| {
        app.planets()
            .iter()
            .map(|w| w.name.clone())
            .collect::<Vec<_>>()
    };
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
    assert_eq!(
        moons.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert!(app.moons(&path("w0")).is_empty());
}

#[test]
fn keyboard_lands_on_a_planet_and_opens_its_moons() {
    let mut app = app();
    app.update(Action::Move(1));
    assert_eq!(app.orbit.focus, 1);
    let effects = app.update(Action::Activate);
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert!(app.orbit.transition.is_some(), "landing animates");
    assert!(effects.is_empty(), "w1 was already selected: {effects:?}");

    app.update(Action::Move(-1));
    assert_eq!(app.orbit.focus, 1, "moon focus wraps around");
    let b = moon(&app, "b");
    assert_eq!(app.orbit.moon, Some(b), "the highlight remembers its moon");
    app.update(Action::Move(1));
    let a = moon(&app, "a");
    let effects = app.update(Action::Activate);
    assert!(
        matches!(effects[..], [Effect::AttachAgent(id)] if id == a),
        "Enter opens the highlighted agent full screen: {effects:?}"
    );
    assert_eq!(
        app.orbit.level,
        Level::Workspace(path("w1")),
        "no preview level"
    );
    assert_eq!(app.section, Section::Overview, "attaching keeps the orbit");
    assert_eq!(app.orbit.focus, 0, "the moon we opened stays focused");

    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert_eq!(app.orbit.focus, 1, "the planet we left stays focused");
    app.message = Some("hello".into());
    app.update(Action::Escape);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert!(
        app.message.is_none(),
        "Esc in the galaxy keeps its old meaning"
    );
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
    assert_eq!(
        app.orbit.level,
        Level::Galaxy,
        "the System pane keeps Enter"
    );

    app.pane = 0;
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::Search);
    app.update(Action::Escape);
    assert!(!app.searching);
    assert_eq!(
        app.orbit.level,
        Level::Workspace(path("w1")),
        "Esc closed search first"
    );

    app.section = Section::Agents;
    app.update(Action::Escape);
    assert_eq!(
        app.orbit.level,
        Level::Workspace(path("w1")),
        "other sections keep Esc"
    );
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
    app.pane = 2;
    let effects = app.update(Action::OrbitAttach(c));
    assert!(matches!(effects[..], [Effect::AttachAgent(id)] if id == c));
    assert_eq!(app.pane, 0, "returning lands on the orbit");
    assert_eq!(app.orbit.moon, Some(c));
    assert_eq!(app.section, Section::Overview);

    app.update(Action::OrbitOut);
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert_eq!(app.orbit.focus, 2);
}

#[test]
fn tab_cycles_the_orbit_system_and_services() {
    let mut app = app();
    for zoom in [false, true] {
        if zoom {
            app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
        }
        assert_eq!(app.pane, 0);
        app.update(Action::FocusNext);
        assert_eq!(app.pane, 1);
        app.update(Action::FocusNext);
        assert_eq!(app.pane, 2);
        app.update(Action::FocusNext);
        assert_eq!(app.pane, 0);
    }
}

#[test]
fn the_orbit_falls_back_when_bodies_disappear() {
    let mut app = app();
    let a = moon(&app, "a");
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::Move(1));
    let others = app
        .agents
        .iter()
        .filter(|s| s.id != moon(&app, "b"))
        .cloned()
        .collect();
    app.apply_provider(event(
        ProviderId::Agents,
        &app,
        Ok(ProviderPayload::Agents(others)),
    ));
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert_eq!(app.orbit.focus, 0, "the highlight stays in range");
    assert_eq!(app.orbit.moon, Some(a), "and names the moon it now sits on");

    app.apply_provider(event(
        ProviderId::Projects,
        &app,
        Ok(ProviderPayload::Projects(vec![
            workspace("w0"),
            workspace("w2"),
        ])),
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
fn transitions_ease() {
    let mut app = app();
    app.animation_elapsed = Duration::from_secs(10);
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    let start = app.animation_elapsed;
    assert_eq!(app.orbit.progress(start), 0.0);
    assert!(app.orbit.animating(start));
    assert_eq!(app.orbit.progress(start + TRANSITION), 1.0);
    assert!(!app.orbit.animating(start + TRANSITION));

    // Switching moons is not a zoom.
    app.animation_elapsed += TRANSITION;
    app.update(Action::Move(1));
    assert!(!app.orbit.animating(app.animation_elapsed));
    assert_eq!(OrbitView::default().progress(Duration::ZERO), 1.0);
}

#[test]
fn a_new_moon_keeps_the_highlighted_one_focused() {
    let mut app = app();
    let b = moon(&app, "b");
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::Move(1));
    assert_eq!(app.orbit.focus, 1);
    let mut agents = app.agents.clone();
    agents.push(agent("0first", "w1"));
    app.apply_provider(event(
        ProviderId::Agents,
        &app,
        Ok(ProviderPayload::Agents(agents)),
    ));
    assert_eq!(app.orbit.moon, Some(b));
    assert_eq!(app.moons(&path("w1"))[app.orbit.focus].id, b);
    let effects = app.update(Action::Activate);
    assert!(matches!(effects[..], [Effect::AttachAgent(id)] if id == b));
}

#[test]
fn falling_back_focuses_the_newly_selected_planet() {
    let mut app = app();
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::SelectWorkspace(path("w2")));
    assert_eq!(app.orbit.level, Level::Galaxy);
    assert_eq!(app.planets()[app.orbit.focus].id, path("w2"));
}

#[test]
fn an_agent_started_from_the_orbit_opens_as_its_moon() {
    let mut app = app();
    app.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    app.update(Action::NewAgent);
    assert!(app.form.is_some(), "n opens the New agent form");
    let created = agent("new", "w1");
    let id = created.id;
    let effects = app.adopt_new_agent(created);
    assert!(app.form.is_none());
    assert_eq!(
        app.section,
        Section::Overview,
        "returning lands on the orbit"
    );
    assert_eq!(app.orbit.level, Level::Workspace(path("w1")));
    assert_eq!(app.orbit.moon, Some(id), "its moon is highlighted");
    assert!(matches!(effects[..], [Effect::AttachAgent(x)] if x == id));

    // Started anywhere else, it lands in Agents as before.
    let mut app = app_in_agents();
    let created = agent("other", "w2");
    let id = created.id;
    let effects = app.adopt_new_agent(created);
    assert_eq!(app.section, Section::Agents);
    assert_eq!(app.workspace(), Some(path("w2").as_path()));
    assert!(matches!(effects[..], [Effect::AttachAgent(x)] if x == id));
    assert!(matches!(&app.agent_items()[app.selection].2, Action::OpenAgent(x) if *x == id));
}
fn app_in_agents() -> App {
    let mut app = app();
    app.section = Section::Agents;
    app
}
