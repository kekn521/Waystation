//! The Overview's orbit: workspaces circle the Waystation star as planets, and agent
//! sessions circle their workspace as moons. A pure scene is laid out first, in pane-local
//! cell coordinates (x right, y down), then painted as braille art; the same scene yields
//! the click targets, so clicks land on the moving bodies. The clock comes from the UI, not
//! a background task.
//!
//! The orbital plane is seen from slightly above: the far half of every orbit is the top
//! half, drawn fainter and hidden behind the star, and bodies nearer the viewer cover the
//! ones behind them. Planets are lit from the star, so each shows a phase.

use super::{
    sky::{Shade, Sky, mix},
    theme::*,
};
use crate::{
    agents::AgentStatus,
    app::{Action, App, HitRegion},
    orbit_nav::Level,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
};
use std::{f64::consts::TAU, path::Path, time::Duration};
use unicode_width::UnicodeWidthStr;

/// Planet orbits, inside out: horizontal share, vertical share, period (s), phase.
const RINGS: [(f64, f64, f64, f64); 3] = [
    (0.4, 0.5, 12.0, 0.3),
    (0.7, 0.76, 20.0, 2.5),
    (1.0, 1.0, 32.0, 4.3),
];
/// Moon orbits around a planet, inside out.
const MOON_RINGS: [(f64, f64, f64, f64); 2] = [(0.55, 0.66, 14.0, 0.7), (0.95, 0.95, 24.0, 1.9)];
const PLANET_COLORS: [Color; 6] = [TEAL, BLUE, MAUVE, PEACH, GREEN, YELLOW];
/// How far the camera closes in on a planet before its own system takes over.
const ZOOM: f64 = 4.0;
/// The star's light: a pale core deepening to a warm rim, with a glint at its centre.
const STAR_CORE: Color = Color::Rgb(0xfb, 0xf1, 0xd8);
const STAR_RIM: Color = PEACH;
/// Where light comes from inside a planet's own system: the star, off screen to the left.
const SYSTEM_LIGHT: (f64, f64) = (-0.8, -0.35);

/// What sets one planet apart from another, chosen from its workspace path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Rocky,
    Banded,
    Ringed,
}
#[derive(Clone, Copy, Debug)]
pub enum Look {
    Star,
    /// `home` marks the selected workspace with a faint halo.
    Planet {
        color: Color,
        kind: Kind,
        home: bool,
    },
    Moon {
        color: Color,
    },
    /// A single dot of a planet's wake.
    Dust {
        color: Color,
    },
}
pub struct Ring {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
}
pub struct Body {
    pub x: f64,
    pub y: f64,
    /// Half the body's width in columns; it is half that in rows.
    pub radius: f64,
    /// Nearness to the viewer, -1 (far side of its orbit) to 1 (near side).
    pub z: f64,
    /// Screen direction towards the light.
    pub light: (f64, f64),
    pub look: Look,
    /// Highlighted: the body the keys act on, while the orbit pane has focus.
    pub focused: bool,
    pub label: Option<String>,
    pub action: Option<Action>,
}
#[derive(Default)]
pub struct Scene {
    pub rings: Vec<Ring>,
    pub bodies: Vec<Body>,
    pub hint: Option<&'static str>,
}

/// Ring and angle offset for each of `count` bodies over `rings` rings: body `i` goes on
/// ring `i % rings`, and bodies sharing a ring are spaced evenly around it.
pub fn place(count: usize, rings: usize) -> Vec<(usize, f64)> {
    (0..count)
        .map(|i| {
            let ring = i % rings;
            let siblings = (count - ring).div_ceil(rings);
            (ring, (i / rings) as f64 / siblings as f64 * TAU)
        })
        .collect()
}
/// "● running", "◌ saved" and so on, as a moon's label shows it.
pub fn status_label(status: &AgentStatus) -> String {
    let symbol = match status {
        AgentStatus::Running => "●",
        AgentStatus::Saved => "◌",
        AgentStatus::Exited(_) | AgentStatus::Unavailable => "○",
    };
    let word = match status {
        AgentStatus::Running => "running".into(),
        AgentStatus::Saved => "saved".into(),
        AgentStatus::Exited(code) => format!("exited {code}"),
        AgentStatus::Unavailable => "unavailable".into(),
    };
    format!("{symbol} {word}")
}
fn moon_color(status: &AgentStatus) -> Color {
    match status {
        AgentStatus::Running => TEAL,
        AgentStatus::Saved => BLUE,
        AgentStatus::Exited(_) => MUTED,
        AgentStatus::Unavailable => RED,
    }
}
/// A stable look for each planet: kind from its path, colour from its path but never one
/// an earlier planet in `planets` already wears.
fn looks(planets: &[&Path]) -> Vec<(Color, Kind)> {
    let mut taken = vec![];
    planets
        .iter()
        .map(|p| {
            // FNV-1a, so the look survives restarts and other workspaces coming and going.
            let hash = p
                .as_os_str()
                .as_encoded_bytes()
                .iter()
                .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
                    (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
                });
            let kind = [Kind::Rocky, Kind::Banded, Kind::Ringed][(hash % 3) as usize];
            let start = (hash / 3) as usize;
            let color = (0..PLANET_COLORS.len())
                .map(|i| PLANET_COLORS[(start + i) % PLANET_COLORS.len()])
                .find(|c| !taken.contains(c))
                .unwrap_or(PLANET_COLORS[start % PLANET_COLORS.len()]);
            taken.push(color);
            (color, kind)
        })
        .collect()
}
fn look_of(app: &App, path: &Path) -> (Color, Kind) {
    let planets = app.planets();
    let paths = planets.iter().map(|w| w.id.as_path()).collect::<Vec<_>>();
    let all = looks(&paths);
    paths
        .iter()
        .position(|p| *p == path)
        .map_or_else(|| looks(&[path])[0], |i| all[i])
}

/// Centre and orbit radii of the space a `w` × `h` pane offers, using all of its width
/// while keeping most of the largest planet inside the pane.
fn geometry(w: u16, h: u16) -> (f64, f64, f64, f64) {
    let cx = f64::from(w.saturating_sub(1)) / 2.0;
    let cy = f64::from(h.saturating_sub(1)) / 2.0;
    let size = sizes(h);
    let largest = size.planet + size.per_agent * 5.0;
    let rx = (cx - 1.0 - largest * 0.6).max(0.0);
    let ry = (cy - 0.4 - largest / 2.0 * 0.4).max(0.0);
    (cx, cy, rx, ry)
}
/// Body sizes in columns for a pane `h` rows tall: the star, the smallest planet, how much
/// each agent adds to a planet, a planet seen up close, and a moon.
struct Sizes {
    star: f64,
    planet: f64,
    per_agent: f64,
    close: f64,
    moon: f64,
}
fn sizes(h: u16) -> Sizes {
    // In dots (four per row), then halved into columns (two dots each).
    let dots = f64::from(h) * 4.0;
    Sizes {
        star: (dots * 0.13).clamp(2.0, 14.0) / 2.0,
        planet: (dots * 0.075).clamp(2.0, 6.0) / 2.0,
        per_agent: (dots * 0.015).clamp(0.3, 1.2) / 2.0,
        close: (dots * 0.24).clamp(3.0, 24.0) / 2.0,
        moon: (dots * 0.05).clamp(2.0, 4.0) / 2.0,
    }
}
/// Seconds of orbit motion, quantized to eight frames per second.
fn clock(t: Duration) -> f64 {
    (t.as_millis() / 125) as f64 / 8.0
}
fn orbit_point(cx: f64, cy: f64, rx: f64, ry: f64, angle: f64) -> (f64, f64) {
    (cx + rx * angle.cos(), cy + ry * angle.sin())
}
/// ` label ` padded for the sky, cut with `…` to at most `room` columns.
fn fit(label: &str, room: usize) -> String {
    let full = format!(" {label} ");
    if full.width() <= room {
        return full;
    }
    let mut used = 0;
    let cut = label
        .chars()
        .take_while(|c| {
            used += unicode_width::UnicodeWidthChar::width(*c).unwrap_or(0);
            used + 3 <= room
        })
        .collect::<String>();
    if cut.is_empty() {
        String::new()
    } else {
        format!(" {cut}… ")
    }
}
fn plural(n: usize) -> String {
    if n == 1 {
        "1 agent".into()
    } else {
        format!("{n} agents")
    }
}
fn star(cx: f64, cy: f64, radius: f64) -> Body {
    Body {
        x: cx,
        y: cy,
        radius,
        z: 0.0,
        light: (0.0, 0.0),
        look: Look::Star,
        focused: false,
        label: None,
        action: None,
    }
}

fn galaxy(app: &App, w: u16, h: u16, t: Duration) -> Scene {
    let (cx, cy, radius, vertical) = geometry(w, h);
    let size = sizes(h);
    let seconds = clock(t);
    let planets = app.planets();
    let looks = looks(&planets.iter().map(|w| w.id.as_path()).collect::<Vec<_>>());
    // An empty galaxy keeps all three rings; otherwise the outermost ones that are used.
    let used = planets.len().div_ceil(2).clamp(1, RINGS.len());
    let rings = if planets.is_empty() {
        &RINGS[..]
    } else {
        &RINGS[RINGS.len() - used..]
    };
    let mut scene = Scene {
        hint: planets
            .is_empty()
            .then_some("Pin or open a workspace to see it here"),
        ..Scene::default()
    };
    for &(hs, vs, _, _) in rings {
        scene.rings.push(Ring {
            cx,
            cy,
            rx: radius * hs,
            ry: vertical * vs,
        });
    }
    let focus = app.orbit.focus.min(planets.len().saturating_sub(1));
    for (i, (planet, (ring, offset))) in planets
        .iter()
        .zip(place(planets.len(), rings.len()))
        .enumerate()
    {
        let (hs, vs, period, phase) = rings[ring];
        let angle = seconds.rem_euclid(period) / period * TAU + phase + offset;
        let (color, kind) = looks[i];
        // A short wake along the orbit, fading behind the planet.
        for k in 1..=5 {
            let behind = angle - f64::from(k) * 0.045;
            let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, behind);
            scene.bodies.push(Body {
                x,
                y,
                radius: 0.0,
                z: behind.sin(),
                light: (0.0, 0.0),
                look: Look::Dust {
                    color: mix(MANTLE, color, 0.75 - f64::from(k) * 0.12),
                },
                focused: false,
                label: None,
                action: None,
            });
        }
        let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, angle);
        let agents = app.moons(&planet.id).len();
        scene.bodies.push(Body {
            x,
            y,
            radius: size.planet + size.per_agent * agents.min(5) as f64,
            z: angle.sin(),
            // Towards the star, in square dot units: a row is twice a column's height.
            light: ((cx - x) * 2.0, (cy - y) * 4.0),
            look: Look::Planet {
                color,
                kind,
                home: app.workspace() == Some(planet.id.as_path()),
            },
            focused: i == focus && app.pane == 0,
            label: (i == focus).then(|| format!("{} · {}", planet.name, plural(agents))),
            action: Some(Action::OrbitEnter(Level::Workspace(planet.id.clone()))),
        });
    }
    scene.bodies.push(star(cx, cy, size.star));
    scene
}

fn system(app: &App, path: &Path, w: u16, h: u16, t: Duration) -> Scene {
    let (cx, cy, radius, vertical) = geometry(w, h);
    let size = sizes(h);
    let seconds = clock(t);
    let moons = app.moons(path);
    let (color, kind) = look_of(app, path);
    let used = if moons.len() <= 2 { 1 } else { 2 };
    let rings = &MOON_RINGS[MOON_RINGS.len() - used..];
    let mut scene = Scene {
        hint: moons
            .is_empty()
            .then_some("No moons yet · n starts an agent here"),
        ..Scene::default()
    };
    for &(hs, vs, _, _) in rings {
        scene.rings.push(Ring {
            cx,
            cy,
            rx: radius * hs,
            ry: vertical * vs,
        });
    }
    let focus = app.orbit.focus.min(moons.len().saturating_sub(1));
    for (j, (session, (ring, offset))) in moons
        .iter()
        .zip(place(moons.len(), rings.len()))
        .enumerate()
    {
        let (hs, vs, period, phase) = rings[ring];
        let angle = seconds.rem_euclid(period) / period * TAU + phase + offset;
        let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, angle);
        scene.bodies.push(Body {
            x,
            y,
            radius: size.moon,
            z: angle.sin(),
            light: SYSTEM_LIGHT,
            look: Look::Moon {
                color: moon_color(&session.status),
            },
            focused: j == focus && app.pane == 0,
            label: (j == focus)
                .then(|| format!("{} · {}", session.name, status_label(&session.status))),
            action: Some(Action::OrbitAttach(session.id)),
        });
    }
    scene.bodies.push(Body {
        x: cx,
        y: cy,
        radius: size.close,
        z: 0.0,
        light: SYSTEM_LIGHT,
        look: Look::Planet {
            color,
            kind,
            home: false,
        },
        focused: false,
        label: None,
        action: Some(Action::OrbitOut),
    });
    scene
}

/// Look at `scene` from `focus`, magnified `scale` times, with `focus` drawn at `anchor`.
fn through(mut scene: Scene, anchor: (f64, f64), focus: (f64, f64), scale: f64) -> Scene {
    let map = |x: f64, y: f64| {
        (
            anchor.0 + (x - focus.0) * scale,
            anchor.1 + (y - focus.1) * scale,
        )
    };
    for ring in &mut scene.rings {
        (ring.cx, ring.cy) = map(ring.cx, ring.cy);
        ring.rx *= scale;
        ring.ry *= scale;
    }
    for body in &mut scene.bodies {
        (body.x, body.y) = map(body.x, body.y);
        body.radius *= scale;
    }
    scene
}

/// Part way between the galaxy (`z = 0`) and `path`'s own system (`z = 1`): the camera
/// closes in while the planet slides to the centre, and in the last third the planet's
/// system grows out of it.
fn zooming(app: &App, path: &Path, z: f64, w: u16, h: u16, t: Duration) -> Scene {
    let (cx, cy, _, _) = geometry(w, h);
    let mut sky = galaxy(app, w, h, t);
    sky.hint = None;
    for body in &mut sky.bodies {
        body.label = None;
        body.focused = false;
    }
    let target =
        |b: &Body| matches!(&b.action, Some(Action::OrbitEnter(Level::Workspace(p))) if p == path);
    let (px, py, radius, light) = sky
        .bodies
        .iter()
        .find(|b| target(b))
        .map_or((cx, cy, 0.0, SYSTEM_LIGHT), |b| {
            (b.x, b.y, b.radius, b.light)
        });
    let scale = 1.0 + (ZOOM - 1.0) * z;
    // Where the planet is drawn, and the camera focus that puts it there.
    let screen = (px + (cx - px) * z, py + (cy - py) * z);
    let focus = (px - (screen.0 - cx) / scale, py - (screen.1 - cy) / scale);
    let mut scene = through(sky, (cx, cy), focus, scale);
    // The planet itself grows into its close-up as the light swings round to the system's.
    let close = sizes(h).close;
    for body in scene.bodies.iter_mut().filter(|b| target(b)) {
        body.radius = radius + (close - radius) * z;
        body.light = (
            light.0 + (SYSTEM_LIGHT.0 - light.0) * z,
            light.1 + (SYSTEM_LIGHT.1 - light.1) * z,
        );
        if let Look::Planet { home, .. } = &mut body.look {
            *home = false;
        }
    }
    let grow = ((z - 2.0 / 3.0) * 3.0).clamp(0.0, 1.0);
    if grow > 0.0 {
        let mut inner = through(system(app, path, w, h, t), screen, (cx, cy), grow);
        // Only the moons and their orbits grow in; the planet is already there.
        inner
            .bodies
            .retain(|b| !matches!(b.action, Some(Action::OrbitOut)));
        for body in &mut inner.bodies {
            body.label = None;
            body.focused = false;
        }
        scene.rings.extend(inner.rings);
        scene.bodies.extend(inner.bodies);
    }
    scene
}

/// The orbit as it should look in a `w` × `h` pane at the app's animation time.
pub fn scene(app: &App, w: u16, h: u16) -> Scene {
    let t = app.animation_elapsed;
    let progress = app.orbit.progress(t);
    let zoom = match (&app.orbit.transition, &app.orbit.level) {
        (Some(tr), Level::Workspace(p)) if tr.from == Level::Galaxy => Some((p, progress)),
        (Some(tr), Level::Galaxy) => match &tr.from {
            Level::Workspace(p) => Some((p, 1.0 - progress)),
            _ => None,
        },
        _ => None,
    };
    match (zoom, &app.orbit.level) {
        (Some((path, z)), _) if app.orbit.animating(t) => zooming(app, path, z, w, h, t),
        (_, Level::Galaxy) => galaxy(app, w, h, t),
        (_, Level::Workspace(p)) => system(app, p, w, h, t),
    }
}

/// Dot coordinates of a cell-space point: a cell is 2 dots wide and 4 tall.
fn dots(x: f64, y: f64) -> (f64, f64) {
    (x * 2.0 + 1.0, y * 4.0 + 2.0)
}
/// Paint an orbit, fainter and dotted over its far half, which the star hides.
fn paint_ring(sky: &mut Sky, ring: &Ring) {
    let (cx, cy) = dots(ring.cx, ring.cy);
    let (rx, ry) = (ring.rx * 2.0, ring.ry * 4.0);
    let samples = ((rx + ry) * 4.0).clamp(24.0, 6000.0) as usize;
    for s in 0..samples {
        let angle = s as f64 / samples as f64 * TAU;
        let near = angle.sin() >= 0.0;
        if !near && s % 2 == 1 {
            continue;
        }
        let color = if near { LINE } else { mix(MANTLE, LINE, 0.6) };
        let (x, y) = orbit_point(cx, cy, rx, ry, angle);
        // Just behind whatever travels along it.
        sky.dot(
            x.floor() as i32,
            y.floor() as i32,
            angle.sin() - 0.02,
            Some(color),
        );
    }
}
/// A ring of dots around (`cx`, `cy`) in dot space; `front` decides which part to draw.
fn paint_band(
    sky: &mut Sky,
    (cx, cy): (f64, f64),
    (rx, ry): (f64, f64),
    z: f64,
    color: Color,
    front: impl Fn(f64) -> bool,
) {
    let samples = ((rx + ry) * 5.0).clamp(16.0, 2000.0) as usize;
    for s in 0..samples {
        let angle = s as f64 / samples as f64 * TAU;
        if front(angle) {
            let (x, y) = orbit_point(cx, cy, rx, ry, angle);
            sky.dot(x.floor() as i32, y.floor() as i32, z, Some(color));
        }
    }
}
fn paint_body(sky: &mut Sky, body: &Body) {
    let (cx, cy) = dots(body.x, body.y);
    let r = body.radius * 2.0;
    match body.look {
        Look::Star => {
            // A sparse corona, densest at the rim and gone by 1.7 radii.
            let reach = r * 1.7;
            for y in (cy - reach) as i32..=(cy + reach) as i32 {
                for x in (cx - reach) as i32..=(cx + reach) as i32 {
                    let d = ((f64::from(x) + 0.5 - cx).hypot(f64::from(y) + 0.5 - cy) - r)
                        / (reach - r);
                    if (0.0..1.0).contains(&d) && (1.0 - d).powi(2) * 0.55 > sparkle(x, y) {
                        sky.dot(x, y, -3.0, Some(mix(MANTLE, STAR_RIM, 0.75 - 0.4 * d)));
                    }
                }
            }
            let glow = Shade::Glow {
                core: STAR_CORE,
                rim: STAR_RIM,
            };
            sky.sphere(cx, cy, r, body.z, &glow, (0.0, 0.0));
        }
        Look::Planet { color, kind, home } => {
            let ring = (r * 1.75, r * 0.38);
            let ring_color = mix(MANTLE, mix(color, TEXT, 0.3), 0.8);
            if kind == Kind::Ringed {
                // The far half of the ring passes behind the planet.
                paint_band(sky, (cx, cy), ring, body.z - 1e-3, ring_color, |a| {
                    a.sin() < 0.0
                });
            }
            if home {
                let halo = (r + 2.5, r + 2.5);
                let dim = mix(MANTLE, color, 0.55);
                paint_band(sky, (cx, cy), halo, body.z - 2e-3, dim, |a| {
                    (a * 8.0).sin() > 0.0
                });
            }
            let shade = Shade::Lit {
                color,
                bands: kind == Kind::Banded,
            };
            sky.sphere(cx, cy, r, body.z, &shade, body.light);
            if kind == Kind::Ringed {
                paint_band(sky, (cx, cy), ring, body.z + 0.01, ring_color, |a| {
                    a.sin() >= 0.0
                });
            }
        }
        Look::Moon { color } => sky.sphere(cx, cy, r, body.z, &Shade::plain(color), body.light),
        Look::Dust { color } => sky.dot(
            cx.floor() as i32,
            cy.floor() as i32,
            body.z - 0.01,
            Some(color),
        ),
    }
}
/// A fixed pseudo-random threshold per dot, for the corona's scattered sparks.
fn sparkle(x: i32, y: i32) -> f64 {
    let h = (x as u32).wrapping_mul(0x9e37_79b1) ^ (y as u32).wrapping_mul(0x85eb_ca77);
    f64::from(h.wrapping_mul(0xc2b2_ae3d) >> 8) / f64::from(1u32 << 24)
}

/// Paint the orbit clipped to `area` and return a click target per body; none while a
/// zoom is in flight.
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let area = area.intersection(frame.area());
    if area.is_empty() {
        return vec![];
    }
    let scene = scene(app, area.width, area.height);
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            buffer[(x, y)].reset();
            buffer[(x, y)].set_bg(MANTLE);
        }
    }
    let cell = |x: i32, y: i32| {
        (x >= 0 && y >= 0 && x < i32::from(area.width) && y < i32::from(area.height))
            .then(|| (area.x + x as u16, area.y + y as u16))
    };
    // Background stars, scattered by a fixed pattern so only moving bodies repaint.
    let count = usize::from(area.width) * usize::from(area.height) / 90 + 4;
    for i in 0..count as i32 {
        let x = (sparkle(i, 7) * f64::from(area.width)) as i32;
        let y = (sparkle(i, 13) * f64::from(area.height)) as i32;
        if let Some(at) = cell(x, y) {
            let bright = i % 5 == 0;
            buffer[at]
                .set_symbol(if bright { "⋆" } else { "·" })
                .set_fg(if bright { MUTED } else { LINE });
        }
    }
    let mut sky = Sky::new(area.width, area.height);
    for ring in &scene.rings {
        paint_ring(&mut sky, ring);
    }
    for body in &scene.bodies {
        paint_body(&mut sky, body);
    }
    for y in 0..area.height {
        for x in 0..area.width {
            if let Some((symbol, color)) = sky.cell(x, y) {
                buffer[(area.x + x, area.y + y)]
                    .set_char(symbol)
                    .set_fg(color);
            }
        }
    }
    // A glint at the star's heart.
    for body in &scene.bodies {
        if matches!(body.look, Look::Star)
            && body.radius >= 1.0
            && let Some(at) = cell(body.x.round() as i32, body.y.round() as i32)
        {
            // Its own cell is lit solid, so the glint never reads as a hole in the star.
            buffer[at]
                .set_symbol("✦")
                .set_fg(STAR_RIM)
                .set_bg(STAR_CORE)
                .modifier
                .insert(Modifier::BOLD);
        }
    }
    let settled = !app.orbit.animating(app.animation_elapsed);
    let mut hits = vec![];
    let mut labels = vec![];
    for body in &scene.bodies {
        let (x, y) = (body.x.round() as i32, body.y.round() as i32);
        let Some(at) = cell(x, y) else { continue };
        // The disc's extent, at least a 3 × 1 target around its centre.
        let (half_w, half_h) = (
            (body.radius.round() as i32).max(1),
            ((body.radius / 2.0).round() as i32).max(0),
        );
        let left = (x - half_w).max(0);
        let top = (y - half_h).max(0);
        let mut target = Rect::new(
            area.x + left as u16,
            area.y + top as u16,
            (x + half_w - left + 1) as u16,
            (y + half_h - top + 1) as u16,
        )
        .intersection(area);
        if let Some(label) = &body.label {
            // Beside the disc on the right when it fits, else on the roomier side, shortened
            // if need be; never over the body itself.
            let edge = half_w.max(1);
            let right = i32::from(area.width) - (x + edge + 1);
            let left_room = x - edge;
            let text = fit(
                &super::safe(label).replace(['\n', '\t'], " "),
                right.max(left_room).max(0) as usize,
            );
            let width = text.width() as i32;
            let start = if width <= right {
                x + edge + 1
            } else {
                x - edge - width
            };
            if width > 0 && start >= 0 {
                let start = area.x + start as u16;
                let span = Rect::new(start, at.1, (width as u16).min(area.right() - start), 1);
                target = target.union(span);
                labels.push((span, text, body.focused));
            }
        }
        if settled && let Some(action) = &body.action {
            hits.push(HitRegion {
                area: target,
                action: action.clone(),
            });
        }
    }
    // Labels last, so no body can cut through one.
    for (span, text, focused) in labels {
        let style = if focused {
            Style::default()
                .fg(TEXT)
                .bg(SELECT)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(MUTED).bg(MANTLE)
        };
        buffer.set_stringn(span.x, span.y, &text, span.width as usize, style);
    }
    if let Some(hint) = scene.hint {
        let width = (hint.width() as u16).min(area.width);
        buffer.set_stringn(
            area.x + (area.width - width) / 2,
            area.bottom() - 1,
            hint,
            width as usize,
            Style::default().fg(MUTED),
        );
    }
    hits
}
