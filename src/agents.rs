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
    io::Write,
    os::unix::{
        ffi::OsStrExt,
        fs::{OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime},
};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum AgentStatus {
    Running,
    Exited(i32),
    /// Not running (for example after a reboot), but its conversation can be resumed.
    Saved,
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
    /// The agent's own conversation id, used to resume it once its process is gone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<String>,
    #[serde(skip)]
    pub status: AgentStatus,
}
impl AgentSession {
    fn target(&self) -> String {
        format!("=agent-{}", self.id)
    }
    fn kind(&self) -> AgentKind {
        match self.executable.file_name().and_then(|n| n.to_str()) {
            Some("claude") => AgentKind::Claude,
            Some("codex") => AgentKind::Codex,
            _ => AgentKind::Other,
        }
    }
}
/// Whether `~/.codex/hooks.json` reports Codex conversation ids to this Waystation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodexHookStatus {
    Installed,
    Missing,
    /// Installed for a Waystation executable at a different path.
    Stale,
}
/// Agents whose conversations Waystation knows how to resume.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AgentKind {
    Claude,
    Codex,
    Other,
}
#[derive(Clone)]
pub struct AgentManager {
    dir: PathBuf,
    claude_home: PathBuf,
    codex_home: PathBuf,
}
impl AgentManager {
    pub fn new(state: PathBuf) -> Self {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let env_or = |var: &str, default: &str| {
            std::env::var_os(var)
                .filter(|v| !v.is_empty())
                .map_or_else(|| home.join(default), PathBuf::from)
        };
        Self {
            dir: state.join("agents"),
            claude_home: env_or("CLAUDE_CONFIG_DIR", ".claude"),
            codex_home: env_or("CODEX_HOME", ".codex"),
        }
    }
    /// Overrides where Claude and Codex keep their conversations.
    pub fn with_agent_homes(mut self, claude: PathBuf, codex: PathBuf) -> Self {
        self.claude_home = claude;
        self.codex_home = codex;
        self
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
    fn manifest(&self, id: Uuid) -> PathBuf {
        self.dir.join(format!("{id}.json"))
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
            s.status = match live.get(&format!("agent-{}", s.id)) {
                Some(status) => status.clone(),
                None if s.conversation.is_some() && s.kind() != AgentKind::Other => {
                    AgentStatus::Saved
                }
                None => AgentStatus::Unavailable,
            };
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
        fs::write(self.dir.join("tmux.conf"), TMUX_CONF)?;
        let mut session = AgentSession {
            id: Uuid::new_v4(),
            name: name.trim().into(),
            tool: tool.into(),
            workspace: workspace.canonicalize()?,
            command: command.clone(),
            executable: program,
            created: SystemTime::now(),
            conversation: None,
            status: AgentStatus::Running,
        };
        // Claude accepts a chosen id; Codex reports its id through the SessionStart hook.
        if session.kind() == AgentKind::Claude {
            session.conversation = Some(Uuid::new_v4().to_string());
        }
        let manifest = self.manifest(session.id);
        atomic_json(&manifest, &session)?;
        if let Err(error) = self.start(key, &session, launcher, false) {
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
    fn start(
        &self,
        key: Uuid,
        session: &AgentSession,
        launcher: &Path,
        resume: bool,
    ) -> Result<()> {
        let mut args = vec![
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
            self.manifest(session.id).into_os_string(),
        ];
        if resume {
            args.push("--resume".into());
        }
        self.checked(key, args)?;
        self.checked(
            key,
            vec![
                "set-option".into(),
                "-g".into(),
                "mouse".into(),
                "on".into(),
            ],
        )?;
        Ok(())
    }
    /// Starts a saved session again in its workspace, resuming its conversation.
    fn resume(&self, session: &AgentSession, launcher: &Path) -> Result<()> {
        ensure!(
            session.workspace.is_dir(),
            "Workspace is unavailable: {}",
            session.workspace.display()
        );
        ensure!(
            crate::runtime::command::executable("tmux".as_ref()).is_some(),
            "Install tmux to manage agent sessions"
        );
        let _lock = self.lock()?;
        let key = self.key()?.context("Missing server")?;
        ensure!(
            self.manifest(session.id).exists(),
            "Agent session not found"
        );
        // Another Waystation may have resumed it while this one waited for the lock.
        if self
            .live(key)?
            .contains_key(&format!("agent-{}", session.id))
        {
            return Ok(());
        }
        // A Claude session closed before its first message has no transcript to resume.
        let resume = session.kind() != AgentKind::Claude
            || session
                .conversation
                .as_deref()
                .is_some_and(|id| self.claude_transcript_exists(id));
        self.start(key, session, launcher, resume)
    }
    fn claude_transcript_exists(&self, id: &str) -> bool {
        let Ok(projects) = fs::read_dir(self.claude_home.join("projects")) else {
            return false;
        };
        projects
            .flatten()
            .any(|project| project.path().join(format!("{id}.jsonl")).is_file())
    }
    pub fn attach(&self, id: Uuid, launcher: &Path) -> Result<CommandSpec> {
        let session = self
            .list()?
            .into_iter()
            .find(|s| s.id == id)
            .context("Agent session not found")?;
        match session.status {
            AgentStatus::Unavailable => anyhow::bail!(
                "Session is no longer running and has no saved conversation to resume. Create a new agent session."
            ),
            AgentStatus::Saved => self.resume(&session, launcher)?,
            AgentStatus::Running | AgentStatus::Exited(_) => {}
        }
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
        if matches!(
            session.status,
            AgentStatus::Running | AgentStatus::Exited(_)
        ) {
            self.checked(
                self.key()?.context("Missing server")?,
                vec!["kill-session".into(), "-t".into(), session.target().into()],
            )?;
        }
        fs::remove_file(self.manifest(id))?;
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
/// The agents' tmux server config. F12 is out of reach on Mac keyboards without `fn`, so
/// Ctrl-\ also returns to Waystation; neither Claude Code nor Codex binds it.
const TMUX_CONF: &str = "set -g remain-on-exit on\nset -g remain-on-exit-format ''\nset -g history-limit 50000\nset -g mouse on\nset -g status-style 'bg=#1e2030,fg=#cad3f5'\nset -g status-left '#[fg=#c6a0f6,bold] WAYSTATION #[default]'\nset -g status-right '#[fg=#8bd5ca] F12 → Waystation  '\nset -g status-right-length 40\nset -g allow-rename off\nset -g automatic-rename off\nbind-key -n F12 detach-client\n";
pub fn exec(path: &Path, resume: bool) -> Result<()> {
    let path = std::path::absolute(path)?;
    let session: AgentSession = serde_json::from_slice(&fs::read(&path)?)?;
    let configured = session.command.args.iter().map(OsString::from);
    let claude_settings = || -> Result<OsString> {
        let hook = serde_json::json!({"hooks": {"SessionStart": [hook_group(&std::env::current_exe()?)?]}});
        Ok(hook.to_string().into())
    };
    let args: Vec<OsString> = match (session.kind(), session.conversation.as_deref(), resume) {
        (AgentKind::Claude, Some(id), resume) => configured
            .chain([
                "--settings".into(),
                claude_settings()?,
                if resume { "--resume" } else { "--session-id" }.into(),
                id.into(),
            ])
            .collect(),
        (AgentKind::Codex, Some(id), true) => std::iter::once("resume".into())
            .chain(configured)
            .chain([id.into()])
            .collect(),
        _ => configured.collect(),
    };
    // Identify this session to `__agent-hook`; exec keeps the pid, so it names the agent itself.
    let error = std::process::Command::new(&session.executable)
        .args(args)
        .current_dir(&session.workspace)
        .env(MANIFEST_ENV, &path)
        .env(PID_ENV, std::process::id().to_string())
        .exec();
    Err(error).context("Starting agent")
}
const MANIFEST_ENV: &str = "WAYSTATION_AGENT_MANIFEST";
const PID_ENV: &str = "WAYSTATION_AGENT_PID";
const HOOK_ARG: &str = "__agent-hook";
/// The SessionStart hook command for this Waystation executable, quoted for `sh -c`.
fn hook_command(launcher: &Path) -> Result<String> {
    let launcher = launcher
        .to_str()
        .context("Waystation's path must be UTF-8 to install agent hooks")?;
    Ok(format!("'{}' {HOOK_ARG}", launcher.replace('\'', r"'\''")))
}
fn hook_group(launcher: &Path) -> Result<serde_json::Value> {
    Ok(serde_json::json!({
        "hooks": [{"type": "command", "command": hook_command(launcher)?, "timeout": 10}]
    }))
}
fn is_waystation_hook(group: &serde_json::Value) -> bool {
    group["hooks"].as_array().is_some_and(|hooks| {
        hooks.iter().any(|h| {
            h["command"]
                .as_str()
                .is_some_and(|c| c.ends_with(&format!(" {HOOK_ARG}")))
        })
    })
}
impl AgentManager {
    fn codex_hooks_path(&self) -> PathBuf {
        self.codex_home.join("hooks.json")
    }
    fn codex_hooks(&self) -> Result<serde_json::Value> {
        match fs::read(self.codex_hooks_path()) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("Reading Codex hooks.json"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
            Err(e) => Err(e.into()),
        }
    }
    pub fn codex_hook(&self, launcher: &Path) -> CodexHookStatus {
        let (Ok(hooks), Ok(expected)) = (self.codex_hooks(), hook_group(launcher)) else {
            return CodexHookStatus::Missing;
        };
        let groups = hooks["hooks"]["SessionStart"].as_array();
        match groups.and_then(|g| g.iter().find(|g| is_waystation_hook(g))) {
            Some(group) if group["hooks"] == expected["hooks"] => CodexHookStatus::Installed,
            Some(_) => CodexHookStatus::Stale,
            None => CodexHookStatus::Missing,
        }
    }
    /// Adds (or repoints) Waystation's SessionStart group, leaving every other hook in place.
    ///
    /// Appending keeps the indexes, and so Codex's trust, of existing groups.
    pub fn install_codex_hook(&self, launcher: &Path) -> Result<()> {
        let mut hooks = self.codex_hooks()?;
        let group = hook_group(launcher)?;
        let root = hooks
            .as_object_mut()
            .context("Codex hooks.json is not a JSON object")?;
        let events = root
            .entry("hooks")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .context("Codex hooks.json has an unexpected `hooks` value")?;
        let groups = events
            .entry("SessionStart")
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .context("Codex hooks.json has an unexpected SessionStart value")?;
        match groups.iter_mut().find(|g| is_waystation_hook(g)) {
            Some(existing) => *existing = group,
            None => groups.push(group),
        }
        let path = self.codex_hooks_path();
        fs::create_dir_all(&self.codex_home)?;
        let mode = fs::metadata(&path).map_or(0o600, |m| m.permissions().mode() & 0o777);
        let tmp = self
            .codex_home
            .join(format!(".hooks.json.{}.tmp", Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut f = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(mode)
                .open(&tmp)?;
            serde_json::to_writer_pretty(&mut f, &hooks)?;
            f.write_all(b"\n")?;
            f.sync_all()?;
            fs::rename(&tmp, &path)?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }
}
/// Handles an agent's SessionStart hook: records the conversation id it now uses.
///
/// Silent and best effort, since agents may show hook output or fail on a non-zero exit.
/// Acts only for the agent process Waystation launched, not nested agents that inherit its env.
pub fn record_hook(input: &[u8]) -> Result<()> {
    let (Some(manifest), Some(pid)) = (
        std::env::var_os(MANIFEST_ENV).map(PathBuf::from),
        std::env::var(PID_ENV)
            .ok()
            .and_then(|p| p.parse::<i32>().ok()),
    ) else {
        return Ok(());
    };
    if !started_by(pid) {
        return Ok(());
    }
    let input: serde_json::Value = serde_json::from_slice(input)?;
    if input["hook_event_name"]
        .as_str()
        .is_some_and(|e| e != "SessionStart")
    {
        return Ok(());
    }
    let Some(id) = input["session_id"].as_str().filter(|id| {
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) else {
        return Ok(());
    };
    let dir = manifest.parent().context("Invalid agent manifest")?;
    ensure!(
        manifest
            .file_stem()
            .and_then(|s| s.to_str())
            .is_some_and(|s| Uuid::parse_str(s).is_ok())
            && dir.file_name().is_some_and(|n| n.as_bytes() == b"agents"),
        "Invalid agent manifest"
    );
    let manager = AgentManager::new(dir.parent().context("Invalid agent manifest")?.into());
    let _lock = manager.lock()?;
    let mut session: AgentSession = match fs::read(&manifest) {
        Ok(bytes) => serde_json::from_slice(&bytes)?,
        // Closed meanwhile; never recreate it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    // A different kind of agent nested inside this session reports its own transcript.
    let home = match session.kind() {
        AgentKind::Claude => manager.claude_home.join("projects"),
        AgentKind::Codex => manager.codex_home.join("sessions"),
        AgentKind::Other => return Ok(()),
    };
    let Some(transcript) = input["transcript_path"].as_str().map(Path::new) else {
        return Ok(());
    };
    if !(transcript.starts_with(&home)
        || home
            .canonicalize()
            .is_ok_and(|home| transcript.starts_with(home)))
    {
        return Ok(());
    }
    if session.conversation.as_deref() != Some(id) {
        session.conversation = Some(id.into());
        atomic_json(&manifest, &session)?;
    }
    Ok(())
}
/// Whether this hook was run by the agent process `pid` itself.
///
/// Walks up from the hook's parent, through any shells the agent wraps hooks in. Meeting
/// another process running the agent's own executable first means a nested copy of the agent
/// (say `claude -p` in a Bash tool call) inherited the session's environment.
fn started_by(pid: i32) -> bool {
    let parent = |pid: i32| -> Option<i32> {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // The command name may contain spaces or parentheses; fields resume after the last ')'.
        stat[stat.rfind(')')? + 1..]
            .split_whitespace()
            .nth(1)?
            .parse()
            .ok()
    };
    let exe = |pid: i32| fs::read_link(format!("/proc/{pid}/exe")).ok();
    let Some(agent) = exe(pid) else {
        return false;
    };
    let mut current = std::os::unix::process::parent_id() as i32;
    for _ in 0..32 {
        if current == pid {
            return true;
        }
        if current <= 1 || exe(current).as_ref() == Some(&agent) {
            return false;
        }
        let Some(next) = parent(current) else {
            return false;
        };
        current = next;
    }
    false
}
