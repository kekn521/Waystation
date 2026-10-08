//! Where the Overview's orbit is looking: the galaxy of workspaces, one workspace with its
//! agents as moons, or one agent's preview. Levels hold identities, not indices, so provider
//! refreshes cannot point them at the wrong body.

use crate::{
    agents::AgentSession,
    app::{Action, App, Effect},
    model::{Section, Snapshot},
    providers::projects::Workspace,
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

/// How long a zoom takes.
pub const TRANSITION: Duration = Duration::from_millis(600);
/// At most this many planets circle the star.
pub const MAX_PLANETS: usize = 6;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Level {
    Galaxy,
    Workspace(PathBuf),
    Moon(PathBuf, Uuid),
}
impl Level {
    pub fn workspace(&self) -> Option<&Path> {
        match self {
            Self::Galaxy => None,
            Self::Workspace(p) | Self::Moon(p, _) => Some(p),
        }
    }
    pub fn agent(&self) -> Option<Uuid> {
        match self {
            Self::Moon(_, id) => Some(*id),
            _ => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Transition {
    pub from: Level,
    /// On the animation clock, `App::animation_elapsed`.
    pub started: Duration,
}
#[derive(Clone, Debug)]
pub struct OrbitView {
    pub level: Level,
    /// The focused body at this level: a planet in the galaxy, a moon otherwise.
    pub focus: usize,
    pub transition: Option<Transition>,
}
impl Default for OrbitView {
    fn default() -> Self {
        Self {
            level: Level::Galaxy,
            focus: 0,
            transition: None,
        }
    }
}
impl OrbitView {
    /// Eased progress of the current zoom: 0.0 when it starts, 1.0 once settled.
    pub fn progress(&self, now: Duration) -> f64 {
        let Some(t) = &self.transition else {
            return 1.0;
        };
        let p = (now.saturating_sub(t.started).as_secs_f64() / TRANSITION.as_secs_f64())
            .clamp(0.0, 1.0);
        p * p * (3.0 - 2.0 * p)
    }
    pub fn animating(&self, now: Duration) -> bool {
        self.transition
            .as_ref()
            .is_some_and(|t| now < t.started + TRANSITION)
    }
    pub fn zoomed(&self) -> bool {
        self.level != Level::Galaxy
    }
    /// Share of the orbit pane the moon system takes: all of it, or 40% beside a preview.
    pub fn split(&self, now: Duration) -> f64 {
        let share = |l: &Level| {
            if matches!(l, Level::Moon(..)) {
                0.4
            } else {
                1.0
            }
        };
        let to = share(&self.level);
        match &self.transition {
            Some(t) if t.from.workspace().is_some() => {
                let from = share(&t.from);
                from + (to - from) * self.progress(now)
            }
            _ => to,
        }
    }
    /// Move to `level`, animating only when the kind of view changes.
    fn go(&mut self, level: Level, focus: usize, now: Duration) {
        let from = std::mem::replace(&mut self.level, level);
        self.transition = (std::mem::discriminant(&from) != std::mem::discriminant(&self.level))
            .then_some(Transition { from, started: now });
        self.focus = focus;
    }
}

impl App {
    /// Workspaces drawn as planets: the selected one, then pinned, then recent ones, up to
    /// six; ordered pinned first (config order), then by path, so landing on a planet never
    /// reshuffles the system.
    pub fn planets(&self) -> Vec<&Workspace> {
        let mut planets: Vec<&Workspace> = vec![];
        let candidates = self
            .workspace()
            .into_iter()
            .chain(self.pinned.iter().map(PathBuf::as_path))
            .chain(self.state.recent_workspaces.iter().map(PathBuf::as_path));
        for path in candidates {
            if planets.len() == MAX_PLANETS {
                break;
            }
            if let Some(w) = self.workspaces.iter().find(|w| w.id == path)
                && !planets.iter().any(|p| p.id == w.id)
            {
                planets.push(w);
            }
        }
        let pin = |w: &Workspace| self.pinned.iter().position(|p| *p == w.id);
        planets.sort_by(|a, b| {
            (pin(a).unwrap_or(usize::MAX), &a.id).cmp(&(pin(b).unwrap_or(usize::MAX), &b.id))
        });
        planets
    }
    /// Agent sessions circling `workspace`, in the Agents list's order.
    pub fn moons(&self, workspace: &Path) -> Vec<&AgentSession> {
        let mut moons = self
            .agents
            .iter()
            .filter(|s| s.workspace == workspace)
            .collect::<Vec<_>>();
        moons.sort_by(|a, b| (&a.name, a.id).cmp(&(&b.name, b.id)));
        moons
    }
    /// Whether keys steer the orbit: its Overview pane has focus and nothing modal is open.
    pub fn orbit_focused(&self) -> bool {
        self.section == Section::Overview
            && self.pane == 0
            && !self.searching
            && !self.help
            && self.form.is_none()
            && self.confirmation.is_none()
            && self.detail.is_none()
    }
    pub fn previewed_agent(&self) -> Option<Uuid> {
        self.orbit.level.agent()
    }
    fn orbit_count(&self) -> usize {
        match self.orbit.level.workspace() {
            None => self.planets().len(),
            Some(p) => self.moons(p).len(),
        }
    }
    pub(crate) fn orbit_action(&mut self, action: &Action) -> Option<Vec<Effect>> {
        match action {
            Action::OrbitEnter(level) => return Some(self.orbit_enter(level.clone())),
            Action::OrbitOut => {
                self.pane = 0;
                return Some(self.orbit_out());
            }
            Action::OrbitAttach(id) => return Some(vec![Effect::AttachAgent(*id)]),
            _ => {}
        }
        if !self.orbit_focused() {
            return None;
        }
        match action {
            Action::Move(n) => Some(self.orbit_move(*n)),
            Action::Activate => Some(self.orbit_activate()),
            Action::Escape if self.orbit.zoomed() => Some(self.orbit_out()),
            _ => None,
        }
    }
    fn orbit_move(&mut self, n: isize) -> Vec<Effect> {
        let count = self.orbit_count();
        if count == 0 {
            return vec![];
        }
        self.orbit.focus = (self.orbit.focus as isize + n).rem_euclid(count as isize) as usize;
        if let Level::Moon(p, _) = &self.orbit.level {
            let p = p.clone();
            let id = self.moons(&p)[self.orbit.focus].id;
            self.orbit.level = Level::Moon(p, id);
            self.agent_preview = Some((id, Snapshot::default()));
            return vec![Effect::Preview];
        }
        vec![]
    }
    fn orbit_activate(&mut self) -> Vec<Effect> {
        let target = match &self.orbit.level {
            Level::Galaxy => self
                .planets()
                .get(self.orbit.focus)
                .map(|w| Level::Workspace(w.id.clone())),
            Level::Workspace(p) => self
                .moons(p)
                .get(self.orbit.focus)
                .map(|s| Level::Moon(p.clone(), s.id)),
            Level::Moon(_, id) => return vec![Effect::AttachAgent(*id)],
        };
        target.map(|l| self.orbit_enter(l)).unwrap_or_default()
    }
    fn orbit_enter(&mut self, level: Level) -> Vec<Effect> {
        self.pane = 0;
        let now = self.animation_elapsed;
        match &level {
            Level::Galaxy => self.orbit_out(),
            Level::Workspace(p) => {
                let effects = if self.workspace() == Some(p.as_path()) {
                    vec![]
                } else {
                    self.update(Action::SelectWorkspace(p.clone()))
                };
                self.orbit.go(level, 0, now);
                self.agent_preview = None;
                effects
            }
            Level::Moon(p, id) => {
                let focus = self.moons(p).iter().position(|s| s.id == *id).unwrap_or(0);
                self.agent_preview = Some((*id, Snapshot::default()));
                self.orbit.go(level, focus, now);
                vec![Effect::Preview]
            }
        }
    }
    fn orbit_out(&mut self) -> Vec<Effect> {
        let now = self.animation_elapsed;
        match self.orbit.level.clone() {
            Level::Galaxy => {}
            Level::Workspace(p) => {
                let focus = self.planets().iter().position(|w| w.id == p).unwrap_or(0);
                self.orbit.go(Level::Galaxy, focus, now);
            }
            Level::Moon(p, id) => {
                let focus = self.moons(&p).iter().position(|s| s.id == id).unwrap_or(0);
                self.orbit.go(Level::Workspace(p), focus, now);
                self.agent_preview = None;
            }
        }
        vec![]
    }
    /// After a refresh or a workspace change, fall back to the deepest level whose bodies
    /// still exist and still match the selected workspace.
    pub(crate) fn reconcile_orbit(&mut self) {
        let fallback = match &self.orbit.level {
            Level::Galaxy => None,
            Level::Workspace(p) | Level::Moon(p, _)
                if self.workspace() != Some(p.as_path())
                    || !self.planets().iter().any(|w| w.id == *p) =>
            {
                Some(Level::Galaxy)
            }
            Level::Moon(p, id) if !self.moons(p).iter().any(|s| s.id == *id) => {
                Some(Level::Workspace(p.clone()))
            }
            _ => None,
        };
        if let Some(level) = fallback {
            self.orbit.level = level;
            self.orbit.transition = None;
            self.agent_preview = None;
        }
        self.orbit.focus = self.orbit.focus.min(self.orbit_count().saturating_sub(1));
    }
}
