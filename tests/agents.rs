use station::{
    agents::{AgentManager, AgentStatus},
    config::ToolCommand,
};
use std::{path::Path, time::Duration};

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
            Path::new(env!("CARGO_BIN_EXE_station")),
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
            Path::new(env!("CARGO_BIN_EXE_station")),
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
    assert!(reloaded.attach(session.id).is_ok());
    reloaded.close(session.id).unwrap();
    assert_eq!(reloaded.list().unwrap()[0].id, other.id);
    reloaded.close(other.id).unwrap();
    assert!(reloaded.list().unwrap().is_empty());
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
                Path::new(env!("CARGO_BIN_EXE_station"))
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
            Path::new(env!("CARGO_BIN_EXE_station")),
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
            Path::new(env!("CARGO_BIN_EXE_station")),
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
    assert!(manager.attach(session.id).is_err());
    manager.close(session.id).unwrap();
    assert!(manager.list().unwrap().is_empty());
}
