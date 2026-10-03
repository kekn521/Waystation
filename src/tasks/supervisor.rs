use super::*;
use crate::runtime::command::{drain, nonblocking};
use nix::{
    sys::{
        signal::{Signal, killpg},
        socket::{getsockopt, sockopt::PeerCredentials},
        wait::{Id, WaitPidFlag, WaitStatus, waitid},
    },
    unistd::Pid,
};
use std::{os::unix::net::UnixListener, time::Instant};
struct Owned(std::process::Child, bool);
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.1 {
            let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
            let _ = self.0.wait();
        }
    }
}
pub fn run(dir: &Path) -> Result<()> {
    secure_dir(dir)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .mode(0o600)
        .open(dir.join("owner.lock"))?;
    lock.try_lock()
        .context("Supervisor already owns this run")?;
    let mut record = read_record(dir)?;
    ensure!(
        record.status == RunStatus::Starting,
        "Run has already started"
    );
    let outcome = supervise(dir, &mut record);
    if let Err(e) = &outcome {
        record.status = RunStatus::Failed;
        record.error = Some(format!("{e:#}"));
        record.ended = Some(SystemTime::now());
        atomic_json(&dir.join("record.json"), &record)?;
    }
    let _ = fs::remove_file(dir.join("control.sock"));
    outcome
}
fn supervise(dir: &Path, record: &mut RunRecord) -> Result<()> {
    let handle = File::open(dir)?;
    let listener = UnixListener::bind(socket_path(&handle))?;
    fs::set_permissions(dir.join("control.sock"), fs::Permissions::from_mode(0o600))?;
    listener.set_nonblocking(true)?;
    let cap = fs::read_to_string(dir.join("capability"))?;
    let program =
        crate::runtime::command::executable_in(record.recipe.command.program.as_ref(), &record.cwd)
            .context("Task executable unavailable")?;
    let child = Command::new(program)
        .args(&record.recipe.command.args)
        .current_dir(&record.cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let mut owned = Owned(child, false);
    let pid = Pid::from_raw(owned.0.id() as i32);
    record.supervisor = Some(identity::read(Path::new("/proc"), std::process::id())?);
    record.child = Some(identity::read(Path::new("/proc"), owned.0.id())?);
    record.status = RunStatus::Running;
    atomic_json(&dir.join("record.json"), record)?;
    let mut stdout = owned.0.stdout.take().context("Task stdout unavailable")?;
    let mut stderr = owned.0.stderr.take().context("Task stderr unavailable")?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut log = logs::LogWriter::new(dir)?;
    let mut stopped = None;
    let mut exited = None;
    loop {
        for _ in 0..4 {
            let (mut stream, _) = match listener.accept() {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e.into()),
            };
            if getsockopt(&stream, PeerCredentials)?.uid() != nix::unistd::geteuid().as_raw() {
                continue;
            }
            stream.set_read_timeout(Some(Duration::from_millis(100)))?;
            stream.set_write_timeout(Some(Duration::from_millis(100)))?;
            let mut command = String::new();
            let mut bytes = [0u8; 128];
            if let Ok(n) = stream.read(&mut bytes) {
                command = String::from_utf8_lossy(&bytes[..n]).into_owned();
            }
            if command.trim_end() == format!("stop {cap}") && exited.is_none() {
                if stopped.is_none() {
                    killpg(pid, Signal::SIGTERM)?;
                    stopped = Some(Instant::now());
                }
                let _ = stream.write_all(b"ok\n");
            }
        }
        let mut out = vec![];
        let mut err = vec![];
        let mut truncated = false;
        let out_done = drain(&mut stdout, &mut out, 128 * 1024, &mut truncated)?;
        let err_done = drain(&mut stderr, &mut err, 128 * 1024, &mut truncated)?;
        log.append("stdout", &out)?;
        log.append("stderr", &err)?;
        if let Some(time) = stopped
            && time.elapsed() > Duration::from_secs(5)
            && exited.is_none()
        {
            let _ = killpg(pid, Signal::SIGKILL);
        }
        if exited.is_none()
            && !matches!(
                waitid(
                    Id::Pid(pid),
                    WaitPidFlag::WEXITED | WaitPidFlag::WNOHANG | WaitPidFlag::WNOWAIT
                )?,
                WaitStatus::StillAlive
            )
        {
            // The leader is still unreaped: its PID/group cannot have been reused.
            let _ = killpg(pid, Signal::SIGKILL);
            exited = Some(Instant::now());
        }
        if exited.is_some_and(|t| out_done && err_done || t.elapsed() > Duration::from_millis(300))
        {
            let status = owned.0.wait()?;
            owned.1 = true;
            record.exit_code = status.code();
            record.status = if stopped.is_some() {
                RunStatus::Stopped
            } else if status.success() {
                RunStatus::Passed
            } else {
                RunStatus::Failed
            };
            record.ended = Some(SystemTime::now());
            atomic_json(&dir.join("record.json"), record)?;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    Ok(())
}
