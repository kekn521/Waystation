use std::fs;
use std::path::{Path, PathBuf};

use station::config::{Config, Paths};
use tempfile::TempDir;

/// Write `body` to a fresh config file and load it with the given home.
fn load_str(home: &Path, body: &str) -> anyhow::Result<(Config, TempDir)> {
    let tmp = TempDir::new().unwrap();
    let path = tmp.path().join("cfg/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, body).unwrap();
    let cfg = Config::load(&path, home)?;
    Ok((cfg, tmp))
}

fn xdg_or(env: &str, fallback: &Path, rest: &str) -> PathBuf {
    match std::env::var(env)
        .ok()
        .filter(|s| !s.is_empty() && Path::new(s).is_absolute())
    {
        Some(dir) => PathBuf::from(dir).join(rest),
        None => fallback.join(rest),
    }
}

#[test]
fn load_missing_file_returns_defaults_without_writing() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("code")).unwrap();
    let cfg_path = home.join(".config/station/config.toml");

    let cfg = Config::load(&cfg_path, &home).unwrap();

    assert_eq!(cfg.project_roots, vec![home.join("code")]);
    assert!(cfg.pinned_projects.is_empty()); // dotfiles does not exist
    assert!(cfg.editor.is_none());
    assert!(cfg.tools.is_empty());
    assert!(cfg.tunnels.is_empty());
    assert_eq!(cfg.theme.accent, "mauve");
    assert!(!cfg.theme.compact);
    assert!(!cfg_path.exists(), "load must not create a config file");
}

#[test]
fn defaults_include_paths_that_exist() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("code")).unwrap();
    fs::create_dir_all(home.join("dotfiles")).unwrap();

    let cfg = Config::defaults(&home);
    assert_eq!(cfg.project_roots, vec![home.join("code")]);
    assert_eq!(cfg.pinned_projects, vec![home.join("dotfiles")]);
}

#[test]
fn malformed_bytes_are_rejected_and_file_unchanged() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let cfg_path = home.join(".config/station/config.toml");
    let bytes: [u8; 8] = [0xff, 0xfe, b'x', 0x00, b'=', b'x', 0x80, 0x81];
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    fs::write(&cfg_path, bytes).unwrap();

    assert!(Config::load(&cfg_path, &home).is_err());
    assert_eq!(fs::read(&cfg_path).unwrap().as_slice(), &bytes[..]);
}

#[test]
fn literal_shell_looking_args_are_preserved() {
    let home = PathBuf::from("/home/tester");
    let (cfg, _tmp) = load_str(
        &home,
        r#"
[tools]
build = { program = "bash", args = ["-lc", "echo hi && rm -rf $HOME/target | grep -c x"] }
"#,
    )
    .unwrap();

    let tool = cfg.tools.get("build").expect("build tool present");
    assert_eq!(tool.program, "bash");
    assert_eq!(
        tool.args,
        vec![
            "-lc".to_string(),
            "echo hi && rm -rf $HOME/target | grep -c x".to_string()
        ]
    );
    // Nothing was split on shell metacharacters.
    assert_eq!(tool.args.len(), 2);
}

#[test]
fn legacy_tasks_are_ignored_and_valid_tunnel_still_loads() {
    let home = PathBuf::from("/home/tester");
    let (cfg, _tmp) = load_str(
        &home,
        r#"
[[tasks]]
id = "build"
label = "Build"
command = { program = "make" }

[[tunnels]]
id = "db"
host = "bastion.example"
local_port = 5432
remote_host = "db.internal"
remote_port = 5432
"#,
    )
    .unwrap();

    // Removed task config parses without error but never resurfaces, and the
    // serialized config no longer round-trips a tasks key.
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(
        !serialized.contains("tasks"),
        "serialized config must not carry tasks: {serialized}"
    );

    let tunnel = &cfg.tunnels[0];
    assert_eq!(tunnel.bind, "127.0.0.1");
}

#[test]
fn legacy_task_entries_are_ignored() {
    let home = PathBuf::from("/home/tester");
    let (cfg, _tmp) = load_str(
        &home,
        r#"
[[tasks]]
id = "dup"
label = "A"
command = { program = "make" }
[[tasks]]
id = "dup"
label = "B"
command = { program = "make" }
"#,
    )
    .unwrap();
    // Task ids are no longer validated because tasks are ignored entirely.
    let serialized = toml::to_string(&cfg).unwrap();
    assert!(!serialized.contains("dup"), "tasks must not be loaded");
}

#[test]
fn legacy_tasks_and_sidecar_are_preserved_verbatim() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(&home).unwrap();
    let cfg_path = home.join(".config/station/config.toml");
    fs::create_dir_all(cfg_path.parent().unwrap()).unwrap();
    let toml_bytes: &[u8] = b"theme.accent = \"blue\"\n\n[[tasks]]\nlabel = \"Legacy build\"\n";
    fs::write(&cfg_path, toml_bytes).unwrap();

    // Pre-removal sidecar directory: one malformed JSON file and one
    // well-formed one that now references a program the UI cannot run.
    let sidecar = cfg_path.with_extension("tasks");
    fs::create_dir_all(&sidecar).unwrap();
    let junk_bytes: &[u8] = b"{ this is not json at all \xff\xfe";
    fs::write(sidecar.join("junk.json"), junk_bytes).unwrap();
    let recipe_bytes: &[u8] = br#"{"id":"old-legacy-id","label":"Old task","command":{"program":"make","args":[]},"cwd":".","required_ports":[]}"#;
    let old_path = sidecar.join("old.json");
    fs::write(&old_path, recipe_bytes).unwrap();

    // Load succeeds even though the sidecar is malformed and the legacy TOML
    // task has no id at all.
    let cfg = Config::load(&cfg_path, &home).unwrap();
    assert_eq!(cfg.theme.accent, "blue");
    assert!(cfg.tunnels.is_empty());

    // Nothing is deleted and nothing is rewritten.
    assert_eq!(fs::read(&cfg_path).unwrap().as_slice(), toml_bytes);
    assert_eq!(
        fs::read(sidecar.join("junk.json")).unwrap().as_slice(),
        junk_bytes
    );
    assert_eq!(fs::read(&old_path).unwrap().as_slice(), recipe_bytes);
}

#[test]
fn duplicate_tunnel_ids_rejected() {
    let home = PathBuf::from("/home/tester");
    let result = load_str(
        &home,
        r#"
[[tunnels]]
id = "dup"
host = "h"
local_port = 1
remote_host = "r"
remote_port = 2
[[tunnels]]
id = "dup"
host = "h"
local_port = 3
remote_host = "r"
remote_port = 4
"#,
    );
    assert!(result.is_err(), "duplicate tunnel ids must be rejected");
}

#[test]
fn invalid_accent_rejected() {
    for accent in ["green", "rosepine", "mauve-blue"] {
        let home = PathBuf::from("/home/tester");
        let result = load_str(&home, &format!("[theme]\naccent = \"{accent}\"\n"));
        assert!(result.is_err(), "accent {accent:?} must be rejected");
    }
}

#[test]
fn other_invalid_fields_rejected() {
    let home = PathBuf::from("/home/tester");
    let cases = [
        // empty editor program
        "editor = { program = \"\" }\n",
        // empty tool program
        "[tools]\nb = { program = \"\" }\n",
        // empty tunnel host
        "[[tunnels]]\nid = \"t\"\nhost = \"\"\nlocal_port = 1\nremote_host = \"r\"\nremote_port = 2\n",
        // empty tunnel remote host
        "[[tunnels]]\nid = \"t\"\nhost = \"h\"\nlocal_port = 1\nremote_host = \"\"\nremote_port = 2\n",
        // local port 0
        "[[tunnels]]\nid = \"t\"\nhost = \"h\"\nlocal_port = 0\nremote_host = \"r\"\nremote_port = 2\n",
        // remote port 0
        "[[tunnels]]\nid = \"t\"\nhost = \"h\"\nlocal_port = 1\nremote_host = \"r\"\nremote_port = 0\n",
        // control character in identifier
        "[[tunnels]]\nid = \"a\\u{7}b\"\nhost = \"h\"\nlocal_port = 1\nremote_host = \"r\"\nremote_port = 2\n",
    ];
    for body in cases {
        assert!(load_str(&home, body).is_err(), "should reject: {body}");
    }
}

#[test]
fn relative_and_tilde_paths_expanded() {
    let home = PathBuf::from("/home/tester");
    let (cfg, tmp) = load_str(
        &home,
        r#"
project_roots = ["relative/sub", "~/elsewhere"]
pinned_projects = ["pinned", "~/dots"]
"#,
    )
    .unwrap();

    let cfg_parent = tmp.path().join("cfg");
    assert_eq!(cfg.project_roots[0], cfg_parent.join("relative/sub"));
    assert_eq!(cfg.project_roots[1], home.join("elsewhere"));
    assert_eq!(cfg.pinned_projects[0], cfg_parent.join("pinned"));
    assert_eq!(cfg.pinned_projects[1], home.join("dots"));
}

#[test]
fn explicit_empty_project_roots_allowed_but_missing_key_defaults() {
    let tmp = TempDir::new().unwrap();
    let home = tmp.path().join("home");
    fs::create_dir_all(home.join("code")).unwrap();

    let path = tmp.path().join("a/config.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "project_roots = []\n").unwrap();
    let cfg = Config::load(&path, &home).unwrap();
    assert!(cfg.project_roots.is_empty(), "explicit empty must be kept");

    let path2 = tmp.path().join("b/config.toml");
    fs::create_dir_all(path2.parent().unwrap()).unwrap();
    fs::write(&path2, "").unwrap();
    let cfg2 = Config::load(&path2, &home).unwrap();
    assert_eq!(cfg2.project_roots, vec![home.join("code")]);
}

#[test]
fn paths_discover_follows_env_without_mutation() {
    // Read the environment; never modify it.
    let home = PathBuf::from(std::env::var("HOME").expect("HOME set in test env"));
    let p = Paths::discover().expect("discover works with a normal env");
    assert_eq!(p.home, home);
    assert_eq!(
        p.config,
        xdg_or(
            "XDG_CONFIG_HOME",
            &home.join(".config"),
            "station/config.toml"
        )
    );
    assert_eq!(
        p.state,
        xdg_or("XDG_STATE_HOME", &home.join(".local/state"), "station")
    );
}
#[test]
fn tunnel_arguments_are_discrete_and_loopback() {
    use station::config::{TaskRecipe, TunnelRecipe};
    let t = TunnelRecipe {
        id: "db".into(),
        host: "saved".into(),
        bind: "127.0.0.1".into(),
        local_port: 15432,
        remote_host: "127.0.0.1".into(),
        remote_port: 5432,
    };
    let r = TaskRecipe::from_tunnel(&t, "/tmp".into()).unwrap();
    assert!(r.command.args.contains(&"-N".into()));
    assert!(r.command.args.contains(&"ExitOnForwardFailure=yes".into()));
    assert!(
        r.command
            .args
            .contains(&"127.0.0.1:15432:127.0.0.1:5432".into())
    );
    let mut t = t;
    t.host = "-bad".into();
    assert!(TaskRecipe::from_tunnel(&t, "/tmp".into()).is_err());
}
#[test]
fn config_directory_is_an_error() {
    let d = tempfile::tempdir().unwrap();
    assert!(station::config::Config::load(d.path(), d.path()).is_err());
}
