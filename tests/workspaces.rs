use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};

use waystation::config::Config;
use waystation::providers::git::{parse_status, parse_worktrees};
use waystation::providers::projects::discover;

/// Create `dir` and drop an empty marker file `name` inside it.
fn marker(dir: &Path, name: &str) {
    fs::create_dir_all(dir).unwrap();
    fs::write(dir.join(name), b"").unwrap();
}

fn config_with(roots: Vec<PathBuf>, pins: Vec<PathBuf>) -> Config {
    Config {
        project_roots: roots,
        pinned_projects: pins,
        ..Config::default()
    }
}

fn names(workspaces: &[waystation::providers::projects::Workspace]) -> Vec<&str> {
    workspaces.iter().map(|w| w.name.as_str()).collect()
}

#[test]
fn depth_two_workspaces_are_found_but_depth_three_is_not() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    // depth 1
    marker(&root.join("proj-a"), "Cargo.toml");
    // plain grouping directory, itself no workspace
    marker(&root.join("group").join("proj-b"), "package.json");
    // depth 3 - out of reach
    marker(
        &root.join("group").join("deeper").join("proj-c"),
        "pyproject.toml",
    );
    // case-insensitive ordering puts Zeta last, after proj-*
    marker(&root.join("Zeta"), "Makefile");

    let found = discover(&config_with(vec![root.to_path_buf()], vec![])).unwrap();
    assert_eq!(names(&found), vec!["proj-a", "proj-b", "Zeta"]);
    for ws in &found {
        assert!(ws.id.is_absolute());
    }
}

#[test]
fn root_itself_qualifies_when_it_has_a_marker() {
    let root = tempfile::tempdir().unwrap();
    marker(root.path(), ".git");
    // a child marker must not produce a second entry under an eligible root
    marker(&root.path().join("inner"), "Cargo.toml");

    let found = discover(&config_with(vec![root.path().to_path_buf()], vec![])).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].id, fs::canonicalize(root.path()).unwrap());
}

#[test]
fn excluded_dependency_directories_are_never_scanned() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let app = root.join("app");
    for junk in [
        "node_modules",
        "target",
        "dist",
        "build",
        "venv",
        "__pycache__",
        ".hidden",
    ] {
        marker(&app.join(junk).join("pkg"), "Cargo.toml");
    }

    let found = discover(&config_with(vec![root.to_path_buf()], vec![])).unwrap();
    assert!(found.is_empty(), "discovered {found:?}");
}

#[test]
fn symlinked_pin_deduplicates_with_discovery() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    let real = root.join("alpha");
    marker(&real, ".git");
    std::os::unix::fs::symlink(&real, root.join("alpha-link")).unwrap();

    // The pin points through the symlink; the scan finds the real directory.
    let found = discover(&config_with(
        vec![root.to_path_buf()],
        vec![root.join("alpha-link")],
    ))
    .unwrap();
    assert_eq!(found.len(), 1, "got {found:?}");
    assert_eq!(found[0].id, fs::canonicalize(&real).unwrap());
    assert_eq!(found[0].name, "alpha");
}

#[test]
fn missing_pin_is_retained_by_original_path() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    marker(&root.join("alpha"), "Cargo.toml");
    let ghost = root.join("ghost");

    let found = discover(&config_with(vec![root.to_path_buf()], vec![ghost.clone()])).unwrap();
    assert_eq!(names(&found), vec!["ghost", "alpha"]);
    // a pin that cannot be canonicalized keeps the path exactly as configured
    assert_eq!(found[0].id, ghost);
}

#[test]
fn unicode_workspace_name_survives() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path();
    marker(&root.join("機器-learning"), "Cargo.toml");

    let found = discover(&config_with(vec![root.to_path_buf()], vec![])).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "機器-learning");
    assert_eq!(
        found[0].id,
        fs::canonicalize(root.join("機器-learning")).unwrap()
    );
}

#[test]
fn mixed_porcelain_status_counts_each_entry_once() {
    // Real git 2.53 `status --porcelain=v2 --branch -z` shape: headers are
    // `# ...`; the rename entry `2` carries the new path in its own field and
    // the preimage in the *next* NUL-terminated field. The preimage here is a
    // file literally named `2`, so a parser that forgets to consume it also
    // double-counts it as an entry.
    let fixture = b"# branch.oid 1111111111111111111111111111111111111111\0\
                    # branch.head main\0\
                    # branch.ab +2 -1\0\
                    1 .M N... 100644 100644 100644 aaaaaaa bbbbbbb file.txt\0\
                    2 R. N... 100644 100644 100644 ccccccc ddddddd R100 two\0\
                    2\0\
                    ? un tracked.txt\0\
                    u UU N... 100644 100644 100644 100644 eeeeeee fffffff ggggggg both.txt\0";

    let state = parse_status(fixture);
    assert_eq!(state.branch.as_deref(), Some("main"));
    assert_eq!(state.ahead, 2);
    assert_eq!(state.behind, 1);
    assert_eq!(state.changed, 4, "rename must count once: {state:?}");
}

#[test]
fn detached_head_reports_detached_branch() {
    let fixture = b"# branch.oid 2222222222222222222222222222222222222222\0\
                    # branch.head (detached)\0\
                    ? un tracked.txt\0";
    let state = parse_status(fixture);
    assert_eq!(state.branch.as_deref(), Some("(detached)"));
    assert_eq!(state.changed, 1);
    assert_eq!(state.ahead, 0);
    assert_eq!(state.behind, 0);
}

#[test]
fn ahead_and_behind_parse_with_defaults_when_absent() {
    let state = parse_status(
        b"# branch.oid 3333333333333333333333333333333333333333\0\
          # branch.head main\0\
          # branch.upstream origin/main\0\
          # branch.ab +0 -5\0",
    );
    assert_eq!((state.ahead, state.behind), (0, 5));

    let bare = parse_status(
        b"# branch.oid 4444444444444444444444444444444444444444\0\
          # branch.head main\0",
    );
    assert_eq!(bare.branch.as_deref(), Some("main"));
    assert_eq!((bare.ahead, bare.behind, bare.changed), (0, 0, 0));
    assert_eq!(parse_status(b"").branch, None);
}

#[test]
fn worktree_list_preserves_non_utf8_paths() {
    let fixture = b"worktree /repo/main\0\
                    HEAD 5555555555555555555555555555555555555555\0\
                    branch refs/heads/main\0\
                    \0\
                    worktree /repo/bad\xffpath\0\
                    HEAD 6666666666666666666666666666666666666666\0\
                    \0";
    let worktrees = parse_worktrees(fixture);
    assert_eq!(worktrees.len(), 2);
    assert_eq!(worktrees[0], PathBuf::from("/repo/main"));
    // the stray 0xFF byte must survive round-trip, not be replaced
    assert_eq!(
        worktrees[1],
        PathBuf::from(OsString::from_vec(b"/repo/bad\xffpath".to_vec()))
    );
    assert!(parse_worktrees(b"").is_empty());
}
