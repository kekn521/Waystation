use std::{
    ffi::OsString,
    time::{Duration, Instant},
};
use waystation::runtime::command::{CommandRunner, CommandSpec};
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

#[test]
fn executable_replaced_while_running_resolves_to_the_new_file() {
    use waystation::runtime::command::live_executable;
    let d = tempfile::tempdir().unwrap();
    let installed = d.path().join("waystation");
    std::fs::write(&installed, b"new build").unwrap();
    // Linux names a running program's replaced binary "<path> (deleted)".
    assert_eq!(
        live_executable(d.path().join("waystation (deleted)")),
        installed
    );
    assert_eq!(live_executable(installed.clone()), installed);
    // A file really named like that is left alone.
    let odd = d.path().join("odd (deleted)");
    std::fs::write(&odd, b"").unwrap();
    assert_eq!(live_executable(odd.clone()), odd);
    let missing = d.path().join("gone");
    assert_eq!(live_executable(missing.clone()), missing);
}
