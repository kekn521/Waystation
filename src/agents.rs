//! Persistent interactive sessions, isolated from the user's tmux server.
use crate::{
    config::ToolCommand,
    runtime::command::{CommandRunner, CommandSpec},
    store::atomic_json,
};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    os::unix::{fs::OpenOptionsExt, process::CommandExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum AgentStatus {
    Running,
    Exited(i32),
    #[default]
    Unavailable,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentSession {
    pub id: Uuid,
    pub name: String,
    pub tool: String,
    #[serde(with = "crate::path_serde")]
    pub workspace: PathBuf,
    pub command: ToolCommand,
    #[serde(with = "crate::path_serde")]
    pub executable: PathBuf,
    pub created: SystemTime,
    #[serde(skip)]
    pub status: AgentStatus,
}
impl AgentSession {
    fn target(&self) -> String {
        format!("=agent-{}", self.id)
    }
}
#[derive(Clone)]
pub struct AgentManager {
    dir: PathBuf,
}
impl AgentManager {
    pub fn new(state: PathBuf) -> Self {
        Self {
            dir: state.join("agents"),
        }
    }
    fn lock(&self) -> Result<File> {
        fs::create_dir_all(&self.dir)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(self.dir.join("lock"))?;
        f.lock()?;
        Ok(f)
    }
    fn key(&self) -> Result<Option<Uuid>> {
        match fs::read(self.dir.join("server.json")) {
            Ok(bytes) => Ok(Some(serde_json::from_slice(&bytes)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    fn spec(&self, key: Uuid, args: Vec<OsString>) -> CommandSpec {
        let mut all = vec![
            "-u".into(),
            "TMUX".into(),
            "tmux".into(),
            "-L".into(),
            format!("station-{key}").into(),
            "-f".into(),
            self.dir.join("tmux.conf").into_os_string(),
        ];
        all.extend(args);
        CommandSpec {
            program: "/usr/bin/env".into(),
            args: all,
            cwd: self.dir.clone(),
        }
    }
    fn call(
        &self,
        key: Uuid,
        args: Vec<OsString>,
    ) -> Result<crate::runtime::command::CapturedOutput> {
        CommandRunner.capture(&self.spec(key, args), Duration::from_secs(4), 256 * 1024)
    }
    fn checked(&self, key: Uuid, args: Vec<OsString>) -> Result<String> {
        let r = self.call(key, args)?;
        ensure!(
            r.status.success(),
            "tmux: {}",
            String::from_utf8_lossy(&r.stderr).trim()
        );
        ensure!(!r.truncated, "tmux response too large");
        Ok(String::from_utf8_lossy(&r.stdout).into_owned())
    }
    fn live(&self, key: Uuid) -> Result<HashMap<String, AgentStatus>> {
        let r = self.call(
            key,
            vec![
                "list-panes".into(),
                "-a".into(),
                "-F".into(),
                "#{session_name}\t#{pane_dead}\t#{pane_dead_status}".into(),
            ],
        )?;
        if !r.status.success() {
            let error = String::from_utf8_lossy(&r.stderr);
            if error.contains("no server running")
                || error.contains("No such file or directory")
                || error.contains("Connection refused")
            {
                return Ok(HashMap::new());
            }
            anyhow::bail!("tmux unavailable: {}", error.trim());
        }
        ensure!(!r.truncated, "Too many agent panes");
        Ok(String::from_utf8_lossy(&r.stdout)
            .lines()
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let name = fields.next()?.to_owned();
                let status = if fields.next()? == "1" {
                    AgentStatus::Exited(fields.next().and_then(|s| s.parse().ok()).unwrap_or(-1))
                } else {
                    AgentStatus::Running
                };
                Some((name, status))
            })
            .collect())
    }
    pub fn list(&self) -> Result<Vec<AgentSession>> {
        let Some(key) = self.key()? else {
            return Ok(vec![]);
        };
        let live = self.live(key)?;
        let mut sessions = vec![];
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Uuid::parse_str(s).ok())
                .is_none()
                || path.extension().is_none_or(|s| s != "json")
            {
                continue;
            }
            let mut s: AgentSession = match fs::read(&path) {
                Ok(bytes) => serde_json::from_slice(&bytes).context("Reading agent session")?,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            s.status = live
                .get(&format!("agent-{}", s.id))
                .cloned()
                .unwrap_or_default();
            sessions.push(s);
        }
        sessions.sort_by_key(|s| s.created);
        Ok(sessions)
    }
    pub fn create(
        &self,
        name: &str,
        tool: &str,
        command: &ToolCommand,
        workspace: &Path,
        launcher: &Path,
    ) -> Result<AgentSession> {
        ensure!(
            !name.trim().is_empty() && name.len() <= 200 && !name.chars().any(char::is_control),
            "Enter a session name (up to 200 bytes)"
        );
        ensure!(workspace.is_dir(), "Workspace is unavailable");
        let program = crate::runtime::command::executable_in(command.program.as_ref(), workspace)
            .context("Agent program is not installed; configure tools in config.toml")?;
        ensure!(
            crate::runtime::command::executable("tmux".as_ref()).is_some(),
            "Install tmux to manage agent sessions"
        );
        let _lock = self.lock()?;
        let key = if let Some(key) = self.key()? {
            key
        } else {
            let key = Uuid::new_v4();
            atomic_json(&self.dir.join("server.json"), &key)?;
            key
        };
        // Loaded before the first session starts, including commands that exit immediately.
        fs::write(
            self.dir.join("tmux.conf"),
            "set -g remain-on-exit on\nset -g remain-on-exit-format ''\nset -g history-limit 50000\nset -g mouse on\nset -g status-style 'bg=#1e2030,fg=#cad3f5'\nset -g status-left '#[fg=#c6a0f6,bold] WAYSTATION #[default]'\nset -g status-right '#[fg=#8bd5ca] F12 → Waystation  '\nset -g status-right-length 40\nset -g allow-rename off\nset -g automatic-rename off\nbind-key -n F12 detach-client\n",
        )?;
        let session = AgentSession {
            id: Uuid::new_v4(),
            name: name.trim().into(),
            tool: tool.into(),
            workspace: workspace.canonicalize()?,
            command: command.clone(),
            executable: program,
            created: SystemTime::now(),
            status: AgentStatus::Running,
        };
        let manifest = self.dir.join(format!("{}.json", session.id));
        atomic_json(&manifest, &session)?;
        let args = vec![
            "new-session".into(),
            "-d".into(),
            "-s".into(),
            format!("agent-{}", session.id).into(),
            "-n".into(),
            "agent".into(),
            "-c".into(),
            session.workspace.clone().into_os_string(),
            "--".into(),
            launcher.as_os_str().into(),
            "__agent-exec".into(),
            manifest.clone().into_os_string(),
        ];
        let started = self.checked(key, args).and_then(|_| {
            self.checked(
                key,
                vec![
                    "set-option".into(),
                    "-g".into(),
                    "mouse".into(),
                    "on".into(),
                ],
            )
        });
        if let Err(error) = started {
            // Preserve the record if tmux did start despite a client timeout.
            if self
                .live(key)
                .is_ok_and(|live| !live.contains_key(&format!("agent-{}", session.id)))
            {
                let _ = fs::remove_file(&manifest);
            }
            return Err(error);
        }
        Ok(session)
    }
    pub fn attach(&self, id: Uuid) -> Result<CommandSpec> {
        let session = self
            .list()?
            .into_iter()
            .find(|s| s.id == id)
            .context("Agent session not found")?;
        ensure!(
            session.status != AgentStatus::Unavailable,
            "Session is no longer running (for example after a reboot). Create a new agent session."
        );
        Ok(self.spec(
            self.key()?.context("Missing server")?,
            vec![
                "attach-session".into(),
                "-t".into(),
                session.target().into(),
            ],
        ))
    }
    pub fn close(&self, id: Uuid) -> Result<()> {
        let _lock = self.lock()?;
        let session = self
            .list()?
            .into_iter()
            .find(|s| s.id == id)
            .context("Agent session not found")?;
        if session.status != AgentStatus::Unavailable {
            self.checked(
                self.key()?.context("Missing server")?,
                vec!["kill-session".into(), "-t".into(), session.target().into()],
            )?;
        }
        fs::remove_file(self.dir.join(format!("{id}.json")))?;
        Ok(())
    }
    pub fn capture(&self, id: Uuid) -> Result<String> {
        self.checked(
            self.key()?.context("Missing server")?,
            vec![
                "capture-pane".into(),
                "-p".into(),
                "-S".into(),
                "-2000".into(),
                "-t".into(),
                format!("=agent-{id}:0.0").into(),
            ],
        )
    }
}
pub fn exec(path: &Path) -> Result<()> {
    let session: AgentSession = serde_json::from_slice(&fs::read(path)?)?;
    let error = std::process::Command::new(&session.executable)
        .args(&session.command.args)
        .current_dir(&session.workspace)
        .exec();
    Err(error).context("Starting agent")
}
