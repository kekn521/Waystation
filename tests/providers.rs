use std::fs;
use waystation::providers::system::SystemSampler;
#[test]
fn first_sample_has_no_rate() {
    let d = tempfile::tempdir().unwrap();
    fs::create_dir(d.path().join("net")).unwrap();
    fs::write(d.path().join("stat"), "cpu  20 0 10 70 0 0 0 0\n").unwrap();
    fs::write(
        d.path().join("meminfo"),
        "MemTotal: 1000 kB\nMemAvailable: 500 kB\n",
    )
    .unwrap();
    fs::write(
        d.path().join("net/dev"),
        "eth0: 100 0 0 0 0 0 0 0 200 0 0 0 0 0 0 0\n",
    )
    .unwrap();
    let mut s = SystemSampler::default();
    let a = s.sample(d.path(), d.path()).unwrap();
    assert!(a.cpu.is_none());
    assert!(a.network_rate.is_none());
    assert_eq!(a.memory_used, 512000);
    assert!(a.gpu.is_none());
    fs::write(d.path().join("stat"), "cpu  30 0 20 100 0 0 0 0\n").unwrap();
    assert!((s.sample(d.path(), d.path()).unwrap().cpu.unwrap() - 40.).abs() < 0.01);
    fs::write(d.path().join("stat"), "cpu  0 0 0 1\n").unwrap();
    assert!(s.sample(d.path(), d.path()).unwrap().cpu.is_none());
}
#[test]
fn terminal_controls_are_harmless() {
    let text = waystation::ui::safe("file\x1b]52;c;secret\x07\x1b[31m");
    assert!(!text.contains('\x1b'));
    assert!(!text.contains('\x07'));
}
#[test]
fn slow_provider_does_not_block_keys_and_late_result_is_discarded() {
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::{Duration, Instant};
    use waystation::{
        app::{Action, App},
        config::Config,
        model::AppState,
        runtime::workers::*,
    };
    let (tx, rx) = mpsc::channel();
    let rx = Arc::new(Mutex::new(rx));
    let pool = WorkerPool::with_provider(move |_| {
        rx.lock().unwrap().recv().unwrap();
        Ok(ProviderPayload::Files(vec![]))
    });
    let mut app = App::new(Config::default(), AppState::default());
    let before = Instant::now();
    pool.submit(ProviderRequest {
        id: ProviderId::Files,
        generation: 0,
        workspace: None,
        directory: None,
        hidden: false,
    });
    app.update(Action::SelectWorkspace("/tmp/new".into()));
    app.update(Action::Search);
    app.update(Action::Insert('x'));
    assert_eq!(app.query, "x");
    assert!(before.elapsed() < Duration::from_millis(100));
    tx.send(()).unwrap();
    let until = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(e) = pool.try_recv() {
            assert!(!app.apply_provider(e));
            break;
        }
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(app.workspace().unwrap(), std::path::Path::new("/tmp/new"));
}
