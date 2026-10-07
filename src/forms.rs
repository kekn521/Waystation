use crate::{
    app::{Action, App, Effect},
    config::ToolCommand,
    model::Section,
};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Form {
    pub name: String,
    pub tool: usize,
    pub projects: Vec<PathBuf>,
    pub project: usize,
    pub field: usize,
    pub cursor: usize,
    pub busy: bool,
    pub error: Option<String>,
}
impl Form {
    fn text(&mut self) -> Option<&mut String> {
        match self.field {
            0 => Some(&mut self.name),
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
                    form.field = (*field).min(3);
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
                    form.field = (form.field as isize + n).rem_euclid(4) as usize;
                    form.end();
                }
                Action::FormCursor(n) => {
                    if form.field == 1 {
                        form.tool = (form.tool as isize + n).rem_euclid(2) as usize;
                    } else if form.field == 2 && !form.projects.is_empty() {
                        form.project = (form.project as isize + n)
                            .rem_euclid(form.projects.len() as isize)
                            as usize;
                    } else {
                        let count = form.text().map(|t| t.chars().count()).unwrap_or(0);
                        form.cursor = form.cursor.saturating_add_signed(*n).min(count);
                    }
                }
                Action::Activate if form.field < 3 => {
                    form.field += 1;
                    form.end();
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
                self.form = Some(Form {
                    name: String::new(),
                    tool: 0,
                    projects,
                    project,
                    field: 0,
                    cursor: 0,
                    busy: false,
                    error: None,
                });
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
