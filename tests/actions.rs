use station::{
    app::Action,
    config::{Config, ToolCommand},
    runtime::actions::resolve,
};
#[test]
fn validates_program_and_cwd() {
    let d = tempfile::tempdir().unwrap();
    let mut c = Config::default();
    c.editor = Some(ToolCommand {
        program: "no-such-station-editor-9284".into(),
        args: vec![],
    });
    assert!(resolve(&Action::Editor, &c, d.path()).is_err());
    c.editor.as_mut().unwrap().program = "printf".into();
    assert!(resolve(&Action::Editor, &c, &d.path().join("missing")).is_err());
}
#[test]
fn editor_arguments_are_literal() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("-weird $(touch sentinel)");
    std::fs::write(&p, "").unwrap();
    let c = Config {
        editor: Some(ToolCommand {
            program: "printf".into(),
            args: vec!["%s".into()],
        }),
        ..Config::default()
    };
    let s = resolve(&Action::OpenPath(p.clone()), &c, d.path()).unwrap();
    assert_eq!(s.args[1], p);
}
#[test]
fn rejects_option_shaped_ssh_hosts() {
    let d = tempfile::tempdir().unwrap();
    assert!(
        resolve(
            &Action::Connect("-oProxyCommand=bad".into()),
            &Config::default(),
            d.path()
        )
        .is_err()
    );
}
