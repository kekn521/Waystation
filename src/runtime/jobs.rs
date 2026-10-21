use crate::{app::Effect, tasks::TaskManager};
use std::sync::mpsc::{self, Receiver, SyncSender};
pub struct JobResult {
    pub message: String,
    pub detail: Option<(String, String)>,
    pub quit: bool,
    pub started: Option<uuid::Uuid>,
    pub agent: Option<crate::agents::AgentSession>,
    /// A workspace directory the New workspace form just made.
    pub workspace: Option<std::path::PathBuf>,
    /// Whether that workspace was pinned in config.toml, being outside every scanned root.
    pub pinned: bool,
    /// A workspace taken off the list, its folder possibly trashed.
    pub removed: Option<std::path::PathBuf>,
    pub attach: Option<crate::runtime::command::CommandSpec>,
}
pub struct Jobs {
    tx: SyncSender<Effect>,
    rx: Receiver<Result<JobResult, String>>,
    pub busy: bool,
}
impl Jobs {
    /// `config` and `config_path` let a new workspace outside the scanned roots be pinned.
    pub fn new(
        manager: TaskManager,
        agents: crate::agents::AgentManager,
        config: crate::config::Config,
        config_path: std::path::PathBuf,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<Effect>(1);
        let (out, results) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            while let Ok(effect) = rx.recv() {
                let result = (|| -> anyhow::Result<JobResult> {
                    let mut result = JobResult {
                        message: String::new(),
                        detail: None,
                        quit: false,
                        started: None,
                        agent: None,
                        workspace: None,
                        pinned: false,
                        removed: None,
                        attach: None,
                    };
                    match effect {
                        Effect::CreateAgent {
                            name,
                            tool,
                            command,
                            workspace,
                        } => {
                            let session = agents.create(
                                &name,
                                &tool,
                                &command,
                                &workspace,
                                &crate::runtime::command::launcher()?,
                            )?;
                            // Persisted before attachment: a failed terminal handoff is recoverable.
                            result.agent = Some(session);
                            result.message =
                                "Session created · Enter opens · F12 or Ctrl-\\ returns to Waystation".into();
                        }
                        Effect::CreateWorkspace { root, name } => {
                            let git = crate::runtime::command::executable("git".as_ref())
                                .ok_or_else(|| {
                                    anyhow::anyhow!("Creating a workspace needs git installed")
                                })?;
                            let dir = crate::providers::projects::create(&root, &name, &git)?;
                            result.message =
                                format!("Created {} · a new Git repository", dir.display());
                            // Outside the roots the scan would drop it again, so it is pinned.
                            if !crate::providers::projects::is_listed(&config, &dir) {
                                match crate::config::pin_project(&config_path, &dir) {
                                    Ok(()) => {
                                        result.pinned = true;
                                        result.message += " · pinned in config.toml";
                                    }
                                    Err(e) => {
                                        result.message +=
                                            &format!(" · could not pin it in config.toml: {e:#}")
                                    }
                                }
                            }
                            result.workspace = Some(dir);
                        }
                        Effect::AddWorkspace(dir) => {
                            let (dir, pinned) = add_folder(&config, &config_path, &dir)?;
                            result.message = format!("Added {}", dir.display());
                            if pinned {
                                result.message += " · pinned in config.toml";
                            }
                            result.pinned = pinned;
                            result.workspace = Some(dir);
                        }
                        Effect::RemoveWorkspace { dir, trash } => {
                            if trash {
                                ensure_trashable(&config, &dir)?;
                                trash::delete(&dir).map_err(|e| {
                                    anyhow::anyhow!("Could not trash {}: {e}", dir.display())
                                })?;
                            }
                            // Unpinned, it is also hidden when a scan would still find it.
                            let mut unpinned = config.clone();
                            unpinned.pinned_projects.retain(|p| {
                                std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) != dir
                            });
                            let hide =
                                !trash && crate::providers::projects::is_listed(&unpinned, &dir);
                            result.message = if trash {
                                format!("Moved {} to the trash", dir.display())
                            } else {
                                format!(
                                    "Removed {} from Waystation · its files stay",
                                    dir.display()
                                )
                            };
                            if let Err(e) = crate::config::forget_project(&config_path, &dir, hide)
                            {
                                result.message += &format!(
                                    " · config.toml unchanged, so it may return after a restart: {e:#}"
                                );
                            }
                            result.removed = Some(dir);
                        }
                        Effect::AttachAgent(id) => {
                            result.attach =
                                Some(agents.attach(id, &crate::runtime::command::launcher()?)?)
                        }
                        Effect::RestartAgent(id) => {
                            agents.restart(id, &crate::runtime::command::launcher()?)?;
                            result.message =
                                "Agent restarted with status tracking · Enter opens".into();
                        }
                        Effect::InstallStatusLine => {
                            agents.install_statusline(&crate::runtime::command::launcher()?)?;
                            result.message =
                                "Claude status line set · limits appear after Claude's next reply"
                                    .into();
                        }
                        Effect::CloseAgent(id) => {
                            agents.close(id)?;
                            result.message = "Agent session closed".into();
                        }
                        Effect::StartTask(recipe) => {
                            let id = manager.start(&recipe)?;
                            result.started = Some(id);
                            result.message =
                                format!("Started {} · {}", recipe.label, &id.to_string()[..8]);
                        }
                        Effect::StopTask(id) => {
                            manager.request_stop(id)?;
                            result.message = "Stop requested · waiting for process to exit".into()
                        }
                        Effect::ReadLog(id) => {
                            let dir = manager.run_dir(id);
                            let r = crate::tasks::read_record(&dir)?;
                            result.detail = Some((
                                format!(
                                    "{} · {:?} · j/k scroll · Esc close",
                                    r.recipe.label, r.status
                                ),
                                crate::tasks::logs::tail(&dir, 65536)?,
                            ));
                        }
                        Effect::DockerLogs(id) => {
                            result.detail = Some((
                                "Docker logs · read only · Esc close".into(),
                                crate::providers::services::logs(&id)?,
                            ))
                        }
                        Effect::Copy(path) => {
                            crate::runtime::actions::copy_path(&path)?;
                            result.message = "Path copied".into()
                        }
                        Effect::StopAllAndQuit(ids) => {
                            for id in ids {
                                manager.request_stop(id)?;
                            }
                            result.quit = true;
                        }
                        _ => anyhow::bail!("Unsupported background action"),
                    }
                    Ok(result)
                })();
                if out.send(result.map_err(|e| format!("{e:#}"))).is_err() {
                    break;
                }
            }
        });
        Self {
            tx,
            rx: results,
            busy: false,
        }
    }
    pub fn submit(&mut self, effect: Effect) -> bool {
        if self.busy {
            return false;
        }
        self.busy = self.tx.try_send(effect).is_ok();
        self.busy
    }
    pub fn try_recv(&mut self) -> Option<Result<JobResult, String>> {
        let r = self.rx.try_recv().ok()?;
        self.busy = false;
        Some(r)
    }
}

/// Make the existing folder `dir` a lasting workspace: unhidden when a scan finds it,
/// pinned in the config at `config_path` otherwise. Returns its canonical path and
/// whether it was pinned.
fn add_folder(
    config: &crate::config::Config,
    config_path: &std::path::Path,
    dir: &std::path::Path,
) -> anyhow::Result<(std::path::PathBuf, bool)> {
    let dir = std::fs::canonicalize(dir)
        .map_err(|_| anyhow::anyhow!("Folder not found: {}", dir.display()))?;
    anyhow::ensure!(dir.is_dir(), "{} is not a folder", dir.display());
    let mut unhidden = config.clone();
    unhidden
        .hidden_projects
        .retain(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone()) != dir);
    if crate::providers::projects::is_listed(&unhidden, &dir) {
        crate::config::unhide_project(config_path, &dir)?;
        Ok((dir, false))
    } else {
        crate::config::pin_project(config_path, &dir)?;
        Ok((dir, true))
    }
}

/// Refuse to trash a folder that holds other projects: home, `/`, or a project root.
fn ensure_trashable(config: &crate::config::Config, dir: &std::path::Path) -> anyhow::Result<()> {
    let canonical = |p: &std::path::Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.into());
    let home = std::env::home_dir().map(|h| canonical(&h));
    if dir.parent().is_none()
        || home.as_deref() == Some(dir)
        || config.project_roots.iter().any(|r| canonical(r) == dir)
    {
        anyhow::bail!(
            "{} holds your other projects, so Waystation won't trash it",
            dir.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{add_folder, ensure_trashable};
    use crate::config::Config;

    #[test]
    fn never_trashes_home_root_or_a_project_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        let config = Config {
            project_roots: vec![root.clone()],
            ..Config::default()
        };
        assert!(ensure_trashable(&config, &root).is_err());
        assert!(ensure_trashable(&config, "/".as_ref()).is_err());
        let home = std::fs::canonicalize(std::env::home_dir().unwrap()).unwrap();
        assert!(ensure_trashable(&config, &home).is_err());
        assert!(ensure_trashable(&config, &root.join("project")).is_ok());
    }

    #[test]
    fn adding_a_folder_pins_it_unless_the_scan_finds_it() {
        let tmp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(tmp.path()).unwrap();
        std::fs::create_dir_all(root.join("code/repo/.git")).unwrap();
        std::fs::create_dir_all(root.join("code/notes")).unwrap();
        let config_path = root.join("config.toml");
        let config = Config {
            project_roots: vec![root.join("code")],
            hidden_projects: vec![root.join("code/repo")],
            ..Config::default()
        };
        std::fs::write(
            &config_path,
            format!("hidden_projects = [{:?}]\n", root.join("code/repo")),
        )
        .unwrap();

        // A plain folder has no marker, so it is pinned.
        let (dir, pinned) = add_folder(&config, &config_path, &root.join("code/notes")).unwrap();
        assert_eq!((dir, pinned), (root.join("code/notes"), true));
        // A hidden repo the scan finds is only unhidden.
        let (dir, pinned) = add_folder(&config, &config_path, &root.join("code/repo")).unwrap();
        assert_eq!((dir, pinned), (root.join("code/repo"), false));
        let saved = Config::load(&config_path, &root).unwrap();
        assert_eq!(saved.pinned_projects, vec![root.join("code/notes")]);
        assert!(
            saved.hidden_projects.is_empty(),
            "{:?}",
            saved.hidden_projects
        );

        assert!(add_folder(&config, &config_path, &root.join("missing")).is_err());
        assert!(add_folder(&config, &config_path, &config_path).is_err());
    }
}
