//! Where the Overview's orbit is looking: the galaxy of workspaces, or one workspace with its
//! agents as moons. Levels and the highlighted moon are identities, not indices, so provider
//! refreshes cannot point them at the wrong body.

use crate::{
    agents::AgentSession,
    app::{Action, App, Effect},
    model::Section,
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
}
impl Level {
    pub fn workspace(&self) -> Option<&Path> {
        match self {
            Self::Galaxy => None,
            Self::Workspace(p) => Some(p),
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
    /// The highlighted moon, so `focus` can be found again when the agents change.
    pub moon: Option<Uuid>,
    pub transition: Option<Transition>,
}
impl Default for OrbitView {
    fn default() -> Self {
        Self {
            level: Level::Galaxy,
            focus: 0,
            moon: None,
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
    /// Zoom to `level` with `focus` on its bodies.
    fn go(&mut self, level: Level, focus: usize, now: Duration) {
        let from = std::mem::replace(&mut self.level, level);
        self.transition = Some(Transition { from, started: now });
        self.focus = focus;
        self.moon = None;
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
            Action::OrbitAttach(id) => return Some(self.orbit_attach(*id)),
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
        if let Some(p) = self.orbit.level.workspace() {
            self.orbit.moon = Some(self.moons(p)[self.orbit.focus].id);
        }
        vec![]
    }
    fn orbit_activate(&mut self) -> Vec<Effect> {
        match &self.orbit.level {
            Level::Galaxy => self
                .planets()
                .get(self.orbit.focus)
                .map(|w| Level::Workspace(w.id.clone()))
                .map(|l| self.orbit_enter(l))
                .unwrap_or_default(),
            Level::Workspace(p) => self
                .moons(p)
                .get(self.orbit.focus)
                .map(|s| s.id)
                .map(|id| self.orbit_attach(id))
                .unwrap_or_default(),
        }
    }
    /// Open an agent full screen, highlighting its moon so returning lands on it.
    fn orbit_attach(&mut self, id: Uuid) -> Vec<Effect> {
        self.pane = 0;
        if self.orbit.zoomed() {
            self.orbit.moon = Some(id);
            self.reconcile_orbit();
        }
        vec![Effect::AttachAgent(id)]
    }
    fn orbit_enter(&mut self, level: Level) -> Vec<Effect> {
        self.pane = 0;
        let now = self.animation_elapsed;
        match &level {
            Level::Galaxy => self.orbit_out(),
            Level::Workspace(p) => {
                if self.orbit.level == level {
                    return vec![];
                }
                let effects = if self.workspace() == Some(p.as_path()) {
                    vec![]
                } else {
                    self.update(Action::SelectWorkspace(p.clone()))
                };
                self.orbit.go(level, 0, now);
                effects
            }
        }
    }
    fn orbit_out(&mut self) -> Vec<Effect> {
        if let Level::Workspace(p) = self.orbit.level.clone() {
            let focus = self.planets().iter().position(|w| w.id == p).unwrap_or(0);
            self.orbit.go(Level::Galaxy, focus, self.animation_elapsed);
        }
        vec![]
    }
    /// After a refresh or a workspace change, fall back to the deepest level whose bodies
    /// still exist and still match the selected workspace.
    pub(crate) fn reconcile_orbit(&mut self) {
        let fell_back = matches!(&self.orbit.level, Level::Workspace(p)
            if self.workspace() != Some(p.as_path())
                || !self.planets().iter().any(|w| w.id == *p));
        if fell_back {
            self.orbit.level = Level::Galaxy;
            self.orbit.transition = None;
            self.orbit.moon = None;
        }
        // Focus is an index, so re-find it from identities whenever they are known: the
        // highlighted moon, or the selected planet after a fall back to the galaxy.
        let found = match &self.orbit.level {
            Level::Workspace(p) => self
                .orbit
                .moon
                .and_then(|id| self.moons(p).iter().position(|s| s.id == id)),
            Level::Galaxy if fell_back => {
                let selected = self.workspace();
                self.planets()
                    .iter()
                    .position(|w| Some(w.id.as_path()) == selected)
            }
            Level::Galaxy => None,
        };
        self.orbit.focus = found
            .unwrap_or(self.orbit.focus)
            .min(self.orbit_count().saturating_sub(1));
        if let Level::Workspace(p) = &self.orbit.level {
            self.orbit.moon = self.moons(p).get(self.orbit.focus).map(|s| s.id);
        }
    }
}
