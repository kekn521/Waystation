use station::{
    config::{TaskRecipe, ToolCommand},
    tasks::{RunStatus, TaskManager, identity},
};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn recipe(cwd: &Path, script: &str) -> TaskRecipe {
    TaskRecipe {
        id: "fixture".into(),
        label: "Fixture".into(),
        cwd: cwd.into(),
        required_ports: vec![],
        command: ToolCommand {
            program: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
        },
    }
}
fn wait(m: &TaskManager, id: uuid::Uuid, status: RunStatus) {
    let until = Instant::now() + Duration::from_secs(8);
    loop {
        let r = m.list().unwrap().into_iter().find(|r| r.id == id).unwrap();
        if r.status == status {
            return;
        }
        assert!(Instant::now() < until, "wanted {status:?}, got {r:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}
#[test]
fn durable_runs_and_owned_stop() {
    let d = tempfile::tempdir().unwrap();
    let m = TaskManager::with_executable(d.path().into(), env!("CARGO_BIN_EXE_station").into());
    let a = m
        .start(&recipe(d.path(), "echo hello; sleep .2; exit 0"))
        .unwrap();
    let b = m.start(&recipe(d.path(), "exit 7")).unwrap();
    assert_ne!(a, b);
    drop(m);
    let m = TaskManager::new(d.path().into());
    wait(&m, a, RunStatus::Passed);
    wait(&m, b, RunStatus::Failed);
    assert_eq!(
        m.list()
            .unwrap()
            .iter()
            .find(|r| r.id == b)
            .unwrap()
            .exit_code,
        Some(7)
    );
    let m = TaskManager::with_executable(d.path().into(), env!("CARGO_BIN_EXE_station").into());
    let c = m.start(&recipe(d.path(), "sleep 30")).unwrap();
    wait(&m, c, RunStatus::Running);
    m.request_stop(c).unwrap();
    wait(&m, c, RunStatus::Stopped);
    assert!(m.request_stop(c).is_err());
}
#[test]
fn identity_rejects_reboot_reuse_and_malformed_stat() {
    let d = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(d.path().join("sys/kernel/random")).unwrap();
    std::fs::write(d.path().join("sys/kernel/random/boot_id"), "boot-a").unwrap();
    std::fs::create_dir(d.path().join("12")).unwrap();
    std::fs::write(
        d.path().join("12/stat"),
        format!(
            "12 (name with ) spaces) S {} 555 0",
            vec!["0"; 18].join(" ")
        ),
    )
    .unwrap();
    let a = identity::read(d.path(), 12).unwrap();
    assert_eq!(a.start_ticks, 555);
    std::fs::write(d.path().join("sys/kernel/random/boot_id"), "boot-b").unwrap();
    assert_ne!(a, identity::read(d.path(), 12).unwrap());
    std::fs::write(d.path().join("12/stat"), "12 malformed").unwrap();
    assert!(identity::read(d.path(), 12).is_err());
}
#[test]
fn forged_saved_identity_never_stops_a_process() {
    let d = tempfile::tempdir().unwrap();
    let m = TaskManager::with_executable(d.path().into(), env!("CARGO_BIN_EXE_station").into());
    let id = m.start(&recipe(d.path(), "sleep 30")).unwrap();
    wait(&m, id, RunStatus::Running);
    let path = m.run_dir(id).join("record.json");
    let original = std::fs::read(&path).unwrap();
    let mut r: station::tasks::RunRecord = serde_json::from_slice(&original).unwrap();
    r.supervisor.as_mut().unwrap().start_ticks += 1;
    station::store::atomic_json(&path, &r).unwrap();
    assert_eq!(m.list().unwrap()[0].status, RunStatus::Unknown);
    assert!(m.request_stop(id).is_err());
    std::fs::write(path, original).unwrap();
    m.request_stop(id).unwrap();
    wait(&m, id, RunStatus::Stopped);
}
#[test]
fn concurrent_managers_keep_distinct_runs() {
    let d = tempfile::tempdir().unwrap();
    let m = TaskManager::with_executable(d.path().into(), env!("CARGO_BIN_EXE_station").into());
    let r = recipe(d.path(), "exit 0");
    let a = m.clone();
    let rr = r.clone();
    let thread = std::thread::spawn(move || a.start(&rr).unwrap());
    let b = m.start(&r).unwrap();
    let a = thread.join().unwrap();
    assert_ne!(a, b);
    wait(&m, a, RunStatus::Passed);
    wait(&m, b, RunStatus::Passed);
    assert_eq!(m.list().unwrap().len(), 2);
}
#[test]
fn logs_rotate_and_tail_is_bounded_and_sanitized() {
    use station::tasks::logs;
    let d = tempfile::tempdir().unwrap();
    let mut log = logs::LogWriter::new(d.path()).unwrap();
    for _ in 0..6 {
        log.append("stdout", &vec![b'a'; 5 * 1024 * 1024]).unwrap();
    }
    log.append("stderr", b"\xff\x1b]52;secret\x07 END").unwrap();
    let tail = logs::tail(d.path(), 65536).unwrap();
    assert!(tail.len() <= 65536);
    assert!(!tail.contains('\x1b'));
    assert!(tail.contains("END"));
    let files = std::fs::read_dir(d.path()).unwrap().count();
    assert_eq!(files, 4);
    for f in std::fs::read_dir(d.path()).unwrap() {
        assert!(f.unwrap().metadata().unwrap().len() <= 5 * 1024 * 1024);
    }
}
#[test]
fn bounded_tail_keeps_the_final_message() {
    let d = tempfile::tempdir().unwrap();
    let mut bytes = vec![0xff; 100_000];
    bytes.extend_from_slice(b"UNIQUE_FINAL_ERROR");
    std::fs::write(d.path().join("output.log"), bytes).unwrap();
    let tail = station::tasks::logs::tail(d.path(), 1024).unwrap();
    assert!(tail.len() <= 1024);
    assert!(
        tail.ends_with("UNIQUE_FINAL_ERROR"),
        "missing final error: {}",
        tail.len()
    );
}
