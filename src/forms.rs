use crate::{
    app::{Action, App, Effect},
    config::ToolCommand,
    model::Section,
};
use std::path::PathBuf;

/// What a form makes: an agent session (name, agent, project), a workspace (name and the
/// directory to create it in), or a workspace from an existing folder (its path).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormKind {
    Agent,
    Workspace,
    AddFolder,
}
#[derive(Clone, Debug)]
pub struct Form {
    pub kind: FormKind,
    pub name: String,
    pub tool: usize,
    /// The agent's project choices, or the project roots for a new workspace.
    pub projects: Vec<PathBuf>,
    pub project: usize,
    /// The typed directory a new workspace goes in, or the folder to add; it starts as
    /// the first root.
    pub location: String,
    pub field: usize,
    pub cursor: usize,
    pub busy: bool,
    pub error: Option<String>,
}
impl Form {
    fn text(&mut self) -> Option<&mut String> {
        match (self.kind, self.field) {
            (FormKind::AddFolder, 0) | (FormKind::Workspace, 1) => Some(&mut self.location),
            (_, 0) => Some(&mut self.name),
            _ => None,
        }
    }
    fn end(&mut self) {
        self.cursor = self.text().map(|t| t.chars().count()).unwrap_or(0);
    }
    pub fn tool_name(&self) -> &'static str {
        ["codex", "claude"][self.tool]
    }
    pub fn workspace(&self) -> Option<&PathBuf> {
        self.projects.get(self.project)
    }
    /// Focus stops, the last being the button.
    pub fn fields(&self) -> usize {
        match self.kind {
            FormKind::Agent => 4,
            FormKind::Workspace => 3,
            FormKind::AddFolder => 2,
        }
    }
    /// The field that picks from `projects`.
    pub fn project_field(&self) -> Option<usize> {
        match self.kind {
            FormKind::Agent => Some(2),
            FormKind::Workspace => Some(1),
            FormKind::AddFolder => None,
        }
    }
    /// The typed location as an absolute path, `~/` expanded, or why it is not one.
    fn location_path(&self) -> Result<PathBuf, &'static str> {
        let location = self.location.trim();
        if location.is_empty() {
            return Err("Enter a location, like ~/code");
        }
        let home = std::env::home_dir().unwrap_or_default();
        let path = crate::config::expand_tilde(location.as_ref(), &home);
        if path.is_relative() {
            return Err("Enter a full path, like ~/code");
        }
        Ok(path)
    }
    fn new(kind: FormKind, projects: Vec<PathBuf>, project: usize) -> Self {
        let root = projects.get(project).map(|p| p.display().to_string());
        let location = match kind {
            FormKind::Agent => String::new(),
            FormKind::Workspace => root.unwrap_or_default(),
            // Ready for the folder's name under the first root.
            FormKind::AddFolder => root.map(|r| format!("{r}/")).unwrap_or_default(),
        };
        Self {
            kind,
            name: String::new(),
            tool: 0,
            projects,
            project,
            location,
            field: 0,
            cursor: 0,
            busy: false,
            error: None,
        }
    }
}
impl App {
    pub fn workflow_action(&mut self, action: &Action) -> Option<Vec<Effect>> {
        if let Some(form) = &mut self.form {
            if form.busy {
                return Some(vec![]);
            }
            form.error = None;
            match action {
                Action::Escape | Action::Quit => self.form = None,
                Action::Insert(c) => {
                    let cursor = form.cursor;
                    if !c.is_control()
                        && let Some(text) = form.text()
                        && text.len() < 8192
                    {
                        let index = text
                            .char_indices()
                            .nth(cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(text.len());
                        text.insert(index, *c);
                        form.cursor += 1;
                    }
                }
                Action::Paste(value) => {
                    let cursor = form.cursor;
                    if value.chars().any(char::is_control) {
                        form.error = Some("Paste a single line without control characters".into());
                    } else if let Some(text) = form.text() {
                        if text.len() + value.len() <= 8192 {
                            let index = text
                                .char_indices()
                                .nth(cursor)
                                .map(|(i, _)| i)
                                .unwrap_or(text.len());
                            text.insert_str(index, value);
                            form.cursor += value.chars().count();
                        } else {
                            form.error = Some("Input is limited to 8192 bytes".into());
                        }
                    }
                }
                Action::FormFocus(field) => {
                    form.field = (*field).min(form.fields() - 1);
                    form.end();
                }
                Action::Backspace => {
                    let cursor = form.cursor;
                    if cursor > 0
                        && let Some(text) = form.text()
                    {
                        let start = text
                            .char_indices()
                            .nth(cursor - 1)
                            .map(|(i, _)| i)
                            .unwrap_or(text.len());
                        let end = text
                            .char_indices()
                            .nth(cursor)
                            .map(|(i, _)| i)
                            .unwrap_or(text.len());
                        text.replace_range(start..end, "");
                        form.cursor -= 1;
                    }
                }
                Action::FormClear => {
                    if let Some(text) = form.text() {
                        text.clear();
                        form.cursor = 0;
                    }
                }
                Action::FormField(n) => {
                    form.field =
                        (form.field as isize + n).rem_euclid(form.fields() as isize) as usize;
                    form.end();
                }
                // The workspace location is typed, so only FormChoice steps through its roots.
                Action::FormCursor(n) | Action::FormChoice(n) => {
                    let choose =
                        form.kind == FormKind::Agent || matches!(action, Action::FormChoice(_));
                    if form.kind == FormKind::Agent && form.field == 1 {
                        form.tool = (form.tool as isize + n).rem_euclid(2) as usize;
                    } else if choose
                        && Some(form.field) == form.project_field()
                        && !form.projects.is_empty()
                    {
                        form.project = (form.project as isize + n)
                            .rem_euclid(form.projects.len() as isize)
                            as usize;
                        if form.kind == FormKind::Workspace {
                            form.location = form.projects[form.project].display().to_string();
                            form.end();
                        }
                    } else {
                        let count = form.text().map(|t| t.chars().count()).unwrap_or(0);
                        form.cursor = form.cursor.saturating_add_signed(*n).min(count);
                    }
                }
                Action::Activate if form.field < form.fields() - 1 => {
                    form.field += 1;
                    form.end();
                }
                Action::Activate | Action::FormSave if form.kind == FormKind::AddFolder => {
                    match form.location_path() {
                        Ok(dir) => {
                            form.busy = true;
                            return Some(vec![Effect::AddWorkspace(dir)]);
                        }
                        Err(e) => form.error = Some(e.into()),
                    }
                }
                Action::Activate | Action::FormSave if form.kind == FormKind::Workspace => {
                    let root = match form.location_path() {
                        Ok(root) => root,
                        Err(e) => {
                            form.error = Some(e.into());
                            return Some(vec![]);
                        }
                    };
                    let name = form.name.trim();
                    if let Err(e) = crate::providers::projects::validate_name(name) {
                        form.error = Some(e.into());
                        return Some(vec![]);
                    }
                    let effect = Effect::CreateWorkspace {
                        root,
                        name: name.into(),
                    };
                    form.busy = true;
                    return Some(vec![effect]);
                }
                Action::Activate | Action::FormSave => {
                    let Some(workspace) = form.workspace().cloned() else {
                        form.error = Some("Select a workspace in Workspaces first".into());
                        return Some(vec![]);
                    };
                    if form.name.trim().is_empty() || form.name.len() > 200 {
                        form.error = Some("Enter a name (up to 200 bytes)".into());
                        return Some(vec![]);
                    }
                    let tool = form.tool_name().to_string();
                    let command = self
                        .config
                        .tools
                        .get(&tool)
                        .cloned()
                        .unwrap_or(ToolCommand {
                            program: tool.clone(),
                            args: vec![],
                        });
                    let effect = Effect::CreateAgent {
                        name: form.name.trim().into(),
                        tool,
                        command,
                        workspace,
                    };
                    form.busy = true;
                    return Some(vec![effect]);
                }
                _ => {}
            }
            return Some(vec![]);
        }
        match action {
            Action::NewAgent => {
                let mut projects = self
                    .workspaces
                    .iter()
                    .map(|w| w.id.clone())
                    .collect::<Vec<_>>();
                if let Some(p) = self.workspace()
                    && !projects.iter().any(|w| w == p)
                {
                    projects.insert(0, p.to_path_buf());
                }
                let project = projects
                    .iter()
                    .position(|p| Some(p.as_path()) == self.workspace())
                    .unwrap_or(0);
                self.form = Some(Form::new(FormKind::Agent, projects, project));
                Some(vec![])
            }
            Action::NewWorkspace => {
                let roots = self.config.project_roots.clone();
                self.form = Some(Form::new(FormKind::Workspace, roots, 0));
                Some(vec![])
            }
            Action::AddFolder => {
                let roots = self.config.project_roots.clone();
                let mut form = Form::new(FormKind::AddFolder, roots, 0);
                form.end();
                self.form = Some(form);
                Some(vec![])
            }
            Action::OpenAgent(id) => {
                let mut effects = vec![];
                if let Some(session) = self.agents.iter().find(|s| s.id == *id) {
                    let workspace = session.workspace.clone();
                    effects.extend(self.update(Action::SelectWorkspace(workspace)));
                    self.section = Section::Agents;
                    self.selection = self
                        .agent_items()
                        .iter()
                        .position(|(_, _, a)| matches!(a, Action::OpenAgent(s) if s == id))
                        .unwrap_or(0);
                }
                effects.push(Effect::AttachAgent(*id));
                Some(effects)
            }
            Action::Stop if self.section == Section::Agents => {
                if let Some((_, _, Action::OpenAgent(id))) = self.agent_items().get(self.selection)
                    && let Some(session) = self.agents.iter().find(|s| s.id == *id)
                {
                    self.confirmation = Some(crate::app::Confirmation {
                        title: format!(
                            "Close {}? Its process and scrollback will end.",
                            session.name
                        ),
                        choices: vec![
                            ("Cancel".into(), Action::Escape),
                            ("Close this agent session".into(), Action::CloseAgent(*id)),
                        ],
                    });
                    self.modal_selection = 0;
                }
                Some(vec![])
            }
            Action::CloseAgent(id) => Some(vec![Effect::CloseAgent(*id)]),
            Action::Stop
                if self.section == Section::Workspaces
                    || (self.orbit_focused() && !self.orbit.zoomed()) =>
            {
                let target = if self.section == Section::Workspaces {
                    self.workspaces.get(self.selection)
                } else {
                    self.planets().get(self.orbit.focus).copied()
                };
                if let Some(w) = target {
                    let agents = self.agents.iter().filter(|s| s.workspace == w.id).count();
                    let running = match agents {
                        0 => String::new(),
                        1 => " 1 agent session works there.".into(),
                        n => format!(" {n} agent sessions work there."),
                    };
                    self.confirmation = Some(crate::app::Confirmation {
                        title: format!(
                            "Remove {} ({})? Removing keeps its files; the trash keeps them restorable.{running}",
                            w.name,
                            w.id.display()
                        ),
                        choices: vec![
                            ("Cancel".into(), Action::Escape),
                            (
                                "Remove from Waystation".into(),
                                Action::RemoveWorkspace(w.id.clone()),
                            ),
                            (
                                "Move folder to trash".into(),
                                Action::TrashWorkspace(w.id.clone()),
                            ),
                        ],
                    });
                    self.modal_selection = 0;
                }
                Some(vec![])
            }
            Action::RemoveWorkspace(dir) => Some(vec![Effect::RemoveWorkspace {
                dir: dir.clone(),
                trash: false,
            }]),
            Action::TrashWorkspace(dir) => Some(vec![Effect::RemoveWorkspace {
                dir: dir.clone(),
                trash: true,
            }]),
            Action::RestartAgent if self.section == Section::Agents => {
                if let Some((_, _, Action::OpenAgent(id))) = self.agent_items().get(self.selection)
                    && let Some(session) = self.agents.iter().find(|s| s.id == *id)
                {
                    let title = if session.conversation.is_some() {
                        format!(
                            "Restart {}? It resumes the same conversation with status tracking.",
                            session.name
                        )
                    } else {
                        format!(
                            "Restart {}? It has no saved conversation, so it starts a fresh one with status tracking.",
                            session.name
                        )
                    };
                    self.confirmation = Some(crate::app::Confirmation {
                        title,
                        choices: vec![
                            ("Cancel".into(), Action::Escape),
                            ("Restart".into(), Action::ConfirmRestartAgent(*id)),
                        ],
                    });
                    self.modal_selection = 0;
                }
                Some(vec![])
            }
            Action::ConfirmRestartAgent(id) => Some(vec![Effect::RestartAgent(*id)]),
            Action::InstallStatusLine => {
                self.confirmation = Some(crate::app::Confirmation {
                    title: "Set Claude's status line in ~/.claude/settings.json to Waystation's? It only saves Claude's plan limits for Waystation; nothing shows in Claude.".into(),
                    choices: vec![
                        ("Cancel".into(), Action::Escape),
                        ("Set the status line".into(), Action::ConfirmInstallStatusLine),
                    ],
                });
                self.modal_selection = 0;
                Some(vec![])
            }
            Action::ConfirmInstallStatusLine => Some(vec![Effect::InstallStatusLine]),
            _ => None,
        }
    }
}
impl App {
    /// Take in a workspace the New workspace or Add existing folder form just made: list it
    /// before the next scan finds it, and select it. Started from the orbit, the view lands on its planet.
    /// `pinned` says the job pinned it in config.toml, which this run's scans do not see.
    pub fn adopt_new_workspace(&mut self, dir: PathBuf, pinned: bool) -> Vec<Effect> {
        self.form = None;
        self.hidden_workspaces.remove(&dir);
        if pinned && !self.pinned.contains(&dir) {
            self.pinned.push(dir.clone());
            self.pinned_paths.insert(dir.clone());
            self.new_pins.push(dir.clone());
        }
        if !self.workspaces.iter().any(|w| w.id == dir) {
            self.workspaces
                .push(crate::providers::projects::Workspace::new(dir.clone()));
        }
        if self.orbit_focused() {
            return self.update(Action::OrbitEnter(crate::orbit_nav::Level::Workspace(dir)));
        }
        self.update(Action::SelectWorkspace(dir))
    }
    /// Drop a workspace the job removed or trashed: off the list, the pins and the recent
    /// ones for good this run, with another one selected if it was the selected one.
    pub fn forget_workspace(&mut self, dir: PathBuf) -> Vec<Effect> {
        self.workspaces.retain(|w| w.id != dir);
        self.pinned.retain(|p| *p != dir);
        self.pinned_paths.remove(&dir);
        self.new_pins.retain(|p| *p != dir);
        self.state.recent_workspaces.retain(|p| *p != dir);
        let selected = self.workspace() == Some(dir.as_path());
        self.hidden_workspaces.insert(dir);
        self.selection = self.selection.min(self.workspaces.len().saturating_sub(1));
        let mut effects = vec![];
        if selected {
            self.state.selected_workspace = None;
            self.file_dir = None;
            if let Some(next) = self.workspaces.first().map(|w| w.id.clone()) {
                effects = self.update(Action::SelectWorkspace(next));
            }
        }
        self.reconcile_orbit();
        effects
    }
    /// Take in an agent the New agent form just created and say how to open it. Started
    /// from a zoomed orbit on its workspace, it opens as that system's highlighted moon, so
    /// returning from it lands back on the orbit; otherwise it is selected in Agents.
    pub fn adopt_new_agent(&mut self, agent: crate::agents::AgentSession) -> Vec<Effect> {
        let id = agent.id;
        let workspace = agent.workspace.clone();
        // The form that created it is closed first; it is what hid the orbit's focus.
        self.form = None;
        let from_orbit =
            self.orbit_focused() && self.orbit.level.workspace() == Some(workspace.as_path());
        self.agents.retain(|s| s.id != id);
        self.agents.push(agent);
        if from_orbit {
            return self.update(Action::OrbitAttach(id));
        }
        self.update(Action::SelectWorkspace(workspace));
        self.section = Section::Agents;
        self.selection = self
            .agent_items()
            .iter()
            .position(|(_, _, a)| matches!(a, Action::OpenAgent(s) if *s == id))
            .unwrap_or(0);
        vec![Effect::AttachAgent(id)]
    }
}
