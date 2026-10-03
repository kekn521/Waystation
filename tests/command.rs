use station::runtime::command::{CommandRunner, CommandSpec};
use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
fn spec(program: &str, args: &[&str], cwd: &std::path::Path) -> CommandSpec {
    CommandSpec {
        program: program.into(),
        args: args.iter().map(OsString::from).collect(),
        cwd: cwd.into(),
    }
}
#[test]
fn literal_arguments_are_not_evaluated() {
    let d = tempfile::tempdir().unwrap();
    let out = CommandRunner
        .capture(
            &spec("printf", &["%s", "$(touch sentinel); *"], d.path()),
            Duration::from_secs(2),
            4096,
        )
        .unwrap();
    assert_eq!(out.stdout, b"$(touch sentinel); *");
    assert!(!d.path().join("sentinel").exists());
}
#[test]
fn both_streams_are_drained_and_capped() {
    let d = tempfile::tempdir().unwrap();
    let out=CommandRunner.capture(&spec("sh",&["-c","i=0; while [ $i -lt 5000 ]; do echo abcdefghij; echo abcdefghij >&2; i=$((i+1)); done"],d.path()),Duration::from_secs(3),1024).unwrap();
    assert!(out.status.success());
    assert_eq!(out.stdout.len(), 1024);
    assert_eq!(out.stderr.len(), 1024);
    assert!(out.truncated);
}
#[test]
fn inherited_pipe_does_not_hang() {
    let d = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let err = CommandRunner
        .capture(
            &spec("sh", &["-c", "sleep 30 & wait"], d.path()),
            Duration::from_millis(150),
            1024,
        )
        .err()
        .unwrap();
    assert!(err.to_string().contains("timed out"));
    assert!(start.elapsed() < Duration::from_secs(3));
}
