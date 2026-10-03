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
