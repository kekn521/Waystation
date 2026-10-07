use std::{path::Path, time::Duration};
use waystation::{
    agents::{AgentManager, AgentStatus, attach_message, attach_term},
    config::ToolCommand,
};

#[test]
fn sessions_survive_manager_restart_and_preserve_literal_arguments() {
    let d = tempfile::tempdir().unwrap();
    let manager = AgentManager::new(d.path().into());
    assert!(manager.list().unwrap().is_empty());
    let command = ToolCommand {
        program: "/usr/bin/printf".into(),
        args: vec!["%s".into(), "literal ; $(false) ' \"".into()],
    };
    let session = manager
        .create(
            "Review",
            "codex",
            &command,
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let other = manager
        .create(
            "Implementation",
            "claude",
            &ToolCommand {
                program: "/bin/sleep".into(),
                args: vec!["60".into()],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let reloaded = AgentManager::new(d.path().into());
    let mut sessions = vec![];
    for _ in 0..40 {
        sessions = reloaded.list().unwrap();
        if sessions
            .iter()
            .any(|s| s.id == session.id && s.status == AgentStatus::Exited(0))
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(sessions.len(), 2);
    assert!(
        sessions
            .iter()
            .any(|s| s.id == session.id && s.status == AgentStatus::Exited(0)),
        "{sessions:?}"
    );
    std::thread::sleep(Duration::from_millis(200));
    let captured = reloaded.capture(session.id).unwrap();
    assert!(
        captured.contains("literal ; $(false) ' \""),
        "captured: {captured:?}"
    );
    assert!(
        reloaded
            .attach(session.id, Path::new(env!("CARGO_BIN_EXE_waystation")))
            .is_ok()
    );
    reloaded.close(session.id).unwrap();
    assert_eq!(reloaded.list().unwrap()[0].id, other.id);
    reloaded.close(other.id).unwrap();
    assert!(reloaded.list().unwrap().is_empty());
}

#[test]
fn agent_sessions_enable_tmux_mouse_scrollback() {
    let d = tempfile::tempdir().unwrap();
    let manager = AgentManager::new(d.path().into());
    let first = manager
        .create(
            "Scrollable",
            "codex",
            &ToolCommand {
                program: "/bin/sleep".into(),
                args: vec!["60".into()],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let key: uuid::Uuid =
        serde_json::from_slice(&std::fs::read(d.path().join("agents/server.json")).unwrap())
            .unwrap();
    let socket = format!("station-{key}");
    let mouse = || {
        std::process::Command::new("tmux")
            .args(["-L", &socket, "show-options", "-gv", "mouse"])
            .output()
            .unwrap()
    };
    let fresh = mouse();
    assert!(
        std::process::Command::new("tmux")
            .args(["-L", &socket, "set-option", "-g", "mouse", "off"])
            .status()
            .unwrap()
            .success()
    );
    let second = manager
        .create(
            "Also scrollable",
            "codex",
            &ToolCommand {
                program: "/bin/sleep".into(),
                args: vec!["60".into()],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let existing = mouse();

    manager.close(first.id).unwrap();
    manager.close(second.id).unwrap();
    assert!(fresh.status.success(), "{fresh:?}");
    assert_eq!(String::from_utf8_lossy(&fresh.stdout).trim(), "on");
    assert!(existing.status.success(), "{existing:?}");
    assert_eq!(String::from_utf8_lossy(&existing.stdout).trim(), "on");
}

#[test]
fn rejects_bad_workspace_or_missing_command_without_creating_session() {
    let d = tempfile::tempdir().unwrap();
    let manager = AgentManager::new(d.path().into());
    let command = ToolCommand {
        program: "station-no-such-program-xyz".into(),
        args: vec![],
    };
    assert!(
        manager
            .create(
                "Test",
                "codex",
                &command,
                d.path(),
                Path::new(env!("CARGO_BIN_EXE_waystation"))
            )
            .is_err()
    );
    assert!(manager.list().unwrap().is_empty());
}

#[test]
fn relative_executable_in_non_utf8_workspace_is_lossless() {
    use std::os::unix::{ffi::OsStringExt, fs::PermissionsExt};
    let d = tempfile::tempdir().unwrap();
    let workspace = d
        .path()
        .join(std::ffi::OsString::from_vec(b"project-\xff".to_vec()));
    std::fs::create_dir(&workspace).unwrap();
    let helper = workspace.join("agent");
    std::fs::write(&helper, "#!/bin/sh\nprintf 'LOSSLESS_PATH_OK\\n'\n").unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let manager = AgentManager::new(d.path().into());
    let session = manager
        .create(
            "Review",
            "codex",
            &ToolCommand {
                program: "./agent".into(),
                args: vec![],
            },
            &workspace,
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let mut output = String::new();
    for _ in 0..40 {
        output = manager.capture(session.id).unwrap();
        if output.contains("LOSSLESS_PATH_OK") {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    manager.close(session.id).unwrap();
    assert!(output.contains("LOSSLESS_PATH_OK"), "{output:?}");
}

#[test]
fn lost_server_leaves_recoverable_records_and_can_be_closed() {
    let d = tempfile::tempdir().unwrap();
    let manager = AgentManager::new(d.path().into());
    let session = manager
        .create(
            "Review",
            "codex",
            &ToolCommand {
                program: "/bin/sleep".into(),
                args: vec!["60".into()],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let key: uuid::Uuid =
        serde_json::from_slice(&std::fs::read(d.path().join("agents/server.json")).unwrap())
            .unwrap();
    assert!(
        std::process::Command::new("tmux")
            .args(["-L", &format!("station-{key}"), "kill-server"])
            .status()
            .unwrap()
            .success()
    );
    for _ in 0..40 {
        if manager
            .list()
            .is_ok_and(|sessions| sessions[0].status == AgentStatus::Unavailable)
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(manager.list().unwrap()[0].status, AgentStatus::Unavailable);
    assert!(
        manager
            .attach(session.id, Path::new(env!("CARGO_BIN_EXE_waystation")))
            .is_err()
    );
    manager.close(session.id).unwrap();
    assert!(manager.list().unwrap().is_empty());
}

fn stand_in(dir: &Path, name: &str) -> String {
    use std::os::unix::fs::PermissionsExt;
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let path = bin.join(name);
    // Records its exact argv beside itself, replacing the previous launch's.
    std::fs::write(
        &path,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$0.args.tmp\" && mv \"$0.args.tmp\" \"$0.args\"\nexec sleep 60\n",
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path.to_str().unwrap().into()
}

fn wait_for_args(program: &str, expected: &[String]) -> Vec<String> {
    let path = format!("{program}.args");
    let mut args = vec![];
    for _ in 0..60 {
        args = std::fs::read_to_string(&path)
            .map(|s| s.lines().map(String::from).collect())
            .unwrap_or_default();
        if args == expected {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_file(&path);
    args
}

fn claude_settings() -> String {
    let command = format!(
        "'{}' __agent-hook",
        Path::new(env!("CARGO_BIN_EXE_waystation")).display()
    );
    let group =
        serde_json::json!([{"hooks": [{"type": "command", "command": command, "timeout": 10}]}]);
    let mut hooks = serde_json::Map::new();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "PostToolUseFailure",
        "PermissionRequest",
        "Notification",
        "Stop",
    ] {
        hooks.insert(event.into(), group.clone());
    }
    serde_json::json!({ "hooks": hooks }).to_string()
}

/// Codex reads hooks from `-c`, which also runs it embedded so hooks see the pane's env.
fn codex_hook_overrides() -> Vec<String> {
    let exe = Path::new(env!("CARGO_BIN_EXE_waystation"))
        .display()
        .to_string();
    [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PermissionRequest",
        "PostToolUse",
        "Stop",
        "Interrupt",
    ]
    .iter()
    .flat_map(|event| {
        [
            "-c".to_string(),
            format!(
                r#"hooks.{event}=[{{hooks=[{{type="command",command="'{exe}' __agent-hook",timeout=10}}]}}]"#
            ),
        ]
    })
    .collect()
}

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

fn kill_server(state: &Path) {
    let key: uuid::Uuid =
        serde_json::from_slice(&std::fs::read(state.join("agents/server.json")).unwrap()).unwrap();
    assert!(
        std::process::Command::new("tmux")
            .args(["-L", &format!("station-{key}"), "kill-server"])
            .status()
            .unwrap()
            .success()
    );
}

fn wait_for_status(
    manager: &AgentManager,
    status: AgentStatus,
) -> Vec<waystation::agents::AgentSession> {
    let mut sessions = vec![];
    for _ in 0..60 {
        sessions = manager.list().unwrap();
        if sessions.iter().all(|s| s.status == status) {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    sessions
}

#[test]
fn claude_sessions_resume_their_conversation_after_server_loss() {
    let d = tempfile::tempdir().unwrap();
    let claude_home = d.path().join("claude-home");
    let manager = AgentManager::new(d.path().join("state"))
        .with_agent_homes(claude_home.clone(), d.path().join("codex-home"));
    let program = stand_in(d.path(), "claude");
    let command = ToolCommand {
        program: program.clone(),
        args: vec!["--model".into(), "opus".into()],
    };
    let session = manager
        .create(
            "Plan",
            "claude",
            &command,
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let conversation = session
        .conversation
        .clone()
        .expect("claude conversation id");
    let settings = claude_settings();
    let fresh = strings(&[
        "--model",
        "opus",
        "--settings",
        &settings,
        "--session-id",
        &conversation,
    ]);
    assert_eq!(wait_for_args(&program, &fresh), fresh);

    // No transcript was written yet, so reopening starts the same conversation id afresh.
    kill_server(&d.path().join("state"));
    let sessions = wait_for_status(&manager, AgentStatus::Saved);
    assert_eq!(sessions[0].status, AgentStatus::Saved, "{sessions:?}");
    assert!(
        manager
            .attach(session.id, Path::new(env!("CARGO_BIN_EXE_waystation")))
            .is_ok()
    );
    assert_eq!(manager.list().unwrap()[0].status, AgentStatus::Running);
    assert_eq!(wait_for_args(&program, &fresh), fresh);

    let project = claude_home.join("projects/-tmp-plan");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(format!("{conversation}.jsonl")), "{}\n").unwrap();
    kill_server(&d.path().join("state"));
    wait_for_status(&manager, AgentStatus::Saved);
    assert!(
        manager
            .attach(session.id, Path::new(env!("CARGO_BIN_EXE_waystation")))
            .is_ok()
    );
    let resumed = strings(&[
        "--model",
        "opus",
        "--settings",
        &settings,
        "--resume",
        &conversation,
    ]);
    let args = wait_for_args(&program, &resumed);
    manager.close(session.id).unwrap();
    assert_eq!(args, resumed);
}

/// Runs the hook as Claude or Codex would, with conversation homes under `homes`.
fn run_hook(
    homes: &Path,
    manifest: Option<&Path>,
    pid: Option<u32>,
    input: &str,
) -> std::process::Output {
    use std::io::Write;
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_waystation"));
    command
        .arg("__agent-hook")
        .env("CLAUDE_CONFIG_DIR", homes.join("claude-home"))
        .env("CODEX_HOME", homes.join("codex-home"))
        .env_remove("WAYSTATION_AGENT_MANIFEST")
        .env_remove("WAYSTATION_AGENT_PID")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    if let Some(manifest) = manifest {
        command.env("WAYSTATION_AGENT_MANIFEST", manifest);
    }
    if let Some(pid) = pid {
        command.env("WAYSTATION_AGENT_PID", pid.to_string());
    }
    let mut child = command.spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn hook_records_conversation_only_for_its_own_agent() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let manager = AgentManager::new(state.clone())
        .with_agent_homes(d.path().join("claude-home"), d.path().join("codex-home"));
    let program = stand_in(d.path(), "claude");
    let session = manager
        .create(
            "Plan",
            "claude",
            &ToolCommand {
                program: program.clone(),
                args: vec![],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let launched = strings(&[
        "--settings",
        &claude_settings(),
        "--session-id",
        session.conversation.as_deref().unwrap(),
    ]);
    assert_eq!(wait_for_args(&program, &launched), launched);

    let manifest = state.join(format!("agents/{}.json", session.id));
    let input = |id: &str, transcript: &Path| {
        serde_json::json!({
            "hook_event_name": "SessionStart",
            "source": "clear",
            "session_id": id,
            "transcript_path": transcript.join(format!("{id}.jsonl")),
        })
        .to_string()
    };
    let claude_projects = d.path().join("claude-home/projects/-work");
    let clear = input("after-clear-1", &claude_projects);
    let conversation = || manager.list().unwrap()[0].conversation.clone();
    let me = Some(std::process::id());
    let mut stranger = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();

    // Outside Waystation, from another process, or from a different agent's conversation,
    // the hook stays silent and changes nothing.
    let before = conversation();
    for (manifest, pid, input) in [
        (None, me, clear.clone()),
        (Some(manifest.as_path()), None, clear.clone()),
        (Some(manifest.as_path()), Some(stranger.id()), clear.clone()),
        (
            Some(manifest.as_path()),
            me,
            input("bad id; rm -rf", &claude_projects),
        ),
        (
            Some(manifest.as_path()),
            me,
            input(
                "codex-thread",
                &d.path().join("codex-home/sessions/2026/10/06"),
            ),
        ),
        (Some(manifest.as_path()), me, "not json".into()),
    ] {
        let out = run_hook(d.path(), manifest, pid, &input);
        assert!(out.status.success(), "{out:?}");
        assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
        assert_eq!(conversation(), before, "{input}");
    }
    stranger.kill().unwrap();
    let _ = stranger.wait();

    let out = run_hook(d.path(), Some(&manifest), me, &clear);
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    assert_eq!(conversation().as_deref(), Some("after-clear-1"));

    manager.close(session.id).unwrap();
    let out = run_hook(d.path(), Some(&manifest), me, &clear);
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    assert!(!manifest.exists());
}

#[test]
fn hook_ignores_a_nested_copy_of_the_same_agent() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let manager = AgentManager::new(state.clone())
        .with_agent_homes(d.path().join("claude-home"), d.path().join("codex-home"));
    let session = manager
        .create(
            "Plan",
            "claude",
            &ToolCommand {
                program: stand_in(d.path(), "claude"),
                args: vec![],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let manifest = state.join(format!("agents/{}.json", session.id));
    let input = |id: &str| {
        serde_json::json!({
            "hook_event_name": "SessionStart",
            "session_id": id,
            "transcript_path": d.path().join(format!("claude-home/projects/-work/{id}.jsonl")),
        })
        .to_string()
    };
    // `sh` stands in for the agent: it reports through a hook it runs itself, or through a
    // second `sh` (the same program, as a nested agent would be) started beneath it.
    let run = |script: &str, id: &str| {
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("export WAYSTATION_AGENT_PID=$$; {script}; true"))
            .env("WAYSTATION_AGENT_MANIFEST", &manifest)
            .env("CLAUDE_CONFIG_DIR", d.path().join("claude-home"))
            .env("CODEX_HOME", d.path().join("codex-home"))
            .env("HOOK", env!("CARGO_BIN_EXE_waystation"))
            .env("IN", input(id))
            .output()
            .unwrap();
        assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    };
    run(
        r#"sh -c 'printf %s "$IN" | "$HOOK" __agent-hook; true'"#,
        "nested-1",
    );
    assert_eq!(
        manager.list().unwrap()[0].conversation,
        session.conversation
    );
    run(r#"printf %s "$IN" | "$HOOK" __agent-hook"#, "direct-1");
    let recorded = manager.list().unwrap()[0].conversation.clone();
    manager.close(session.id).unwrap();
    assert_eq!(recorded.as_deref(), Some("direct-1"));
}

#[test]
fn codex_sessions_resume_the_conversation_their_hook_reported() {
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let manager = AgentManager::new(state.clone())
        .with_agent_homes(d.path().join("claude-home"), d.path().join("codex-home"));
    let program = stand_in(d.path(), "codex");
    let session = manager
        .create(
            "Build",
            "codex",
            &ToolCommand {
                program: program.clone(),
                args: vec!["--model".into(), "o3".into()],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    assert!(session.conversation.is_none());
    let hooks = codex_hook_overrides();
    let fresh = [hooks.clone(), strings(&["--model", "o3"])].concat();
    assert_eq!(wait_for_args(&program, &fresh), fresh);

    let manifest = state.join(format!("agents/{}.json", session.id));
    let out = run_hook(
        d.path(),
        Some(&manifest),
        Some(std::process::id()),
        &serde_json::json!({
            "hook_event_name": "SessionStart",
            "source": "startup",
            "session_id": "019a0000-aaaa-7000-8000-000000000001",
            "transcript_path": d.path().join("codex-home/sessions/2026/10/06/rollout-x.jsonl"),
        })
        .to_string(),
    );
    assert!(out.status.success(), "{out:?}");
    kill_server(&state);
    let sessions = wait_for_status(&manager, AgentStatus::Saved);
    assert_eq!(sessions[0].status, AgentStatus::Saved, "{sessions:?}");
    assert!(
        manager
            .attach(session.id, Path::new(env!("CARGO_BIN_EXE_waystation")))
            .is_ok()
    );
    let resumed = [
        strings(&["resume"]),
        hooks,
        strings(&["--model", "o3", "019a0000-aaaa-7000-8000-000000000001"]),
    ]
    .concat();
    let args = wait_for_args(&program, &resumed);
    manager.close(session.id).unwrap();
    assert_eq!(args, resumed);
}

/// A terminfo directory holding the named entries, laid out as ncurses reads them.
fn terminfo(names: &[&str]) -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    for name in names {
        let dir = d.path().join(&name[..1]);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(name), b"compiled").unwrap();
    }
    d
}

#[test]
fn attach_keeps_a_term_that_has_a_terminfo_entry() {
    let d = terminfo(&["xterm-ghostty", "ghostty", "xterm-256color"]);
    let dirs = [d.path().to_path_buf()];
    assert_eq!(attach_term("xterm-ghostty".as_ref(), &dirs), None);
}

#[test]
fn attach_maps_ghostty_to_the_entry_ncurses_ships() {
    // ncurses names Ghostty's entry `ghostty`; Ghostty itself sets TERM=xterm-ghostty.
    let d = terminfo(&["ghostty", "xterm-256color"]);
    let dirs = [d.path().to_path_buf()];
    assert_eq!(
        attach_term("xterm-ghostty".as_ref(), &dirs),
        Some("ghostty".into())
    );
}

#[test]
fn attach_falls_back_to_xterm_256color_for_unknown_terminals() {
    let d = terminfo(&["xterm-256color"]);
    let dirs = [d.path().to_path_buf()];
    assert_eq!(
        attach_term("xterm-ghostty".as_ref(), &dirs),
        Some("xterm-256color".into())
    );
    assert_eq!(
        attach_term("wezterm".as_ref(), &dirs),
        Some("xterm-256color".into())
    );
}

#[test]
fn attach_searches_every_terminfo_directory() {
    let user = terminfo(&["xterm-ghostty"]);
    let system = terminfo(&["xterm-256color"]);
    let dirs = [system.path().to_path_buf(), user.path().to_path_buf()];
    assert_eq!(attach_term("xterm-ghostty".as_ref(), &dirs), None);
}

#[test]
fn attach_leaves_term_alone_when_no_substitute_exists() {
    let d = terminfo(&[]);
    let dirs = [d.path().to_path_buf()];
    assert_eq!(attach_term("xterm-ghostty".as_ref(), &dirs), None);
    assert_eq!(attach_term("".as_ref(), &dirs), None);
}

#[test]
fn attach_failure_message_names_the_tmux_error() {
    use std::os::unix::process::ExitStatusExt;
    let failed = std::process::ExitStatus::from_raw(1 << 8);
    assert_eq!(
        attach_message(failed, "missing or unsuitable terminal: xterm-ghostty\n"),
        "Agent attachment ended: missing or unsuitable terminal: xterm-ghostty"
    );
    assert_eq!(
        attach_message(failed, "  \n"),
        "Agent attachment ended: exit status: 1"
    );
    assert_eq!(
        attach_message(std::process::ExitStatus::from_raw(0), ""),
        "Back at Waystation · agent sessions stay available"
    );
}

#[test]
fn attaching_binds_ctrl_backslash_for_keyboards_without_f12() {
    let d = tempfile::tempdir().unwrap();
    let manager = AgentManager::new(d.path().into());
    let launcher = Path::new(env!("CARGO_BIN_EXE_waystation"));
    let session = manager
        .create(
            "Mac",
            "codex",
            &ToolCommand {
                program: "/bin/sleep".into(),
                args: vec!["60".into()],
            },
            d.path(),
            launcher,
        )
        .unwrap();
    let key: uuid::Uuid =
        serde_json::from_slice(&std::fs::read(d.path().join("agents/server.json")).unwrap())
            .unwrap();
    let socket = format!("station-{key}");
    let tmux = |args: &[&str]| {
        std::process::Command::new("tmux")
            .args(["-L", &socket])
            .args(args)
            .output()
            .unwrap()
    };
    // A server started by an older Waystation, before the binding existed.
    assert!(tmux(&["unbind-key", "-n", "C-\\"]).status.success());
    manager.attach(session.id, launcher).unwrap();
    let keys = tmux(&["list-keys", "-T", "root"]);
    manager.close(session.id).unwrap();
    let _ = tmux(&["kill-server"]);
    let keys = String::from_utf8_lossy(&keys.stdout);
    assert!(
        keys.lines()
            .any(|l| l.contains("C-\\") && l.contains("detach-client")),
        "{keys}"
    );
    assert!(
        keys.lines()
            .any(|l| l.contains("F12") && l.contains("detach-client")),
        "{keys}"
    );
}

#[test]
fn hook_events_record_what_the_agent_is_doing() {
    use waystation::activity::{ActivityState, read};
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let manager = AgentManager::new(state.clone())
        .with_agent_homes(d.path().join("claude-home"), d.path().join("codex-home"));
    let session = manager
        .create(
            "Plan",
            "claude",
            &ToolCommand {
                program: stand_in(d.path(), "claude"),
                args: vec![],
            },
            d.path(),
            Path::new(env!("CARGO_BIN_EXE_waystation")),
        )
        .unwrap();
    let manifest = state.join(format!("agents/{}.json", session.id));
    let me = Some(std::process::id());
    let transcript = d.path().join("claude-home/projects/-work/s.jsonl");
    let send = |pid: Option<u32>, event: serde_json::Value| {
        let out = run_hook(d.path(), Some(&manifest), pid, &event.to_string());
        assert!(
            out.status.success() && out.stdout.is_empty() && out.stderr.is_empty(),
            "{out:?}"
        );
    };
    // Launching marks the session ready before any hook fires.
    assert_eq!(read(&manifest).unwrap().state, ActivityState::Ready);

    send(
        me,
        serde_json::json!({"hook_event_name": "PreToolUse", "session_id": "s",
        "transcript_path": transcript, "tool_name": "Bash", "tool_input": {"command": "cargo test"}}),
    );
    let activity = read(&manifest).expect("recorded");
    assert_eq!(
        (activity.state, activity.detail.as_str()),
        (ActivityState::Running, "$ cargo test")
    );
    // The conversation id only changes on SessionStart.
    assert_eq!(
        manager.list().unwrap()[0].conversation,
        session.conversation
    );

    // Claude's notifications may carry no transcript path; the process check still applies.
    send(
        me,
        serde_json::json!({"hook_event_name": "Notification", "session_id": "s",
        "notification_type": "permission_prompt", "message": "Claude needs your permission to use Bash"}),
    );
    assert_eq!(read(&manifest).unwrap().state, ActivityState::NeedsInput);

    let mut stranger = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    send(
        Some(stranger.id()),
        serde_json::json!({"hook_event_name": "Stop", "session_id": "s",
        "transcript_path": transcript, "last_assistant_message": "not mine"}),
    );
    stranger.kill().unwrap();
    let _ = stranger.wait();
    assert_eq!(read(&manifest).unwrap().state, ActivityState::NeedsInput);

    manager.close(session.id).unwrap();
    assert!(read(&manifest).is_none(), "closing removes the activity");
    send(
        me,
        serde_json::json!({"hook_event_name": "Stop", "session_id": "s",
        "transcript_path": transcript, "last_assistant_message": "late"}),
    );
    assert!(
        read(&manifest).is_none(),
        "a closed session is never recreated"
    );
}

#[test]
fn launching_marks_the_agent_ready_and_restart_resumes_with_current_hooks() {
    use waystation::activity::{ActivityState, read};
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let claude_home = d.path().join("claude-home");
    let manager = AgentManager::new(state.clone())
        .with_agent_homes(claude_home.clone(), d.path().join("codex-home"));
    let launcher = Path::new(env!("CARGO_BIN_EXE_waystation"));
    let program = stand_in(d.path(), "claude");
    let session = manager
        .create(
            "Plan",
            "claude",
            &ToolCommand {
                program: program.clone(),
                args: vec![],
            },
            d.path(),
            launcher,
        )
        .unwrap();
    let manifest = state.join(format!("agents/{}.json", session.id));
    let conversation = session.conversation.clone().unwrap();
    let settings = claude_settings();
    let fresh = strings(&["--settings", &settings, "--session-id", &conversation]);
    assert_eq!(wait_for_args(&program, &fresh), fresh);
    assert_eq!(read(&manifest).unwrap().state, ActivityState::Ready);

    // A session that has run and has a transcript comes back on the same conversation.
    std::fs::write(waystation::activity::path(&manifest), "{}").unwrap();
    let project = claude_home.join("projects/-work");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join(format!("{conversation}.jsonl")), "{}\n").unwrap();
    manager.restart(session.id, launcher).unwrap();
    let resumed = strings(&["--settings", &settings, "--resume", &conversation]);
    assert_eq!(wait_for_args(&program, &resumed), resumed);
    assert_eq!(read(&manifest).unwrap().state, ActivityState::Ready);
    assert_eq!(manager.list().unwrap()[0].status, AgentStatus::Running);
    manager.close(session.id).unwrap();

    // A Claude session from before conversations were recorded gets one, starting fresh.
    let old = manager
        .create(
            "Old",
            "claude",
            &ToolCommand {
                program: program.clone(),
                args: vec![],
            },
            d.path(),
            launcher,
        )
        .unwrap();
    let old_manifest = state.join(format!("agents/{}.json", old.id));
    let mut record: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&old_manifest).unwrap()).unwrap();
    record.as_object_mut().unwrap().remove("conversation");
    std::fs::write(&old_manifest, record.to_string()).unwrap();
    manager.restart(old.id, launcher).unwrap();
    let assigned = manager.list().unwrap()[0]
        .conversation
        .clone()
        .expect("assigned");
    let started = strings(&["--settings", &settings, "--session-id", &assigned]);
    let args = wait_for_args(&program, &started);
    manager.close(old.id).unwrap();
    assert_eq!(args, started);
}
