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
}
#[derive(Clone, Debug)]
pub enum Effect {
    Quit,
    Refresh,
}
#[derive(Clone, Debug)]
pub struct HitRegion {
    pub area: Rect,
    pub action: Action,
}
pub struct App {
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
}
impl App {
    pub fn new(config: Config, state: AppState) -> Self {
        Self {
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
        }
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
        match action {
            Action::Nav(s) => {
                self.section = s;
                self.selection = 0;
                self.searching = false;
                self.message = None
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
                self.message = None
            }
            Action::Help => self.help = !self.help,
            Action::FocusNext => self.pane = (self.pane + 1) % 4,
            Action::Move(n) => self.selection = self.selection.saturating_add_signed(n),
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
            Action::Activate if matches!(self.section, Section::Overview | Section::Workspaces) => {
                return self.update(Action::Project(
                    self.selection.min(self.workspaces.len().saturating_sub(1)),
                ));
            }
            Action::Activate if self.section == Section::Files => {
                if let Some(f) = self.files.get(self.selection).cloned() {
                    return self.update(Action::OpenPath(f.path));
                }
            }
            Action::OpenPath(p) if p.is_dir() => {
                self.file_dir = Some(p);
                self.selection = 0;
                return vec![Effect::Refresh];
            }
            _ => self.message = Some("This integration is being connected.".into()),
        };
        vec![]
    }
}
