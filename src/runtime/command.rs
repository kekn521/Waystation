use anyhow::{Context, Result, bail, ensure};
use nix::{
    fcntl::{FcntlArg, OFlag, fcntl},
    sys::signal::{Signal, killpg},
    unistd::Pid,
};
use std::{
    ffi::OsString,
    io::Read,
    os::{
        fd::AsFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
}
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub truncated: bool,
}
#[derive(Clone, Copy, Default)]
pub struct CommandRunner;
struct OwnedChild(Child, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !self.1 {
            let _ = killpg(Pid::from_raw(self.0.id() as i32), Signal::SIGKILL);
            let _ = self.0.wait();
        }
    }
}
/// Waystation's own executable, as written into agents' hooks and settings and used to
/// launch agents and task supervisors.
pub fn launcher() -> std::io::Result<PathBuf> {
    std::env::current_exe().map(live_executable)
}
/// Linux reports a running program's binary as `<path> (deleted)` once it has been
/// replaced, say by a reinstall; the file now at `<path>` is the one to run.
pub fn live_executable(exe: PathBuf) -> PathBuf {
    use std::os::unix::ffi::OsStrExt;
    if exe.exists() {
        return exe;
    }
    match exe.as_os_str().as_bytes().strip_suffix(b" (deleted)") {
        Some(path) => PathBuf::from(std::ffi::OsStr::from_bytes(path)),
        None => exe,
    }
}
pub fn executable(program: &std::ffi::OsStr) -> Option<PathBuf> {
    executable_in(program, &std::env::current_dir().ok()?)
}
pub fn executable_in(program: &std::ffi::OsStr, cwd: &Path) -> Option<PathBuf> {
    let p = Path::new(program);
    let valid = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if p.components().count() > 1 {
        let path = if p.is_absolute() {
            p.to_owned()
        } else {
            cwd.join(p)
        };
        return valid(&path).then_some(path);
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| {
            if d.is_absolute() {
                d.join(p)
            } else {
                cwd.join(d).join(p)
            }
        })
        .find(|p| valid(p))
}
pub fn nonblocking(fd: &impl AsFd) -> Result<()> {
    let flags = OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL)?);
    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK))?;
    Ok(())
}
pub fn drain(
    reader: &mut impl Read,
    buf: &mut Vec<u8>,
    limit: usize,
    truncated: &mut bool,
) -> Result<bool> {
    let mut block = [0u8; 8192];
    for _ in 0..16 {
        match reader.read(&mut block) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                let keep = n.min(limit.saturating_sub(buf.len()));
                buf.extend_from_slice(&block[..keep]);
                *truncated |= keep < n;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(false)
}
impl CommandRunner {
    pub fn capture(
        &self,
        spec: &CommandSpec,
        timeout: Duration,
        limit: usize,
    ) -> Result<CapturedOutput> {
        ensure!(
            spec.cwd.is_dir(),
            "Working directory unavailable: {}",
            spec.cwd.display()
        );
        let program = executable_in(&spec.program, &spec.cwd)
            .with_context(|| format!("Program not found: {}", spec.program.to_string_lossy()))?;
        let child = Command::new(program)
            .args(&spec.args)
            .current_dir(&spec.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .spawn()
            .context("Starting command")?;
        let mut guard = OwnedChild(child, false);
        let mut out = guard.0.stdout.take().context("Missing stdout")?;
        let mut err = guard.0.stderr.take().context("Missing stderr")?;
        nonblocking(&out)?;
        nonblocking(&err)?;
        let deadline = Instant::now() + timeout;
        let (mut stdout, mut stderr, mut truncated) = (vec![], vec![], false);
        let (mut out_done, mut err_done) = (false, false);
        loop {
            if !out_done {
                out_done = drain(&mut out, &mut stdout, limit, &mut truncated)?
            }
            if !err_done {
                err_done = drain(&mut err, &mut stderr, limit, &mut truncated)?
            }
            if out_done
                && err_done
                && let Some(status) = guard.0.try_wait()?
            {
                guard.1 = true;
                return Ok(CapturedOutput {
                    status,
                    stdout,
                    stderr,
                    truncated,
                });
            }
            if Instant::now() >= deadline {
                bail!(
                    "{} timed out after {:.1}s",
                    spec.program.to_string_lossy(),
                    timeout.as_secs_f32()
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
