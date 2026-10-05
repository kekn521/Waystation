use waystation::{app::App, config::Config, model::AppState, providers::projects::Workspace};
#[test]
fn refresh_preserves_selected_path() {
    let mut app = App::new(Config::default(), AppState::default());
    let w = |name: &str| Workspace {
        id: format!("/tmp/{name}").into(),
        name: name.into(),
    };
    app.set_workspaces(vec![w("a"), w("b")]);
    assert_eq!(app.workspace().unwrap(), std::path::Path::new("/tmp/a"));
    app.update(waystation::app::Action::Project(1));
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
        app.update(waystation::app::Action::Project(0));
        assert_eq!(app.workspace(), Some(p.as_path()));
    }
}
#[test]
fn file_refresh_keeps_selected_path() {
    use waystation::{model::Section, providers::files::FileEntry, runtime::workers::*};
    let mut a = App::new(Config::default(), AppState::default());
    a.section = Section::Files;
    a.file_dir = Some("/tmp".into());
    let f = |n: &str| FileEntry {
        path: format!("/tmp/{n}").into(),
        label: n.into(),
        is_dir: false,
    };
    a.files = vec![f("b"), f("c")];
    a.selection = 1;
    a.apply_provider(ProviderEvent {
        request: ProviderRequest {
            id: ProviderId::Files,
            generation: 0,
            workspace: None,
            directory: Some("/tmp".into()),
            hidden: false,
        },
        payload: Ok(ProviderPayload::Files(vec![f("a"), f("b"), f("c")])),
    });
    assert_eq!(a.selection, 2);
}
#[test]
fn recent_projects_follow_pins_and_resume_selected_path() {
    let d = tempfile::tempdir().unwrap();
    let paths = ["pin", "alpha", "zebra"].map(|n| d.path().join(n));
    for p in &paths {
        std::fs::create_dir(p).unwrap();
    }
    let mut app = App::new(
        Config {
            pinned_projects: vec![paths[0].clone()],
            ..Default::default()
        },
        AppState {
            recent_workspaces: vec![paths[2].clone()],
            selected_workspace: Some(paths[2].clone()),
            ..Default::default()
        },
    );
    app.set_workspaces(
        paths
            .iter()
            .map(|p| Workspace {
                id: p.clone(),
                name: p.file_name().unwrap().to_string_lossy().into(),
            })
            .collect(),
    );
    assert_eq!(
        app.workspaces.iter().map(|w| &w.id).collect::<Vec<_>>(),
        vec![&paths[0], &paths[2], &paths[1]]
    );
    assert_eq!(app.selection, 1);
}
