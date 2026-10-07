pub mod identity;
pub mod logs;
pub mod supervisor;
use crate::{config::TaskRecipe, store::atomic_json};
use anyhow::{Context, Result, ensure};
use identity::ProcessIdentity;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::fd::AsRawFd,
    os::unix::{
        fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
        net::UnixStream,
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, SystemTime},
};
pub type RunId = uuid::Uuid;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RunStatus {
    Starting,
    Running,
    Passed,
    Failed,
    Stopped,
    Unknown,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: RunId,
    pub recipe: TaskRecipe,
    #[serde(with = "crate::path_serde")]
    pub cwd: PathBuf,
    pub started: SystemTime,
    pub ended: Option<SystemTime>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub supervisor: Option<ProcessIdentity>,
    pub child: Option<ProcessIdentity>,
    #[serde(with = "crate::path_serde")]
    pub log_dir: PathBuf,
    pub error: Option<String>,
}
impl RunRecord {
    pub fn stoppable(&self) -> bool {
        self.status == RunStatus::Running && self.supervisor.as_ref().is_some_and(identity::matches)
    }
}
#[derive(Clone)]
pub struct TaskManager {
    pub state_dir: PathBuf,
    executable: PathBuf,
}
impl TaskManager {
    pub fn new(state_dir: PathBuf) -> Self {
        Self {
            state_dir,
            executable: crate::runtime::command::launcher().unwrap_or_default(),
        }
    }
    pub fn with_executable(state_dir: PathBuf, executable: PathBuf) -> Self {
        Self {
            state_dir,
            executable,
        }
    }
    pub fn run_dir(&self, id: RunId) -> PathBuf {
        self.state_dir.join("runs").join(id.to_string())
    }
    pub fn start(&self, recipe: &TaskRecipe) -> Result<RunId> {
        ensure!(
            recipe.cwd.is_absolute() && recipe.cwd.is_dir(),
            "Task working directory must exist and be absolute"
        );
        ensure!(
            crate::runtime::command::executable_in(recipe.command.program.as_ref(), &recipe.cwd)
                .is_some(),
            "Task program unavailable: {}",
            recipe.command.program
        );
        fs::create_dir_all(self.state_dir.join("runs"))?;
        let id = RunId::new_v4();
        let dir = self.run_dir(id);
        fs::DirBuilder::new().mode(0o700).create(&dir)?;
        let record = RunRecord {
            id,
            recipe: recipe.clone(),
            cwd: recipe.cwd.clone(),
            started: SystemTime::now(),
            ended: None,
            status: RunStatus::Starting,
            exit_code: None,
            supervisor: None,
            child: None,
            log_dir: dir.clone(),
            error: None,
        };
        atomic_json(&dir.join("record.json"), &record)?;
        let mut token = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(dir.join("capability"))?;
        write!(token, "{}", RunId::new_v4())?;
        token.sync_all()?;
        let mut cmd = Command::new(&self.executable);
        cmd.arg("__supervise")
            .arg("--run-dir")
            .arg(&dir)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::from)?;
                Ok(())
            });
        }
        match cmd.spawn() {
            Ok(mut child) => {
                std::thread::spawn(move || {
                    let _ = child.wait();
                });
            }
            Err(e) => {
                let mut record = record;
                record.status = RunStatus::Failed;
                record.error = Some(e.to_string());
                record.ended = Some(SystemTime::now());
                atomic_json(&dir.join("record.json"), &record)?;
                return Err(e.into());
            }
        }
        Ok(id)
    }
    pub fn list(&self) -> Result<Vec<RunRecord>> {
        let mut records = vec![];
        let dirs = match fs::read_dir(self.state_dir.join("runs")) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(records),
            Err(e) => return Err(e.into()),
        };
        for entry in dirs.flatten() {
            if entry
                .file_name()
                .to_str()
                .and_then(|s| RunId::parse_str(s).ok())
                .is_none()
            {
                continue;
            }
            if let Ok(mut r) = read_record(&entry.path()) {
                if r.status == RunStatus::Running && !r.stoppable()
                    || r.status == RunStatus::Starting
                        && r.started.elapsed().unwrap_or_default() > Duration::from_secs(10)
                {
                    r.status = RunStatus::Unknown;
                }
                records.push(r);
            }
        }
        records.sort_by_key(|r| std::cmp::Reverse(r.started));
        Ok(records)
    }
    pub fn request_stop(&self, id: RunId) -> Result<()> {
        let dir = self.run_dir(id);
        let r = read_record(&dir)?;
        ensure!(
            r.stoppable(),
            "Run ownership cannot be verified; stop disabled"
        );
        let cap = fs::read_to_string(dir.join("capability"))?;
        let handle = File::open(&dir)?;
        let socket = socket_path(&handle);
        let mut stream =
            UnixStream::connect(socket).context("Supervisor unavailable; stop disabled")?;
        stream.set_read_timeout(Some(Duration::from_millis(500)))?;
        stream.set_write_timeout(Some(Duration::from_millis(500)))?;
        writeln!(stream, "stop {cap}")?;
        let mut response = String::new();
        stream.take(16).read_to_string(&mut response)?;
        ensure!(response == "ok\n", "Supervisor did not acknowledge stop");
        Ok(())
    }
}
pub fn read_record(dir: &Path) -> Result<RunRecord> {
    let mut s = String::new();
    File::open(dir.join("record.json"))?
        .take(1024 * 1024)
        .read_to_string(&mut s)?;
    Ok(serde_json::from_str(&s)?)
}
pub fn socket_path(dir: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}/control.sock", dir.as_raw_fd()))
}
pub fn secure_dir(dir: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let m = fs::symlink_metadata(dir)?;
    ensure!(
        m.is_dir()
            && m.uid() == nix::unistd::geteuid().as_raw()
            && m.permissions().mode() & 0o077 == 0,
        "Run directory must be private and owned by current user"
    );
    Ok(())
}
