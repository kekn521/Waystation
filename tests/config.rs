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
    assert!(cfg.tasks.is_empty());
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
fn task_defaults_and_full_round_trip() {
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

    let task = &cfg.tasks[0];
    assert_eq!(task.command.args, Vec::<String>::new());
    assert_eq!(task.cwd, PathBuf::from(".")); // stays relative
    assert!(task.required_ports.is_empty());

    let tunnel = &cfg.tunnels[0];
    assert_eq!(tunnel.bind, "127.0.0.1");
}

#[test]
fn duplicate_task_ids_rejected() {
    let home = PathBuf::from("/home/tester");
    let result = load_str(
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
    );
    assert!(result.is_err(), "duplicate task ids must be rejected");
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
        // empty task id
        "[[tasks]]\nid = \"\"\nlabel = \"x\"\ncommand = { program = \"make\" }\n",
        // empty task program
        "[[tasks]]\nid = \"t\"\nlabel = \"x\"\ncommand = { program = \"\" }\n",
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
        "[[tasks]]\nid = \"a\\u{7}b\"\nlabel = \"x\"\ncommand = { program = \"make\" }\n",
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

[[tasks]]
id = "t"
label = "T"
command = { program = "make" }
cwd = "~/ws"
"#,
    )
    .unwrap();

    let cfg_parent = tmp.path().join("cfg");
    assert_eq!(cfg.project_roots[0], cfg_parent.join("relative/sub"));
    assert_eq!(cfg.project_roots[1], home.join("elsewhere"));
    assert_eq!(cfg.pinned_projects[0], cfg_parent.join("pinned"));
    assert_eq!(cfg.pinned_projects[1], home.join("dots"));
    assert_eq!(cfg.tasks[0].cwd, home.join("ws"));
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
