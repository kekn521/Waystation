use crate::{app::Effect, tasks::TaskManager};
use std::sync::mpsc::{self, Receiver, SyncSender};
pub struct JobResult {
    pub message: String,
    pub detail: Option<(String, String)>,
    pub quit: bool,
    pub started: Option<uuid::Uuid>,
    pub agent: Option<crate::agents::AgentSession>,
    pub attach: Option<crate::runtime::command::CommandSpec>,
}
pub struct Jobs {
    tx: SyncSender<Effect>,
    rx: Receiver<Result<JobResult, String>>,
    pub busy: bool,
}
impl Jobs {
    pub fn new(manager: TaskManager, agents: crate::agents::AgentManager) -> Self {
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
                                &std::env::current_exe()?,
                            )?;
                            // Persisted before attachment: a failed terminal handoff is recoverable.
                            result.agent = Some(session);
                            result.message =
                                "Session created · Enter opens · F12 or Ctrl-\\ returns to Waystation".into();
                        }
                        Effect::AttachAgent(id) => {
                            result.attach = Some(agents.attach(id, &std::env::current_exe()?)?)
                        }
                        Effect::InstallCodexHook => {
                            agents.install_codex_hook(&std::env::current_exe()?)?;
                            result.message =
                                "Codex hook added · Codex asks you to trust it on its next start"
                                    .into();
                        }
                        Effect::InstallStatusLine => {
                            agents.install_statusline(&std::env::current_exe()?)?;
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
