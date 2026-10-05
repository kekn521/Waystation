use super::*;
use crate::{config::TaskRecipe, tasks::RunStatus};
impl App {
    pub fn file_items(&self) -> Vec<(String, String, Action)> {
        let mut items = self
            .files
            .iter()
            .map(|f| {
                (
                    format!("{} {}", if f.is_dir { "▸" } else { " " }, f.label),
                    if f.is_dir {
                        "directory".into()
                    } else {
                        "Enter opens in editor".into()
                    },
                    Action::OpenPath(f.path.clone()),
                )
            })
            .collect::<Vec<_>>();
        if let Some(e) = self.provider_errors.get("Files") {
            items.insert(
                0,
                (
                    "Files unavailable · F5 retries".into(),
                    e.clone(),
                    Action::Reload,
                ),
            );
        }
        items
    }
    pub fn section_items(&self) -> Vec<(String, String, Action)> {
        let section = if self.section == Section::Overview {
            [
                Section::Overview,
                Section::Workspaces,
                Section::System,
                Section::Services,
            ][self.pane]
        } else {
            self.section
        };
        match section {
            Section::Workspaces => self
                .workspaces
                .iter()
                .map(|w| {
                    (
                        w.name.clone(),
                        w.id.display().to_string(),
                        Action::SelectWorkspace(w.id.clone()),
                    )
                })
                .collect(),
            Section::Agents => self.agent_items(),
            Section::Services => crate::ui::services::items(self),
            Section::Connections => self.connection_items(),
            Section::Files => self.file_items(),
            Section::System => vec![(
                "htop".into(),
                "Interactive process viewer".into(),
                Action::Tool("htop".into()),
            )],
            Section::Activity => self.activity_items(),
            Section::Overview => vec![],
        }
    }
    pub fn activity_items(&self) -> Vec<(String, String, Action)> {
        self.state
            .activity
            .iter()
            .rev()
            .filter(|a| match &a.kind {
                crate::model::ActivityKind::TaskRun(id) => self
                    .runs
                    .iter()
                    .any(|r| r.id.to_string() == *id && r.recipe.id.starts_with("tunnel:")),
                _ => true,
            })
            .map(|a| {
                let action = match &a.kind {
                    crate::model::ActivityKind::TaskRun(id) => uuid::Uuid::parse_str(id)
                        .map(Action::RunLog)
                        .unwrap_or(Action::ShowText(a.outcome.clone())),
                    _ => a
                        .workspace
                        .clone()
                        .map(Action::SelectWorkspace)
                        .unwrap_or(Action::ShowText(a.outcome.clone())),
                };
                (
                    a.outcome.clone(),
                    a.workspace
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    action,
                )
            })
            .collect()
    }
    fn selected_run(&self) -> Option<&crate::tasks::RunRecord> {
        if self.section != Section::Connections {
            return None;
        }
        let items = self.connection_items();
        let (_, _, Action::RunLog(id)) = items.get(self.selection)? else {
            return None;
        };
        self.runs
            .iter()
            .find(|r| r.id == *id && r.recipe.id.starts_with("tunnel:"))
    }
    pub fn task_action(&mut self, a: &Action) -> Option<Vec<Effect>> {
        let effects = match a {
            Action::ConfirmChoice(i) => {
                let choice = self.confirmation.as_ref()?.choices.get(*i)?.1.clone();
                self.confirmation = None;
                return Some(self.update(choice));
            }
            Action::StartTunnel(id) => {
                let t = self.config.tunnels.iter().find(|t| &t.id == id)?;
                let cwd = self
                    .workspace()
                    .map(Path::to_path_buf)
                    .unwrap_or(std::env::temp_dir());
                match TaskRecipe::from_tunnel(t, cwd) {
                    Ok(r) => return Some(self.preflight(r)),
                    Err(e) => {
                        self.message = Some(e.to_string());
                        vec![]
                    }
                }
            }
            Action::StartTask(recipe) => {
                self.section = Section::Connections;
                self.searching = false;
                self.selection = 0;
                vec![Effect::StartTask(recipe.clone())]
            }
            Action::RunLog(id) => vec![Effect::ReadLog(*id)],
            Action::Stop => {
                let r = self.selected_run()?;
                if !r.stoppable() {
                    self.message =
                        Some("This run is not verified as running; stop unavailable".into());
                    vec![]
                } else {
                    self.confirmation = Some(Confirmation {
                        title: format!("Stop {}?", r.recipe.label),
                        choices: vec![
                            ("Cancel".into(), Action::Escape),
                            ("Stop this tunnel".into(), Action::ConfirmStop(r.id)),
                        ],
                    });
                    self.modal_selection = 0;
                    vec![]
                }
            }
            Action::ConfirmStop(id) => vec![Effect::StopTask(*id)],
            Action::Quit if !self.pending_starts.is_empty() => {
                self.message =
                    Some("Waiting for background processes to appear; quit again shortly".into());
                vec![]
            }
            Action::Quit => {
                let running = self
                    .runs
                    .iter()
                    .filter(|r| matches!(r.status, RunStatus::Running | RunStatus::Starting))
                    .count();
                if running == 0 {
                    return None;
                }
                self.confirmation = Some(Confirmation {
                    title: format!("{running} background processes are still running"),
                    choices: vec![
                        ("Keep running and quit".into(), Action::QuitKeep),
                        ("Stop owned processes and quit".into(), Action::QuitStop),
                        ("Cancel".into(), Action::Escape),
                    ],
                });
                self.modal_selection = 0;
                vec![]
            }
            Action::QuitKeep => vec![Effect::Quit],
            Action::QuitStop
                if !self.pending_starts.is_empty()
                    || self.runs.iter().any(|r| r.status == RunStatus::Starting) =>
            {
                self.message=Some("Background processes are still starting; wait for ownership to be verified before stopping and quitting".into());
                vec![]
            }
            Action::QuitStop => vec![Effect::StopAllAndQuit(
                self.runs
                    .iter()
                    .filter(|r| r.stoppable())
                    .map(|r| r.id)
                    .collect(),
            )],
            _ => return None,
        };
        Some(effects)
    }
    fn preflight(&mut self, recipe: TaskRecipe) -> Vec<Effect> {
        let conflicts = self
            .services
            .listeners
            .value
            .as_ref()
            .map(|ls| {
                crate::providers::services::required_port_conflicts(&recipe.required_ports, ls)
            })
            .unwrap_or_default();
        if conflicts.is_empty() {
            return self.update(Action::StartTask(recipe));
        }
        self.confirmation = Some(Confirmation {
            title: format!("Required ports occupied: {conflicts:?}"),
            choices: vec![
                ("Cancel".into(), Action::Escape),
                ("Start anyway".into(), Action::StartTask(recipe)),
            ],
        });
        self.modal_selection = 0;
        vec![]
    }
}
