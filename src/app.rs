#[path = "app_tasks.rs"]
mod task_actions;
use crate::{
    config::Config,
    model::{AppState, Section},
};
use crate::{
    model::Snapshot,
    providers::{files::FileEntry, git::GitState, projects::Workspace},
    search::{SearchItem, rank},
};
use ratatui::layout::Rect;
use std::path::{Path, PathBuf};
#[derive(Clone, Debug)]
pub enum Action {
    Nav(Section),
    Move(isize),
    FocusNext,
    Search,
    Insert(char),
    Backspace,
    Escape,
    Help,
    Quit,
    Activate,
    Editor,
    Shell,
    Git,
    Herdr,
    Files,
    Reload,
    Hidden,
    Copy,
    Stop,
    Rerun,
    Recipes,
    Project(usize),
    SelectWorkspace(PathBuf),
    OpenPath(PathBuf),
    Tool(String),
    Connect(String),
    Attach(String),
    DockerLogs(String),
    ShowText(String),
    StartRecipe(String),
    StartTunnel(String),
    StartTask(crate::config::TaskRecipe),
    RunLog(uuid::Uuid),
    ConfirmStop(uuid::Uuid),
    QuitKeep,
    QuitStop,
    ConfirmChoice(usize),
}
#[derive(Clone, Debug)]
pub enum Effect {
    Foreground(Action),
    Copy(PathBuf),
    DockerLogs(String),
    StartTask(crate::config::TaskRecipe),
    StopTask(uuid::Uuid),
    ReadLog(uuid::Uuid),
    StopAllAndQuit(Vec<uuid::Uuid>),
    Quit,
    Refresh,
}
#[derive(Clone, Debug)]
pub struct HitRegion {
    pub area: Rect,
    pub action: Action,
}
#[derive(Clone, Debug)]
pub struct Confirmation {
    pub title: String,
    pub choices: Vec<(String, Action)>,
}
pub struct App {
    pub runs: Vec<crate::tasks::RunRecord>,
    pub recipes: bool,
    pub confirmation: Option<Confirmation>,
    pub modal_selection: usize,
    pub detail: Option<(String, String)>,
    pub detail_scroll: u16,
    pub config: Config,
    pub state: AppState,
    pub section: Section,
    pub pane: usize,
    pub selection: usize,
    pub query: String,
    pub searching: bool,
    pub help: bool,
    pub message: Option<String>,
    pub hits: Vec<HitRegion>,
    pub workspaces: Vec<Workspace>,
    pub git: Snapshot<GitState>,
    pub files: Vec<FileEntry>,
    pub file_dir: Option<PathBuf>,
    pub hidden: bool,
    pub generation: u64,
    pub services: crate::providers::services::ServicesState,
    pub system: Snapshot<crate::providers::system::SystemStats>,
    pub cpu_history: std::collections::VecDeque<u64>,
    pub provider_errors: std::collections::BTreeMap<String, String>,
    pub aliases: Vec<String>,
    pub sessions: Vec<(String, String, PathBuf)>,
}
impl App {
    pub fn new(config: Config, state: AppState) -> Self {
        Self {
            runs: vec![],
            recipes: false,
            confirmation: None,
            modal_selection: 0,
            detail: None,
            detail_scroll: 0,
            config,
            state,
            section: Section::Overview,
            pane: 0,
            selection: 0,
            query: String::new(),
            searching: false,
            help: false,
            message: None,
            hits: vec![],
            workspaces: vec![],
            git: Snapshot::default(),
            files: vec![],
            file_dir: None,
            hidden: false,
            generation: 0,
            services: Default::default(),
            system: Snapshot::default(),
            cpu_history: std::collections::VecDeque::new(),
            provider_errors: Default::default(),
            aliases: vec![],
            sessions: vec![],
        }
    }
    pub fn apply_provider(&mut self, event: crate::runtime::workers::ProviderEvent) -> bool {
        use crate::runtime::workers::{ProviderId, ProviderPayload};
        if event.request.generation != self.generation {
            return false;
        }
        let key = format!("{:?}", event.request.id);
        match event.payload {
            Ok(payload) => {
                self.provider_errors.remove(&key);
                match payload {
                    ProviderPayload::Tasks(r) => {
                        for run in &r {
                            if run.ended.is_some()
                                && !self
                                    .state
                                    .activity
                                    .iter()
                                    .any(|a| a.id == run.id.to_string())
                            {
                                self.state.activity.push(crate::model::ActivityEntry {
                                    id: run.id.to_string(),
                                    at: run.ended.unwrap_or(run.started),
                                    workspace: Some(run.cwd.clone()),
                                    kind: crate::model::ActivityKind::TaskRun(run.id.to_string()),
                                    outcome: format!("{} · {:?}", run.recipe.label, run.status),
                                });
                            }
                        }
                        self.runs = r;
                    }
                    ProviderPayload::Services(s) => self.services = s,
                    ProviderPayload::Projects(w) => self.set_workspaces(w),
                    ProviderPayload::Git(g) => self.git = Snapshot::ready(g, self.generation),
                    ProviderPayload::Files(f) => {
                        if event.request.directory != self.file_dir
                            || event.request.hidden != self.hidden
                        {
                            return false;
                        }
                        self.files = f;
                    }
                    ProviderPayload::System(s) => {
                        if let Some(cpu) = s.cpu {
                            self.cpu_history.push_back(cpu as u64);
                            if self.cpu_history.len() > 60 {
                                self.cpu_history.pop_front();
                            }
                        }
                        self.system = Snapshot::ready(s, self.generation);
                    }
                    ProviderPayload::Aliases(a) => self.aliases = a,
                    ProviderPayload::Sessions(s) => self.sessions = s,
                }
            }
            Err(e) => {
                if event.request.id == ProviderId::Git {
                    self.git.availability = crate::model::Availability::Failed(e.clone())
                }
                if event.request.id == ProviderId::System {
                    self.system.availability = crate::model::Availability::Failed(e.clone())
                }
                self.provider_errors.insert(key, e);
            }
        }
        true
    }
    pub fn workspace(&self) -> Option<&Path> {
        self.state.selected_workspace.as_deref()
    }
    pub fn set_workspaces(&mut self, items: Vec<Workspace>) {
        self.workspaces = items;
        if self.state.selected_workspace.is_none() {
            self.state.selected_workspace = self.workspaces.first().map(|w| w.id.clone());
        }
        if self.file_dir.is_none() {
            self.file_dir = self.state.selected_workspace.clone();
        }
    }
    pub fn agent_items(&self) -> Vec<(String, String, Action)> {
        ["herdr", "codex", "claude"]
            .iter()
            .map(|name| {
                let program = self
                    .config
                    .tools
                    .get(*name)
                    .map(|t| t.program.as_str())
                    .unwrap_or(name);
                let available = crate::runtime::command::executable(program.as_ref()).is_some();
                (
                    name.to_string(),
                    if available {
                        "Ready · Enter to launch"
                    } else {
                        "Unavailable · configure tools in config.toml"
                    }
                    .into(),
                    Action::Tool(name.to_string()),
                )
            })
            .collect()
    }
    pub fn connection_items(&self) -> Vec<(String, String, Action)> {
        let mut items = self
            .aliases
            .iter()
            .map(|h| {
                (
                    h.clone(),
                    "Saved SSH alias · Enter to connect".into(),
                    Action::Connect(h.clone()),
                )
            })
            .collect::<Vec<_>>();
        items.extend(self.sessions.iter().map(|(id, name, cwd)| {
            (
                name.clone(),
                format!("tmux · {}", cwd.display()),
                Action::Attach(id.clone()),
            )
        }));
        items.extend(self.config.tunnels.iter().map(|t| {
            (
                format!("Tunnel {}", t.id),
                format!(
                    "{}:{} → {}:{}",
                    t.bind, t.local_port, t.remote_host, t.remote_port
                ),
                Action::StartTunnel(t.id.clone()),
            )
        }));
        items
    }
    pub fn search_items(&self) -> Vec<SearchItem> {
        let mut items = self
            .workspaces
            .iter()
            .map(|w| SearchItem {
                id: w.id.to_string_lossy().into_owned(),
                label: w.name.clone(),
                detail: w.id.display().to_string(),
                action: Action::SelectWorkspace(w.id.clone()),
            })
            .collect::<Vec<_>>();
        for (label, action) in [
            ("Open editor", Action::Editor),
            ("Open shell", Action::Shell),
            ("Open Herdr", Action::Herdr),
            ("Git changes", Action::Git),
            ("Browse files", Action::Files),
            ("Refresh providers", Action::Reload),
        ] {
            items.push(SearchItem {
                id: label.into(),
                label: label.into(),
                detail: "selected workspace".into(),
                action,
            })
        }
        for (label, detail, action) in self
            .agent_items()
            .into_iter()
            .chain(self.connection_items())
            .chain(self.recipe_items())
        {
            items.push(SearchItem {
                id: label.clone(),
                label,
                detail,
                action,
            })
        }
        for s in Section::ALL {
            items.push(SearchItem {
                id: s.name().into(),
                label: s.name().into(),
                detail: "section".into(),
                action: Action::Nav(s),
            })
        }
        for f in &self.files {
            items.push(SearchItem {
                id: f.path.to_string_lossy().into_owned(),
                label: f.label.clone(),
                detail: "file".into(),
                action: Action::OpenPath(f.path.clone()),
            })
        }
        items
    }
    pub fn matches(&self) -> Vec<SearchItem> {
        let all = self.search_items();
        rank(&self.query, &all)
            .into_iter()
            .map(|i| all[i].clone())
            .collect()
    }
    pub fn update(&mut self, action: Action) -> Vec<Effect> {
        if let Some(e) = self.task_action(&action) {
            return e;
        }
        if self.confirmation.is_some() {
            match action {
                Action::Move(n) => {
                    let count = self.confirmation.as_ref().unwrap().choices.len();
                    self.modal_selection = self
                        .modal_selection
                        .saturating_add_signed(n)
                        .min(count.saturating_sub(1));
                }
                Action::Activate => {
                    return self.update(Action::ConfirmChoice(self.modal_selection));
                }
                Action::Escape | Action::Quit => self.confirmation = None,
                _ => {}
            }
            return vec![];
        }
        if self.detail.is_some() {
            match action {
                Action::Move(n) => {
                    self.detail_scroll = self.detail_scroll.saturating_add_signed(n as i16)
                }
                Action::Escape => self.detail = None,
                Action::Quit => {}
                _ => return vec![],
            }
            if !matches!(action, Action::Quit) {
                return vec![];
            }
        }

        match action {
            Action::Nav(s) => {
                self.section = s;
                self.selection = 0;
                self.searching = false;
                self.message = None;
                self.detail = None;
                self.confirmation = None;
            }
            Action::Search => {
                self.searching = true;
                self.selection = 0;
                self.query.clear()
            }
            Action::Insert(c) => self.query.push(c),
            Action::Backspace => {
                self.query.pop();
            }
            Action::Escape => {
                self.searching = false;
                self.query.clear();
                self.help = false;
                self.message = None;
                self.detail = None;
                self.confirmation = None;
            }
            Action::Help => self.help = !self.help,
            Action::FocusNext => {
                self.pane = (self.pane + 1) % 4;
                self.selection = 0;
            }
            Action::Move(n) => {
                let count = if self.searching {
                    self.matches().len()
                } else {
                    self.section_items().len()
                };
                self.selection = self
                    .selection
                    .saturating_add_signed(n)
                    .min(count.saturating_sub(1));
            }
            Action::Quit => return vec![Effect::Quit],
            Action::SelectWorkspace(p) => {
                self.state.selected_workspace = Some(p.clone());
                self.state.recent_workspaces.retain(|x| x != &p);
                self.state.recent_workspaces.insert(0, p.clone());
                self.file_dir = Some(p);
                self.git = Snapshot::default();
                self.files.clear();
                self.generation += 1;
                self.selection = 0;
                self.searching = false;
                return vec![Effect::Refresh];
            }
            Action::Project(i) => {
                if let Some(w) = self.workspaces.get(i) {
                    return self.update(Action::SelectWorkspace(w.id.clone()));
                }
            }
            Action::Files => {
                self.section = Section::Files;
                self.selection = 0;
                return vec![Effect::Refresh];
            }
            Action::Hidden => {
                self.hidden = !self.hidden;
                return vec![Effect::Refresh];
            }
            Action::Reload => return vec![Effect::Refresh],
            Action::Activate if self.searching => {
                if let Some(item) = self
                    .matches()
                    .get(self.selection.min(self.matches().len().saturating_sub(1)))
                    .cloned()
                {
                    self.searching = false;
                    return self.update(item.action);
                }
            }
            Action::Activate => {
                if let Some((_, _, a)) = self.section_items().get(self.selection).cloned() {
                    return self.update(a);
                }
            }
            Action::OpenPath(p) if p.is_dir() => {
                self.file_dir = Some(p);
                self.selection = 0;
                return vec![Effect::Refresh];
            }
            Action::DockerLogs(id) => return vec![Effect::DockerLogs(id)],
            Action::ShowText(text) => self.detail = Some(("Details".into(), text)),
            Action::Editor
            | Action::Shell
            | Action::Git
            | Action::Herdr
            | Action::Tool(_)
            | Action::Connect(_)
            | Action::Attach(_)
            | Action::OpenPath(_) => return vec![Effect::Foreground(action)],
            Action::Copy => {
                if let Some(p) = self
                    .files
                    .get(self.selection)
                    .map(|f| f.path.clone())
                    .or_else(|| self.state.selected_workspace.clone())
                {
                    return vec![Effect::Copy(p)];
                }
            }
            _ => {}
        };
        vec![]
    }
}
