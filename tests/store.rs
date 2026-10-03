use station::{
    model::{ActivityEntry, ActivityKind, AppState},
    store::Store,
};
use std::{fs, time::SystemTime};
#[test]
fn state_round_trip_merges_concurrent_activity() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().into()).unwrap();
    let entry = |id: &str| ActivityEntry {
        id: id.into(),
        at: SystemTime::now(),
        workspace: None,
        kind: ActivityKind::Launch("hx".into()),
        outcome: "exited 0".into(),
    };
    let a = AppState {
        selected_workspace: Some("/tmp/project".into()),
        activity: vec![entry("a")],
        ..AppState::default()
    };
    store.merge(&a).unwrap();
    let mut b = AppState::default();
    b.activity.push(entry("b"));
    store.merge(&b).unwrap();
    let state = store.load().unwrap();
    assert_eq!(state.activity.len(), 2);
    assert_eq!(state.selected_workspace, a.selected_workspace);
    store.merge(&a).unwrap();
    assert_eq!(store.load().unwrap().activity.len(), 2);
}
#[test]
fn future_schema_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().into()).unwrap();
    let bytes =
        br#"{"schema_version":999,"selected_workspace":null,"recent_workspaces":[],"activity":[]}"#;
    fs::write(dir.path().join("state.json"), bytes).unwrap();
    assert!(store.load().is_err());
    assert!(store.merge(&AppState::default()).is_err());
    assert_eq!(fs::read(dir.path().join("state.json")).unwrap(), bytes);
}
