use super::*;
use crate::{config::TaskRecipe, tasks::RunStatus};
impl App {
    pub fn recipe_items(&self) -> Vec<(String, String, Action)> {
        self.config
            .tasks
            .iter()
            .map(|r| {
                (
                    r.label.clone(),
                    format!("{} {}", r.command.program, r.command.args.join(" ")),
                    Action::StartRecipe(r.id.clone()),
                )
            })
            .collect()
    }
    pub fn task_items(&self) -> Vec<(String, String, Action)> {
        if self.recipes {
            return self.recipe_items();
        }
        self.runs
            .iter()
            .map(|r| {
                (
                    format!("{}  · {:?}", r.recipe.label, r.status),
                    format!("{} · {}", r.cwd.display(), &r.id.to_string()[..8]),
                    Action::RunLog(r.id),
                )
            })
            .collect()
    }
    pub fn section_items(&self) -> Vec<(String, String, Action)> {
        let section = if self.section == Section::Overview {
            [
                Section::Workspaces,
                Section::System,
                Section::Tasks,
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
            Section::Tasks => self.task_items(),
            Section::Services => crate::ui::services::items(self),
            Section::Connections => self.connection_items(),
            Section::Files => self
                .files
                .iter()
                .map(|f| {
                    (
                        f.label.clone(),
                        f.path.display().to_string(),
                        Action::OpenPath(f.path.clone()),
                    )
                })
                .collect(),
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
        self.runs.get(self.selection)
    }
    pub fn task_action(&mut self, a: &Action) -> Option<Vec<Effect>> {
        let effects = match a {
            Action::ConfirmChoice(i) => {
                let choice = self.confirmation.as_ref()?.choices.get(*i)?.1.clone();
                self.confirmation = None;
                return Some(self.update(choice));
            }
            Action::Recipes => {
                self.section = Section::Tasks;
                self.recipes = !self.recipes;
                self.selection = 0;
                vec![]
            }
            Action::StartRecipe(id) => {
                let mut recipe = self.config.tasks.iter().find(|r| &r.id == id)?.clone();
                if recipe.cwd.is_relative() {
                    let Some(w) = self.workspace() else {
                        self.message =
                            Some("Select a workspace before starting this recipe".into());
                        return Some(vec![]);
                    };
                    recipe.cwd = w.join(&recipe.cwd);
                }
                return Some(self.preflight(recipe));
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
                self.recipes = false;
                self.section = Section::Tasks;
                vec![Effect::StartTask(recipe.clone())]
            }
            Action::RunLog(id) => vec![Effect::ReadLog(*id)],
            Action::Rerun => {
                let r = self.selected_run()?.recipe.clone();
                return Some(self.preflight(r));
            }
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
                            ("Stop this Station task".into(), Action::ConfirmStop(r.id)),
                        ],
                    });
                    self.modal_selection = 0;
                    vec![]
                }
            }
            Action::ConfirmStop(id) => vec![Effect::StopTask(*id)],
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
                    title: format!("{running} tasks are still running"),
                    choices: vec![
                        ("Keep tasks running and quit".into(), Action::QuitKeep),
                        ("Stop owned tasks and quit".into(), Action::QuitStop),
                        ("Cancel".into(), Action::Escape),
                    ],
                });
                self.modal_selection = 0;
                vec![]
            }
            Action::QuitKeep => vec![Effect::Quit],
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
