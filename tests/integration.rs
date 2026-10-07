use std::{
    fs,
    process::Command,
    time::{Duration, Instant},
};
use waystation::runtime::command::{CommandRunner, CommandSpec};
#[test]
fn fresh_environment_help_and_invalid_config_preservation() {
    let d = tempfile::tempdir().unwrap();
    let binary = env!("CARGO_BIN_EXE_waystation");
    let output = Command::new(binary)
        .arg("--help")
        .env("HOME", d.path())
        .env("XDG_CONFIG_HOME", d.path().join("config"))
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(!help.contains("__supervise"));
    assert!(help.contains("--state-dir"));
    let p = d.path().join("bad.toml");
    let content = b"project_roots = [broken";
    fs::write(&p, content).unwrap();
    let output = Command::new(binary)
        .arg("--config")
        .arg(&p)
        .env("HOME", d.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("parsing"));
    assert_eq!(fs::read(&p).unwrap(), content);
}
#[test]
fn relative_executable_uses_command_cwd() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("fixture");
    fs::write(&p, "#!/bin/sh\nprintf 'relative works'\n").unwrap();
    fs::set_permissions(p, fs::Permissions::from_mode(0o755)).unwrap();
    let spec = CommandSpec {
        program: "./fixture".into(),
        args: vec![],
        cwd: d.path().into(),
    };
    // A test forking in parallel can briefly hold the fixture open for writing, which makes
    // executing it fail with "Text file busy" until that child execs.
    let mut attempt = 0;
    let output = loop {
        match CommandRunner.capture(&spec, Duration::from_secs(2), 1024) {
            Err(e) if attempt < 20 && format!("{e:#}").contains("Text file busy") => {
                attempt += 1;
                std::thread::sleep(Duration::from_millis(10));
            }
            result => break result.unwrap(),
        }
    };
    assert_eq!(output.stdout, b"relative works");
}
#[test]
fn exited_leader_with_inherited_pipe_still_times_out() {
    let d = tempfile::tempdir().unwrap();
    let begin = Instant::now();
    let result = CommandRunner.capture(
        &CommandSpec {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), "sleep 30 & exit 0".into()],
            cwd: d.path().into(),
        },
        Duration::from_millis(200),
        1024,
    );
    assert!(result.is_err());
    assert!(begin.elapsed() < Duration::from_secs(3));
}
