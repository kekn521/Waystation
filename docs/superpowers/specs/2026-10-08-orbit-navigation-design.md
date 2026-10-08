# Orbit navigation

Status: direction chosen 2026-10-08 (planets are recent + pinned workspaces, moons are their
agents, a moon shows a live read-only preview and Enter attaches, the orbit pane grows when
zoomed). Section approvals waived by the user.

## Goal

The Overview's decorative orbit becomes a way to move around Waystation:

- **Galaxy.** The star is Waystation; each planet circling it is a workspace.
- **Workspace.** Choosing a planet makes it the selected workspace and zooms in: the planet
  sits in the centre and its agent sessions circle it as moons.
- **Moon.** Choosing a moon shows a live, read-only tail of that agent's terminal next to
  the moon system. Enter (or clicking the moon or the preview again) attaches full-screen,
  exactly as Enter does in the Agents section. F12 / Ctrl-\ returns to the same zoom level.

Everything works by mouse and by keyboard. Esc zooms out one level.

Out of scope: hover effects (no mouse-move tracking), persisting the zoom level across
restarts, showing workspaces that are neither pinned nor recent.

## Bodies

### Planets

`App::planets() -> Vec<&Workspace>`, built only from `app.workspaces` (so rendering never
touches the filesystem):

- Membership, capped at **6**, filled in priority order: the selected workspace, pinned
  workspaces, then `state.recent_workspaces` from most recent.
- Order: pinned workspaces in config order, then the rest sorted by path. The order is
  deliberately not recency, so landing on a planet never reshuffles the system.
- Rings: `k = ceil(n / 2)`, at most 3, using the outermost `k` of today's three ring
  proportions. Planet `i` goes on ring `i % k`; bodies sharing a ring are spaced evenly
  (two planets sit half a turn apart).
- Colours cycle TEAL, BLUE, MAUVE, PEACH, GREEN, YELLOW (theme constants; add any missing).
- The selected workspace's planet is drawn `◉`, others `●`.
- The focused planet's label (`name · 2 agents`) is drawn beside it, flipped to the left when
  it would clip. Only the focused planet is labelled, so labels never overlap.
- No planets: the three rings stay, faint, with the star and a centred hint
  `Pin or open a workspace to see it here`.

### Moons

Agents whose `workspace` equals the planet's path, in `agent_items()` order. Moon symbol and
colour follow status: running `●` TEAL, saved `◌` BLUE, exited `○` MUTED, unavailable `○`
RED. In the galaxy view moons are not drawn (too small); the planet label carries the count.

In the workspace view the planet is a braille disc (radius about 2 cells) in its colour;
its name is in the pane title. Moons use one ring when there are at most 2 and two rings
otherwise. Moon `j` goes on ring `j % k`, spaced evenly, with no cap. The focused body (planet or
moon) gets a SELECT background and its label. A planet with no moons shows
`No moons yet · n starts an agent here`.

## Navigation state (`src/orbit_nav.rs`, re-exported through `app`)

```rust
pub enum Level { Galaxy, Workspace(PathBuf), Moon(PathBuf, Uuid) }
pub struct OrbitView {
    pub level: Level,
    pub focus: usize,              // index of the focused body at this level
    pub transition: Option<Transition>,
}
pub struct Transition { pub from: Level, pub started: Duration } // animation clock
```

Levels hold identities, not indices, so provider refreshes cannot point them at the wrong
body. After each `apply_provider`, `OrbitView::reconcile` falls back to the deepest level
still valid: if the moon's agent vanished go to `Workspace`, if the planet is no longer a
planet go to `Galaxy`. `focus` is clamped.

Keys while the Overview's orbit pane is focused (`pane == 0`, reached with Tab as today, or
by clicking any orbit body):

| Key | Galaxy | Workspace | Moon |
|-----|--------|-----------|------|
| j/k, ↑/↓ | focus next/prev planet | focus next/prev moon | switch to next/prev moon (preview follows) |
| Enter | zoom into focused planet (also `SelectWorkspace`) | open focused moon's preview | attach (`OpenAgent`) |
| Esc | (existing behaviour) | back to Galaxy, planet focused | back to Workspace, moon focused |
| n | — | `NewAgent` (form already defaults to the selected workspace) | `NewAgent` |

Mouse: clicking a planet focuses and zooms into it in one go; clicking a moon opens its
preview; clicking the selected moon or anywhere in the preview attaches; clicking the
central planet in the workspace view zooms out. Clicking any body also sets `pane = 0`.

New actions: `OrbitEnter(Level)` (zoom to a level, used by clicks and Enter), `OrbitOut`
and `OrbitAttach(Uuid)`. `OrbitAttach` emits `Effect::AttachAgent` without switching to
the Agents section, so returning from the agent lands back on the zoomed orbit. `main.rs`
stops forcing `section = Agents` when an attach finishes; every other attach path already
sets the section itself. `Move`, `Activate` and `Escape` are routed to the orbit when
`section == Overview && pane == 0` and nothing modal is open. Search, forms, confirmations
and detail views keep priority exactly as today.

## Layout

- **Galaxy:** unchanged four-pane Overview; the orbit lives in its quarter.
- **Workspace / Moon (Compact and Wide):** the top row becomes one full-width Orbit pane;
  System and Services stay underneath. The Workspaces pane is hidden while zoomed and
  `FocusNext` skips index 1.
- **Moon:** the Orbit pane splits horizontally 40 / 60: moon system on the left, preview
  panel on the right.
- **Single (narrow):** the orbit pane fills the content area as today; in Moon view it splits
  vertically 40 / 60 instead.
- Orbit pane title shows the path and hints, e.g. `Orbit · station · ↑↓ moons · Enter
  preview · Esc out`. The footer shows the same hints when the orbit is focused.

## Rendering (`src/ui/orbit.rs`)

Split into a pure scene and a painter:

```rust
pub struct Body { pub x: f64, pub y: f64, pub symbol: &'static str, pub color: Color,
                  pub label: Option<String>, pub action: Action }
pub fn scene(app: &App, area: Rect, t: Duration) -> Scene   // rings, bodies, hint
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion>
```

`render` paints the scene and returns a hit region per body: the body's cell ±1 column,
plus its label if drawn. Hit regions are rebuilt from the same scene every frame, so clicks
land on the moving bodies (the fastest moves about 2 cells per second). The scene is pure
and is what the geometry tests exercise.

### Zoom animation

Scenes are laid out in world coordinates and drawn through a camera
`(centre, scale)`. A transition lasts **600 ms** with ease-in-out:

- Galaxy → Workspace: the pane grows first. The camera then glides from the star to the
  chosen planet while scale rises 1 → 4, so rings and other planets sweep outward and clip
  away. In the last third the planet's disc and moon rings fade in (drawn in LINE, then in
  their colours).
- Workspace → Moon: the moon system's camera slides into the left column and the selected
  moon gains a ring; the preview panel appears once the slide ends.
- Zooming out plays the same path in reverse.

While a transition runs, the main loop polls input every 40 ms instead of 100 ms (about 25
fps). Idle orbiting keeps today's 8 fps quantisation. `OrbitView::animating(now)` exposes
this to `main.rs`.

## Agent preview

- `AgentManager::capture_screen(id)`: `capture-pane -p` on the agent's pane (the visible
  screen only, plain text, no `-e`/`-J`).
- New `ProviderId::AgentPreview`; `ProviderRequest` gains `agent: Option<Uuid>`. `main.rs`
  submits it on the 1-second tick while the level is `Moon`. Entering a moon returns a new
  `Effect::Preview`, which `main.rs` turns into an immediate submit. The payload is `AgentPreview { id, text }` and is ignored if `id` is no longer
  the previewed agent.
- `App::agent_preview: Option<(Uuid, Snapshot<String>)>`, cleared when leaving Moon.
- The panel title is `name · tool · ● running · Enter attaches`. The body is the last
  rows that fit, with trailing blank lines trimmed, each line truncated to the panel width
  by display width (no wrapping, so the tail stays bottom-aligned) and passed through
  `ui::safe`.
- Saved agent: `Saved conversation · Enter resumes it` (no capture). Unavailable:
  `This session is gone`. Capture error: keep the last text with ` · stale` in the title,
  or show the error muted if nothing was captured yet. Loading: `Connecting…`.

## Docs

The README's Overview section, the `?` help text and the footer gain the orbit keys.

## Testing

- `tests/orbit.rs`: update the existing tests to the new `render` signature, then add:
  planet selection order, the cap and the two-per-ring pairing. Every body cell is
  covered by a hit region whose action matches it. Labels never clip. The empty-galaxy hint
  shows. During a transition the scene at `t = started` equals the from-level scene, and at
  `t = started + 600 ms` it equals the to-level scene.
- `tests/selection.rs` (or a new `tests/orbit_nav.rs`): key-driven transitions
  (Enter/Esc/Move per level), clicking a planet selects the workspace, Esc priority with
  search/detail open, reconcile fallbacks when an agent or workspace disappears, and
  `FocusNext` skipping the hidden Workspaces pane.
- Preview: `App` ignores stale `AgentPreview` payloads; the panel renders the tail and the
  saved/unavailable/error states (TestBackend).
- `tests/snapshots`: Overview snapshots change because the decorative planets become real
  ones (default state means an empty galaxy). Review the new snapshots, then accept them.
- `tests/integration.rs`: `capture_screen` against the isolated tmux server, gated like the
  existing agent integration tests.
