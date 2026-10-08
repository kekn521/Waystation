//! The Overview's orbit: workspaces circle the Waystation star as planets, and agent
//! sessions circle their workspace as moons. A pure scene is laid out first, in pane-local
//! cell coordinates (x right, y down), then painted; the same scene yields the click
//! targets, so clicks land on the moving bodies. The clock comes from the UI, not a
//! background task.

use super::theme::*;
use crate::{
    agents::AgentStatus,
    app::{Action, App, HitRegion},
    orbit_nav::Level,
};
use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    widgets::canvas::{Canvas, Points},
};
use std::{f64::consts::TAU, path::Path, time::Duration};
use unicode_width::UnicodeWidthStr;

/// Planet rings, inside out: horizontal share, vertical share, period (s), phase.
const RINGS: [(f64, f64, f64, f64); 3] = [
    (0.34, 0.48, 12.0, 0.3),
    (0.65, 0.75, 20.0, 2.5),
    (1.0, 1.0, 32.0, 4.3),
];
/// Moon rings around a planet, inside out.
const MOON_RINGS: [(f64, f64, f64, f64); 2] = [(0.5, 0.6, 14.0, 0.7), (0.95, 0.95, 24.0, 1.9)];
const PLANET_COLORS: [Color; 6] = [TEAL, BLUE, MAUVE, PEACH, GREEN, YELLOW];
/// How far the camera closes in on a planet before its own system takes over.
const ZOOM: f64 = 4.0;

pub struct Ring {
    pub cx: f64,
    pub cy: f64,
    pub rx: f64,
    pub ry: f64,
    pub color: Color,
}
pub struct Body {
    pub x: f64,
    pub y: f64,
    pub symbol: &'static str,
    pub color: Color,
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
/// "● running", "◌ saved" and so on, as the moons and the preview show it.
pub fn status_label(status: &AgentStatus) -> String {
    let (symbol, _) = moon_look(status);
    let word = match status {
        AgentStatus::Running => "running".into(),
        AgentStatus::Saved => "saved".into(),
        AgentStatus::Exited(code) => format!("exited {code}"),
        AgentStatus::Unavailable => "unavailable".into(),
    };
    format!("{symbol} {word}")
}
fn moon_look(status: &AgentStatus) -> (&'static str, Color) {
    match status {
        AgentStatus::Running => ("●", TEAL),
        AgentStatus::Saved => ("◌", BLUE),
        AgentStatus::Exited(_) => ("○", MUTED),
        AgentStatus::Unavailable => ("○", RED),
    }
}

/// Centre and radii of the space a `w` × `h` pane offers.
fn geometry(w: u16, h: u16) -> (f64, f64, f64, f64) {
    let cx = f64::from(w.saturating_sub(1)) / 2.0;
    let cy = f64::from(h.saturating_sub(1)) / 2.0;
    (cx, cy, (cx - 2.0).clamp(0.0, 30.0), (cy - 0.25).max(0.0))
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

fn galaxy(app: &App, w: u16, h: u16, t: Duration) -> Scene {
    let (cx, cy, radius, vertical) = geometry(w, h);
    let seconds = clock(t);
    let planets = app.planets();
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
            color: LINE,
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
        let color = PLANET_COLORS[i % PLANET_COLORS.len()];
        for trail in [0.3, 0.15] {
            let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, angle - trail);
            scene.bodies.push(Body {
                x,
                y,
                symbol: "·",
                color,
                focused: false,
                label: None,
                action: None,
            });
        }
        let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, angle);
        scene.bodies.push(Body {
            x,
            y,
            symbol: if app.workspace() == Some(planet.id.as_path()) {
                "◉"
            } else {
                "●"
            },
            color,
            focused: i == focus && app.pane == 0,
            label: (i == focus)
                .then(|| format!("{} · {}", planet.name, plural(app.moons(&planet.id).len()))),
            action: Some(Action::OrbitEnter(Level::Workspace(planet.id.clone()))),
        });
    }
    scene.bodies.push(Body {
        x: cx,
        y: cy,
        symbol: "✦",
        color: PEACH,
        focused: false,
        label: None,
        action: None,
    });
    scene
}

fn system(app: &App, path: &Path, w: u16, h: u16, t: Duration) -> Scene {
    let (cx, cy, radius, vertical) = geometry(w, h);
    let seconds = clock(t);
    let moons = app.moons(path);
    let index = app.planets().iter().position(|p| p.id == path).unwrap_or(0);
    let color = PLANET_COLORS[index % PLANET_COLORS.len()];
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
            color: LINE,
        });
    }
    // The planet: a braille disc of concentric rings around its clickable centre.
    for r in [2.0, 1.5, 1.0, 0.5] {
        scene.rings.push(Ring {
            cx,
            cy,
            rx: r,
            ry: r / 2.0,
            color,
        });
    }
    let previewed = app.orbit.level.agent();
    let focus = app.orbit.focus.min(moons.len().saturating_sub(1));
    for (j, (session, (ring, offset))) in moons
        .iter()
        .zip(place(moons.len(), rings.len()))
        .enumerate()
    {
        let (hs, vs, period, phase) = rings[ring];
        let angle = seconds.rem_euclid(period) / period * TAU + phase + offset;
        let (x, y) = orbit_point(cx, cy, radius * hs, vertical * vs, angle);
        let (symbol, color) = moon_look(&session.status);
        scene.bodies.push(Body {
            x,
            y,
            symbol,
            color,
            focused: j == focus && app.pane == 0,
            label: (j == focus)
                .then(|| format!("{} · {}", session.name, status_label(&session.status))),
            action: Some(if previewed == Some(session.id) {
                Action::OrbitAttach(session.id)
            } else {
                Action::OrbitEnter(Level::Moon(path.to_path_buf(), session.id))
            }),
        });
    }
    scene.bodies.push(Body {
        x: cx,
        y: cy,
        symbol: "◉",
        color,
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
    let (px, py) = sky
        .bodies
        .iter()
        .find(|b| matches!(&b.action, Some(Action::OrbitEnter(Level::Workspace(p))) if p == path))
        .map_or((cx, cy), |b| (b.x, b.y));
    let scale = 1.0 + (ZOOM - 1.0) * z;
    // Where the planet is drawn, and the camera focus that puts it there.
    let screen = (px + (cx - px) * z, py + (cy - py) * z);
    let focus = (px - (screen.0 - cx) / scale, py - (screen.1 - cy) / scale);
    let mut scene = through(sky, (cx, cy), focus, scale);
    let grow = ((z - 2.0 / 3.0) * 3.0).clamp(0.0, 1.0);
    if grow > 0.0 {
        let mut inner = through(system(app, path, w, h, t), screen, (cx, cy), grow);
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
        (_, Level::Workspace(p) | Level::Moon(p, _)) => system(app, p, w, h, t),
    }
}

/// Paint the orbit clipped to `area` and return a click target per body; none while a
/// zoom is in flight.
pub fn render(frame: &mut Frame, area: Rect, app: &App) -> Vec<HitRegion> {
    let area = area.intersection(frame.area());
    if area.is_empty() {
        return vec![];
    }
    let scene = scene(app, area.width, area.height);
    let (w, h) = (f64::from(area.width), f64::from(area.height));
    // Braille gives each terminal cell 2 × 4 dots, enough for smooth shallow ellipses.
    // The canvas counts y upwards, the scene downwards.
    frame.render_widget(
        Canvas::default()
            .background_color(MANTLE)
            .x_bounds([0.0, (w - 1.0).max(1.0)])
            .y_bounds([0.0, (h - 1.0).max(1.0)])
            .paint(|ctx| {
                for ring in &scene.rings {
                    let samples = ((ring.rx + ring.ry) * 8.0).clamp(24.0, 2000.0) as usize;
                    let points = (0..samples)
                        .map(|s| {
                            let (x, y) = orbit_point(
                                ring.cx,
                                ring.cy,
                                ring.rx,
                                ring.ry,
                                s as f64 / samples as f64 * TAU,
                            );
                            (x, (h - 1.0) - y)
                        })
                        .collect::<Vec<_>>();
                    ctx.draw(&Points::new(&points, ring.color));
                }
            }),
        area,
    );
    let buffer = frame.buffer_mut();
    let cell = |x: i32, y: i32| {
        (x >= 0 && y >= 0 && x < i32::from(area.width) && y < i32::from(area.height))
            .then(|| (area.x + x as u16, area.y + y as u16))
    };
    // Stars are fixed so only the moving bodies need terminal updates.
    for (x, y, symbol) in [
        (5, 1, "·"),
        (13, 4, "⋆"),
        (25, 0, "·"),
        (73, 1, "⋆"),
        (87, 4, "·"),
        (95, 2, "·"),
    ] {
        if let Some(at) = cell(i32::from(area.width) * x / 100, y) {
            buffer[at].set_symbol(symbol).set_fg(LINE);
        }
    }
    let settled = !app.orbit.animating(app.animation_elapsed);
    let mut hits = vec![];
    let mut labels = vec![];
    for body in &scene.bodies {
        let (x, y) = (body.x.round() as i32, body.y.round() as i32);
        let Some(at) = cell(x, y) else { continue };
        let painted = buffer[at].set_symbol(body.symbol).set_fg(body.color);
        if body.focused {
            painted.set_bg(SELECT).modifier.insert(Modifier::BOLD);
        }
        let mut target =
            Rect::new(at.0.saturating_sub(1).max(area.x), at.1, 3, 1).intersection(area);
        if let Some(label) = &body.label {
            // Right of the body when it fits, else on the roomier side, shortened if need be;
            // never over the body itself.
            let right = i32::from(area.width) - (x + 1);
            let text = fit(
                &super::safe(label).replace(['\n', '\t'], " "),
                right.max(x) as usize,
            );
            let width = text.width() as i32;
            let start = if width <= right { x + 1 } else { x - width };
            if width > 0 {
                let start = area.x + start as u16;
                let span = Rect::new(start, at.1, (width as u16).min(area.right() - start), 1);
                target = target.union(span);
                labels.push((span, text));
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
    for (span, text) in labels {
        buffer.set_stringn(
            span.x,
            span.y,
            &text,
            span.width as usize,
            Style::default().fg(TEXT).bg(MANTLE),
        );
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
