use waystation::{
    app::Action,
    providers::files,
    search::{SearchItem, rank},
};
#[test]
fn files_stay_in_workspace() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("機器.txt"), b"a").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("escape")).unwrap();
    assert_eq!(
        files::list(root.path(), root.path(), false)
            .unwrap()
            .iter()
            .filter(|e| e.label == "機器.txt")
            .count(),
        1
    );
    assert!(files::list(root.path(), outside.path(), false).is_err());
    assert!(files::list(root.path(), &root.path().join("escape"), false).is_err());
}
#[test]
fn fuzzy_search_prefers_exact_match() {
    let items = ["Headless runner", "Herdr", "help directory"].map(|s| SearchItem {
        id: s.into(),
        label: s.into(),
        detail: String::new(),
        action: Action::Help,
    });
    assert_eq!(rank("herdr", &items).first(), Some(&1));
    assert!(rank("hrdr", &items).contains(&1));
    assert!(rank("zzzz", &items).is_empty());
}
