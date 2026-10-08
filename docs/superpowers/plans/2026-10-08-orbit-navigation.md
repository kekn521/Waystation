# Orbit Navigation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Overview's decorative orbit into navigation: planets are workspaces, moons are their agents, a moon shows a live preview of the agent's screen and Enter attaches.

**Architecture:** A new `orbit_nav` module owns the zoom state (`Level`, `OrbitView`) and the `App` methods that route keys and clicks to it. `ui/orbit.rs` becomes a pure scene builder (rings + bodies in pane-local cell coordinates, viewed through a camera for zoom animations) plus a painter that also returns the hit regions. A new `AgentPreview` provider captures the agent's tmux screen off the UI thread, and `ui/preview.rs` draws it.

**Tech Stack:** Rust 2024, ratatui 0.30 (Canvas/braille), crossterm 0.29, tmux, insta snapshots.

**Spec:** `docs/superpowers/specs/2026-10-08-orbit-navigation-design.md`

## Global Constraints

- At most **6** planets; membership priority selected → pinned → recent; order pinned (config order) then by path.
- Zoom transition **600 ms**, smoothstep easing; poll every **40 ms** while animating, 100 ms otherwise; idle orbit motion keeps 8 fps quantisation.
- Moon view splits the orbit pane **40 / 60** (horizontal; vertical in the Single layout).
- Preview refreshes on the existing 1-second tick, only while a moon is open.
- Rendering must never touch the filesystem or run subprocesses.
- Theme colours come from `src/ui/theme.rs`; all user text passes through `ui::safe`.

## Review Focus

- A workspace or agent disappears while zoomed → the orbit falls back a level instead of showing a dead body or panicking (Task 1 reconcile test).
- The user selects another workspace from search or the Agents section while zoomed → the orbit returns to the galaxy rather than showing the wrong system (Task 1).
- A slow tmux capture for a moon the user already left → that late preview is ignored (Task 2 stale payload test).
- Labels near the right edge → flipped left, never clipped or spilling out of the pane (Task 3 label test).
- Returning from an attached agent → the user lands on the same zoom level, not the Agents section (Task 4 `main.rs` change; Task 1 `OrbitAttach` test asserts section unchanged).

---

### Task 1: Orbit navigation state and routing

**Files:**
- Create: `src/orbit_nav.rs`
- Modify: `src/lib.rs`, `src/app.rs` (fields, `Action`/`Effect` variants, `update` hook, `FocusNext`, `apply_provider`, `SelectWorkspace`), `src/input.rs` (`n` key)
- Test: `tests/orbit_nav.rs` (new), `tests/task_ui.rs` (rename the inert test)

**Interfaces:**
- Produces:
  - `orbit_nav::{Level, OrbitView, Transition, TRANSITION, MAX_PLANETS}`
  - `Level::{Galaxy, Workspace(PathBuf), Moon(PathBuf, Uuid)}`, `Level::workspace(&self) -> Option<&Path>`, `Level::agent(&self) -> Option<Uuid>`
  - `OrbitView { level, focus: usize, transition: Option<Transition> }`, `progress(now) -> f64`, `animating(now) -> bool`, `zoomed() -> bool`, `split(now) -> f64`
  - `App::planets(&self) -> Vec<&Workspace>`, `App::moons(&self, &Path) -> Vec<&AgentSession>`, `App::orbit_focused(&self) -> bool`, `App::previewed_agent(&self) -> Option<Uuid>`
  - `App.orbit: OrbitView`, `App.agent_preview: Option<(Uuid, Snapshot<String>)>`
  - `Action::{OrbitEnter(Level), OrbitOut, OrbitAttach(Uuid)}`, `Effect::Preview`

- [ ] **Step 1: Write the failing tests** in `tests/orbit_nav.rs`. They cover planet membership and order, keyboard zoom in and out, clicks, `FocusNext` skipping, reconcile fallbacks, the `n` key and the transition maths. The full test file is in the repository after this task; the key cases:

```rust
// planets: 8 workspaces, pinned [w7, w5], recent [w3, w0, w1, w2, w4, w6], selected w6
assert_eq!(names(&app), ["w7", "w5", "w0", "w1", "w3", "w6"]);
// keyboard: pane 0, Move(1) then Activate lands on planets[1] and selects it
assert_eq!(app.orbit.level, Level::Workspace(p.clone()));
assert_eq!(app.workspace(), Some(p.as_path()));
// Activate on a workspace opens the first moon's preview
assert!(matches!(effects[..], [Effect::Preview]));
// Activate on a moon attaches without leaving the Overview
assert!(matches!(effects[..], [Effect::AttachAgent(id)] if id == second));
assert_eq!(app.section, Section::Overview);
// Esc twice returns to the galaxy with the planet focused
// reconcile: agent vanishes -> Workspace(p); workspace vanishes -> Galaxy
// FocusNext from pane 0 while zoomed -> pane 2
// 'n' maps to NewAgent only when the orbit is focused and zoomed
// progress: 0.0 at start, 1.0 after TRANSITION; split 1.0 -> 0.4 for Workspace -> Moon
```

- [ ] **Step 2: Run them to verify they fail**

Run: `cargo test --test orbit_nav`
Expected: compile errors (`orbit_nav` module, `App::planets`, `Action::OrbitEnter` missing).

- [ ] **Step 3: Implement `src/orbit_nav.rs`**

```rust
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
        let share = |l: &Level| if matches!(l, Level::Moon(..)) { 0.4 } else { 1.0 };
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
        self.transition = (std::mem::discriminant(&from)
            != std::mem::discriminant(&self.level))
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
```

- [ ] **Step 4: Wire it into `App`**

`src/lib.rs`: add `pub mod orbit_nav;`.

`src/app.rs`:
- `Action`: add `OrbitEnter(crate::orbit_nav::Level)`, `OrbitOut`, `OrbitAttach(uuid::Uuid)`, each with a doc comment.
- `Effect`: add `/// Capture the previewed agent's screen now.` `Preview`.
- `App` fields: `pub orbit: crate::orbit_nav::OrbitView`, `pub agent_preview: Option<(uuid::Uuid, Snapshot<String>)>`, `pub(crate) pinned: Vec<PathBuf>`. In `new`, compute `pinned` as the canonicalised `config.pinned_projects` in order (as a `Vec`), and derive `pinned_paths` from it.
- `update`: after `task_action`, add `if let Some(e) = self.orbit_action(&action) { return e; }`.
- `FocusNext`: after the modulo, `if self.pane == 1 && self.orbit.zoomed() { self.pane = 2; }`.
- `SelectWorkspace`: call `self.reconcile_orbit();` before returning.
- `apply_provider`: handle `ProviderPayload::AgentPreview` (Task 2) and call `self.reconcile_orbit()` after `Agents` and `Projects` payloads.

`src/input.rs`: change the `n` arm to
`KeyCode::Char('n') if app.section == Section::Agents || (app.orbit_focused() && app.orbit.zoomed()) => Action::NewAgent,`

`tests/task_ui.rs`: rename `overview_focus_activates_matching_panel_and_the_orbit_pane_is_inert` to `..._and_an_empty_orbit_is_inert`. Change the comment to say the orbit pane has no section items and, with no planets, Activate does nothing.

- [ ] **Step 5: Run the tests**

Run: `cargo test --test orbit_nav --test task_ui --test selection`
Expected: PASS.

---

### Task 2: Agent preview data path

**Files:**
- Modify: `src/agents.rs` (`capture_screen`), `src/runtime/workers.rs` (`ProviderId::AgentPreview`, `ProviderRequest.agent`, `ProviderPayload::AgentPreview`), `src/app.rs` (`apply_provider`), `src/main.rs` (`submit`, the 1 s tick, `Effect::Preview`), every `ProviderRequest { .. }` literal (`src/main.rs`, `tests/providers.rs`, `tests/task_ui.rs`, `tests/selection.rs`)
- Test: `tests/orbit_nav.rs` (preview payloads), `tests/agents.rs` (capture_screen)

**Interfaces:**
- Consumes: `App.agent_preview`, `App::previewed_agent`, `Effect::Preview` (Task 1)
- Produces: `AgentManager::capture_screen(&self, Uuid) -> Result<String>`, `ProviderPayload::AgentPreview(Uuid, String)`, `ProviderRequest { agent: Option<Uuid>, .. }`

- [ ] **Step 1: Failing tests.** In `tests/orbit_nav.rs`, while at `Moon(p, id)`: an `AgentPreview(other, ..)` payload leaves the snapshot empty, `AgentPreview(id, "hi")` sets `value == Some("hi")`, and an `Err` whose request has `agent: Some(id)` sets `Availability::Failed` but keeps the value. In `tests/agents.rs`, next to the existing `reloaded.capture(session.id)` assertion: `assert!(reloaded.capture_screen(session.id).unwrap().contains("literal ; $(false) ' \""));`.
- [ ] **Step 2:** `cargo test --test orbit_nav --test agents` → compile failure.
- [ ] **Step 3: Implement.**

```rust
// src/agents.rs, next to `capture`
/// The agent's visible screen as plain text, for the orbit's preview.
pub fn capture_screen(&self, id: Uuid) -> Result<String> {
    self.checked(
        self.key()?.context("Missing server")?,
        vec![
            "capture-pane".into(),
            "-p".into(),
            "-t".into(),
            format!("=agent-{id}:0.0").into(),
        ],
    )
}
```

```rust
// src/runtime/workers.rs
ProviderId::AgentPreview => {
    let id = r.agent.ok_or_else(|| anyhow::anyhow!("No agent to preview"))?;
    ProviderPayload::AgentPreview(
        id,
        crate::agents::AgentManager::new(state.clone()).capture_screen(id)?,
    )
}
```

```rust
// src/app.rs apply_provider, Ok arm
ProviderPayload::AgentPreview(id, text) => {
    if let Some((current, snapshot)) = &mut self.agent_preview
        && *current == id
    {
        *snapshot = Snapshot::ready(text, self.generation);
    }
}
// Err arm
if event.request.id == ProviderId::AgentPreview
    && let Some((current, snapshot)) = &mut self.agent_preview
    && Some(*current) == event.request.agent
{
    snapshot.availability = crate::model::Availability::Failed(e.clone());
}
```

`src/main.rs`: `submit` sets `agent: app.previewed_agent()`. On the 1-second tick, `if app.previewed_agent().is_some() { submit(.., AgentPreview) }`. In the effect loop, `Effect::Preview => submit(&app, &pool, ProviderId::AgentPreview)`. All other `ProviderRequest` literals gain `agent: None`.

- [ ] **Step 4:** `cargo test --test orbit_nav --test agents --test providers --test selection --test task_ui` → PASS.

---

### Task 3: Orbit scene, camera and painter

**Files:**
- Modify: `src/ui/orbit.rs` (rewrite), `src/ui/theme.rs` (add `YELLOW = Rgb(0xee, 0xd4, 0x9f)`)
- Test: `tests/orbit.rs`

**Interfaces:**
- Consumes: `App::planets`, `App::moons`, `App.orbit`, `Level`, `Action::{OrbitEnter, OrbitOut, OrbitAttach}`
- Produces: `orbit::render(&mut Frame, Rect, &App) -> Vec<HitRegion>`, `orbit::scene(&App, u16, u16) -> Scene`, `orbit::place(usize, usize) -> Vec<(usize, f64)>`, `orbit::status_label(&AgentStatus) -> String`

- [ ] **Step 1: Failing tests** (`tests/orbit.rs`). Switch the helpers to `orbit::render(f, area, &app)`. Use a `planets()` fixture app with three workspaces, one of which has two agents. Then cover:
  - Determinism, and motion over 3 s.
  - Never painting outside the area.
  - Every planet has a hit region whose cell holds `●`/`◉`.
  - The focused label is fully visible at 40×10 for every whole second in 0..32.
  - The empty-galaxy hint.
  - The zoom transition: at `started + TRANSITION` the buffer equals the settled workspace buffer; at `started` it differs and still shows `✦`.
  - The system view shows the hint when there are no moons.
  - Moon hits carry `OrbitEnter(Moon)`, the previewed moon carries `OrbitAttach`, and the centre carries `OrbitOut`.
  - `place(5, 3)` gives rings `[0,1,2,0,1]` with second-on-ring offsets of π.
- [ ] **Step 2:** `cargo test --test orbit` → compile failure.
- [ ] **Step 3: Implement** `src/ui/orbit.rs` as the scene/camera/painter described in the spec's Rendering section (code lands in the repository; key pieces):
  - `RINGS` (today's three), `MOON_RINGS = [(0.5, 0.6, 14.0, 0.7), (0.95, 0.95, 24.0, 1.9)]`, `PLANET_COLORS = [TEAL, BLUE, MAUVE, PEACH, GREEN, YELLOW]`, `ZOOM = 4.0`.
  - `place(count, rings)`: ring `i % rings`, offset `(i / rings) / siblings × TAU` with `siblings = (count - ring).div_ceil(rings)`.
  - `galaxy`, `system`, `through(scene, anchor, focus, scale)` (`x' = anchor + (x - focus) × scale`) and `zooming(z)`:
    - The camera focus is `f = p − (p − c)(1 − z)/s`, so the target planet slides linearly to the centre while the scale is `s = 1 + 3z`.
    - The planet's own system is overlaid, scaled by `grow = clamp((z − ⅔)·3)` and anchored at the planet's screen position.
    - Galaxy labels and highlights are cleared while zooming.
  - The painter samples each ring at `((rx + ry) × 8).clamp(24, 2000)` points, flipping y for the canvas. It draws the fixed stars, then the bodies with SELECT + bold for the focused one. A label sits right of its body, or flips left when it would clip. The hint goes on the bottom row. Hit regions are only returned while not animating.
- [ ] **Step 4:** `cargo test --test orbit` → PASS.

---

### Task 4: Overview layout, preview panel and loop pacing

**Files:**
- Create: `src/ui/preview.rs`
- Modify: `src/ui/mod.rs`:
  - The zoomed Overview layout.
  - `render_pane` gains a `narrow` argument; pane 0 handles the title, split and preview.
  - The footer and help text.
- Modify: `src/main.rs`:
  - Poll every 40 ms while animating.
  - Stop setting `section = Agents` after an attach.
- Test: `tests/orbit.rs` (layout + preview)

**Interfaces:**
- Consumes: everything from Tasks 1–3.
- Produces: `preview::tail(&str, u16, u16) -> Vec<String>`, `preview::render(&mut Frame, Rect, &App) -> Vec<HitRegion>`

- [ ] **Step 1: Failing tests.**
  - `tail` trims trailing blank lines, keeps the last `height` lines, and truncates by display width (`"機器機器"` at width 5 → `"機器"`).
  - Zoomed at 120×38: there is no `Continue working`, `Machine pulse` and `Services & ports` are present, and the orbit title has the workspace name.
  - At a settled Moon with snapshot `"one\ntwo"`: the screen shows `two` and `Enter attaches`, and a hit carries `OrbitAttach`.
  - A saved agent shows `Saved conversation`; a failed capture that has a value shows `stale`.
  - At 80×24, pane 0 shows the zoomed system.
- [ ] **Step 2:** `cargo test --test orbit` → FAIL.
- [ ] **Step 3: Implement** the preview panel (left border only, title `name · tool · ● running[ · stale] · Enter attaches`; the body follows the spec's state table), the zoomed layout (top row = orbit, bottom row = System | Services), the moon split from `OrbitView::split` (horizontal, or vertical when narrow; preview drawn only once settled), the titles (`Orbit · ↑↓ planets · Enter land`, `Orbit · <name> · ↑↓ moons · Enter preview · n new · Esc out`, `Orbit · <name> · ↑↓ moons · Enter attach · Esc out`), the footer for the focused orbit (` ↑↓ choose · Enter zoom in · Esc zoom out · n new agent · Tab pane · q quit`), and the help line `Orbit (Tab to it): ↑↓ choose · Enter zoom in or attach · Esc zoom out`. In `main.rs`: `poll(Duration::from_millis(if app.orbit.animating(app.animation_elapsed) { 40 } else { 100 }))`, and remove `app.section = Section::Agents` from the `r.attach` branch.
- [ ] **Step 4:** `cargo test --test orbit` → PASS.

---

### Task 5: Docs, snapshots, full verification

**Files:**
- Modify: `README.md` (Overview section: orbit navigation keys), `tests/snapshots/*.snap`

- [ ] **Step 1:** README: add a short "Orbit" paragraph under the Overview section with the keys above.
- [ ] **Step 2:** `INSTA_UPDATE=always cargo test --test ui`, then review `git diff tests/snapshots`. The only changes should be the orbit pane (empty-galaxy rings and hint) and the help text.
- [ ] **Step 3:** `cargo test` (all), `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check` → all clean.
- [ ] **Step 4:** Drive the real app (run skill) at 120×38. Tab to the orbit, Enter on a planet, Enter on a moon, Esc twice, and capture screens.
