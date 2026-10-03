use station::{app::App, config::Config, model::AppState, providers::projects::Workspace};
#[test]
fn refresh_preserves_selected_path() {
    let mut app = App::new(Config::default(), AppState::default());
    let w = |name: &str| Workspace {
        id: format!("/tmp/{name}").into(),
        name: name.into(),
    };
    app.set_workspaces(vec![w("a"), w("b")]);
    assert_eq!(app.workspace().unwrap(), std::path::Path::new("/tmp/a"));
    app.update(station::app::Action::Project(1));
    app.set_workspaces(vec![w("b"), w("a")]);
    assert_eq!(app.workspace().unwrap(), std::path::Path::new("/tmp/b"));
}
#[test]
fn unicode_names_survive_selection() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};
    for name in [
        OsString::from("機器-learning"),
        OsString::from("cafe\u{301}"),
        OsString::from_vec(b"bad\xffpath".to_vec()),
    ] {
        let p = PathBuf::from("/tmp").join(name);
        let mut app = App::new(Config::default(), AppState::default());
        app.set_workspaces(vec![Workspace {
            id: p.clone(),
            name: p.to_string_lossy().into(),
        }]);
        app.update(station::app::Action::Project(0));
        assert_eq!(app.workspace(), Some(p.as_path()));
    }
}
