use crate::{
    config::Config,
    providers::{
        files::{self, FileEntry},
        git::{self, GitState},
        projects::{self, Workspace},
        system::{SystemSampler, SystemStats},
    },
    runtime::command::CommandRunner,
};
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderId {
    Agents,
    Projects,
    Git,
    Files,
    System,
    Connections,
    Sessions,
    Services,
    Tasks,
    Usage,
}
#[derive(Clone, Debug)]
pub struct ProviderRequest {
    pub id: ProviderId,
    pub generation: u64,
    pub workspace: Option<PathBuf>,
    pub directory: Option<PathBuf>,
    pub hidden: bool,
}
#[derive(Debug)]
pub enum ProviderPayload {
    Agents(
        Vec<crate::agents::AgentSession>,
        Option<crate::agents::CodexHookStatus>,
    ),
    Tasks(Vec<crate::tasks::RunRecord>),
    Services(crate::providers::services::ServicesState),
    Projects(Vec<Workspace>),
    Git(GitState),
    Files(Vec<FileEntry>),
    System(SystemStats),
    Aliases(Vec<String>),
    Sessions(Vec<(String, String, PathBuf)>),
    Usage(Box<crate::usage::Usage>),
}
#[derive(Debug)]
pub struct ProviderEvent {
    pub request: ProviderRequest,
    pub payload: Result<ProviderPayload, String>,
}
pub struct WorkerPool {
    tx: SyncSender<ProviderRequest>,
    rx: Receiver<ProviderEvent>,
    pending: Mutex<HashSet<(ProviderId, u64)>>,
}
impl WorkerPool {
    pub fn with_provider(
        f: impl Fn(&ProviderRequest) -> Result<ProviderPayload, String> + Send + Sync + 'static,
    ) -> Self {
        let (tx, rx) = mpsc::sync_channel::<ProviderRequest>(16);
        let (out, events) = mpsc::sync_channel(32);
        let rx = Arc::new(Mutex::new(rx));
        let f = Arc::new(f);
        for _ in 0..4 {
            let rx = rx.clone();
            let out = out.clone();
            let f = f.clone();
            thread::spawn(move || {
                loop {
                    let request = match rx.lock().unwrap().recv() {
                        Ok(r) => r,
                        Err(_) => break,
                    };
                    let payload = f(&request);
                    if out.send(ProviderEvent { request, payload }).is_err() {
                        break;
                    }
                }
            });
        }
        Self {
            tx,
            rx: events,
            pending: Mutex::new(HashSet::new()),
        }
    }
    pub fn new(config: Config, home: PathBuf, state: PathBuf) -> Self {
        let sampler = Mutex::new(SystemSampler::default());
        let scanner = {
            let (claude, codex) = crate::agents::agent_homes();
            Mutex::new(crate::usage::Scanner::new(claude, codex))
        };
        Self::with_provider(move |r| {
            let work = r
                .workspace
                .as_deref()
                .ok_or_else(|| "Select a workspace".to_string());
            let result: anyhow::Result<ProviderPayload> = (|| {
                Ok(match r.id {
                    ProviderId::Usage => {
                        // Usage only for the agents Waystation launches, by their real names.
                        let installed = |tool: &str| {
                            let program =
                                config.tools.get(tool).map_or(tool, |t| t.program.as_str());
                            crate::runtime::command::executable(program.as_ref())
                                .is_some_and(|p| p.file_name().is_some_and(|n| n == tool))
                        };
                        let statusline = crate::agents::AgentManager::new(state.clone())
                            .statusline_status(&std::env::current_exe()?);
                        let mut scanner = scanner.lock().unwrap_or_else(|e| e.into_inner());
                        ProviderPayload::Usage(Box::new(crate::usage::collect(
                            &mut scanner,
                            &state,
                            installed("claude"),
                            installed("codex"),
                            statusline,
                            crate::usage::now(),
                        )))
                    }
                    ProviderId::Agents => {
                        let manager = crate::agents::AgentManager::new(state.clone());
                        let codex = config
                            .tools
                            .get("codex")
                            .map_or("codex", |t| t.program.as_str());
                        // Resuming relies on the real `codex` CLI, not a differently named wrapper.
                        let hook = crate::runtime::command::executable(codex.as_ref())
                            .filter(|p| p.file_name().is_some_and(|n| n == "codex"))
                            .map(|_| anyhow::Ok(manager.codex_hook(&std::env::current_exe()?)))
                            .transpose()?;
                        ProviderPayload::Agents(manager.list()?, hook)
                    }
                    ProviderId::Tasks => ProviderPayload::Tasks(
                        crate::tasks::TaskManager::new(state.clone()).list()?,
                    ),
                    ProviderId::Projects => ProviderPayload::Projects(projects::discover(&config)?),
                    ProviderId::Git => ProviderPayload::Git(git::inspect(
                        work.map_err(anyhow::Error::msg)?,
                        &CommandRunner,
                    )?),
                    ProviderId::Files => {
                        let root = work.map_err(anyhow::Error::msg)?;
                        ProviderPayload::Files(files::list(
                            root,
                            r.directory.as_deref().unwrap_or(root),
                            r.hidden,
                        )?)
                    }
                    ProviderId::Connections => ProviderPayload::Aliases(
                        crate::providers::connections::aliases(&home.join(".ssh/config"))?,
                    ),
                    ProviderId::Sessions => ProviderPayload::Sessions(
                        crate::providers::sessions::list(&CommandRunner)?
                            .into_iter()
                            .map(|s| (s.id, s.name, s.cwd))
                            .collect(),
                    ),
                    ProviderId::Services => ProviderPayload::Services(
                        crate::providers::services::collect(&CommandRunner),
                    ),
                    ProviderId::System => ProviderPayload::System(
                        sampler
                            .lock()
                            .unwrap()
                            .sample(std::path::Path::new("/proc"), &home)?,
                    ),
                })
            })();
            result.map_err(|e| format!("{e:#}"))
        })
    }
    pub fn submit(&self, request: ProviderRequest) -> bool {
        let key = (request.id, request.generation);
        let mut pending = self.pending.lock().unwrap();
        if pending.contains(&key) {
            return false;
        }
        if self.tx.try_send(request).is_ok() {
            pending.insert(key);
            true
        } else {
            false
        }
    }
    pub fn try_recv(&self) -> Option<ProviderEvent> {
        let event = self.rx.try_recv().ok()?;
        self.pending
            .lock()
            .unwrap()
            .remove(&(event.request.id, event.request.generation));
        Some(event)
    }
}
