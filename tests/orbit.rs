use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Stylize},
    widgets::Block,
};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;
use waystation::{
    agents::{AgentSession, AgentStatus},
    app::{Action, App, HitRegion},
    config::{Config, ToolCommand},
    model::{AppState, Section},
    orbit_nav::{Level, TRANSITION},
    providers::projects::Workspace,
    ui::{draw, orbit},
};
fn app() -> App {
    App::new(Config::default(), AppState::default())
}
fn path(name: &str) -> PathBuf {
    format!("/orbit-fixture/{name}").into()
}
/// Planets w0..w2 (w1 selected), with agents "a" and "b" circling w1.
fn planets() -> App {
    let mut a = App::new(
        Config::default(),
        AppState {
            selected_workspace: Some(path("w1")),
            recent_workspaces: vec![path("w1"), path("w0"), path("w2")],
            ..AppState::default()
        },
    );
    a.set_workspaces(
        ["w0", "w1", "w2"]
            .map(|n| Workspace {
                id: path(n),
                name: n.into(),
            })
            .to_vec(),
    );
    a.agents = ["a", "b"]
        .map(|n| AgentSession {
            id: Uuid::new_v4(),
            name: n.into(),
            tool: "claude".into(),
            workspace: path("w1"),
            command: ToolCommand {
                program: "claude".into(),
                args: vec![],
            },
            executable: "/usr/bin/claude".into(),
            created: std::time::SystemTime::UNIX_EPOCH,
            conversation: None,
            status: AgentStatus::Running,
        })
        .to_vec();
    a
}
fn moon(app: &App, name: &str) -> Uuid {
    app.agents.iter().find(|s| s.name == name).unwrap().id
}
/// Renders the orbit into `area` over a uniform DarkGray backdrop so that any
/// cell touched by the renderer is detectable against the baseline.
fn orbit_buffer(width: u16, height: u16, area: Rect, elapsed: Duration) -> Buffer {
    let mut a = planets();
    a.animation_elapsed = elapsed;
    orbit_hits(&a, width, height, area).0
}
fn orbit_hits(app: &App, width: u16, height: u16, area: Rect) -> (Buffer, Vec<HitRegion>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut hits = vec![];
    terminal
        .draw(|f| {
            let full = f.area();
            f.render_widget(Block::default().bg(Color::DarkGray), full);
            hits = orbit::render(f, area, app);
        })
        .unwrap();
    (terminal.backend().buffer().clone(), hits)
}
fn text_of(buffer: &Buffer) -> Vec<String> {
    buffer
        .content
        .chunks(buffer.area.width as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect())
        .collect()
}
fn buffer_of(app: &App, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            draw(f, app);
        })
        .unwrap();
    terminal.backend().buffer().clone()
}
fn screen(app: &App, width: u16, height: u16) -> String {
    buffer_of(app, width, height)
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn rows_of(app: &App, width: u16, height: u16) -> Vec<String> {
    buffer_of(app, width, height)
        .content
        .chunks(width as usize)
        .map(|row| row.iter().map(|c| c.symbol()).collect())
        .collect()
}
/// Top-left cell of the first row containing `label`, in (column, row).
fn label_pos(rows: &[String], label: &str) -> (usize, usize) {
    for (y, row) in rows.iter().enumerate() {
        if let Some(i) = row.find(label) {
            return (row[..i].chars().count(), y);
        }
    }
    panic!("missing {label:?}");
}
#[test]
fn direct_orbit_render_is_deterministic() {
    let area = Rect::new(0, 0, 90, 6);
    assert_eq!(
        orbit_buffer(90, 6, area, Duration::from_secs(2)),
        orbit_buffer(90, 6, area, Duration::from_secs(2)),
        "the same elapsed time must render the same scene"
    );
    assert_ne!(
        orbit_buffer(90, 6, area, Duration::ZERO),
        orbit_buffer(90, 6, area, Duration::from_secs(3)),
        "three seconds of clock must move the planets"
    );
}
#[test]
fn orbit_render_never_touches_cells_outside_its_area() {
    const WIDTH: u16 = 100;
    const HEIGHT: u16 = 10;
    let elapsed = Duration::from_secs(5);
    // A zero-sized Rect is a no-op, so this buffer is the untouched backdrop.
    let baseline = orbit_buffer(WIDTH, HEIGHT, Rect::ZERO, elapsed);
    let areas = [
        (Rect::new(0, 0, WIDTH, HEIGHT), true),
        (Rect::new(3, 2, 60, 6), true),
        (Rect::new(90, 6, 10, 4), true),
        (Rect::new(94, 8, 6, 2), true),
        (Rect::new(0, 9, WIDTH, 1), true),
        (Rect::new(99, 0, 1, HEIGHT), true),
        (Rect::new(1, 1, 2, 6), true),
        (Rect::new(0, 4, 1, 1), true),
        (Rect::new(0, 0, 0, 6), false),
        (Rect::new(50, 5, 20, 0), false),
    ];
    for (area, renders) in areas {
        let got = orbit_buffer(WIDTH, HEIGHT, area, elapsed);
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                if !area.contains(Position::new(x, y)) {
                    assert_eq!(
                        got[(x, y)],
                        baseline[(x, y)],
                        "cell ({x},{y}) changed outside {area:?}"
                    );
                }
            }
        }
        if renders {
            assert!(
                (0..HEIGHT)
                    .flat_map(|y| (0..WIDTH).map(move |x| (x, y)))
                    .any(|(x, y)| area.contains(Position::new(x, y))
                        && got[(x, y)] != baseline[(x, y)]),
                "orbit painted nothing inside {area:?}"
            );
        } else {
            assert_eq!(got, baseline, "zero-sized {area:?} must render nothing");
        }
    }
}
#[test]
fn overview_shows_the_orbit_above_system_and_services() {
    for (w, h) in [(120, 38), (100, 32)] {
        let a = app();
        let text = screen(&a, w, h);
        // Pane order: 0 Orbit across the top, 1 System lower-left, 2 Services lower-right.
        for want in ["✦", "Orbit", "Machine pulse", "Services & ports"] {
            assert!(text.contains(want), "missing {want:?} at {w}x{h}");
        }
        for gone in ["Tasks", "Continue working"] {
            assert!(!text.contains(gone), "{gone:?} must not show at {w}x{h}");
        }
        let rows = rows_of(&a, w, h);
        let orbit = label_pos(&rows, "Orbit");
        let system = label_pos(&rows, "Machine pulse");
        let services = label_pos(&rows, "Services & ports");
        assert!(
            system.0 < services.0,
            "system {system:?} must sit left of services {services:?} at {w}x{h}"
        );
        assert!(
            orbit.1 < system.1 && orbit.1 < services.1,
            "orbit {orbit:?} must sit above the others at {w}x{h}"
        );
    }
}
#[test]
fn narrow_overview_starts_on_the_orbit() {
    let a = app();
    for (w, h) in [(80, 24), (60, 18)] {
        let text = screen(&a, w, h);
        assert!(
            text.contains("✦"),
            "the default pane is the orbit at {w}x{h}"
        );
        assert!(
            !text.contains("Machine pulse"),
            "one pane at a time at {w}x{h}"
        );
    }
    let mut focused = app();
    focused.pane = 1;
    let narrow = screen(&focused, 60, 18);
    assert!(!narrow.contains("✦"), "orbit leaked into pane 1 at 60x18");
    assert!(
        narrow.contains("Machine pulse"),
        "focused pane missing at 60x18"
    );
    let tiny = screen(&a, 59, 17);
    assert!(tiny.contains("Resize"));
    assert!(!tiny.contains("✦"), "orbit leaked below minimum size");
}
#[test]
fn no_orbit_outside_the_wide_overview() {
    for section in Section::ALL {
        let mut a = app();
        a.section = section;
        if section == Section::Overview {
            a.searching = true;
            a.query = "herdr".into();
        }
        let text = screen(&a, 120, 38);
        assert!(!text.contains("✦"), "orbit leaked into {}", section.name());
    }
    let mut searching = app();
    searching.searching = true;
    searching.query = "herdr".into();
    assert!(screen(&searching, 120, 38).contains("Jump to anything"));
}
#[test]
fn only_the_overview_moves_with_the_clock() {
    let mut a = planets();
    let early = buffer_of(&a, 120, 38);
    // Two draws at the same elapsed time must not advance the scene.
    let mut terminal = Terminal::new(TestBackend::new(120, 38)).unwrap();
    terminal
        .draw(|f| {
            draw(f, &a);
        })
        .unwrap();
    let first = terminal.backend().buffer().clone();
    terminal
        .draw(|f| {
            draw(f, &a);
        })
        .unwrap();
    assert_eq!(
        first,
        terminal.backend().buffer().clone(),
        "redraw at the same time advanced the animation"
    );
    assert_eq!(first, early, "repeated draws must be reproducible");
    a.animation_elapsed = Duration::from_secs(3);
    let late = buffer_of(&a, 120, 38);
    assert_ne!(early, late, "overview must follow the animation clock");
    let mut workspaces = planets();
    workspaces.section = Section::Workspaces;
    let before = buffer_of(&workspaces, 120, 38);
    workspaces.animation_elapsed = Duration::from_secs(3);
    assert_eq!(
        before,
        buffer_of(&workspaces, 120, 38),
        "Workspaces must ignore the animation clock"
    );
}

/// Whether `area` shows part of a disc: a dense braille cell in a colour no orbit line uses.
fn disc_in(buffer: &Buffer, area: Rect) -> bool {
    use waystation::ui::{
        sky::mix,
        theme::{LINE, MANTLE},
    };
    area.positions().any(|p| {
        let cell = &buffer[(p.x, p.y)];
        let dots = cell
            .symbol()
            .chars()
            .next()
            .filter(|c| ('\u{2801}'..='\u{28ff}').contains(c))
            .map_or(0, |c| (c as u32 - 0x2800).count_ones());
        dots >= 4 && cell.fg != LINE && cell.fg != mix(MANTLE, LINE, 0.6)
    })
}
#[test]
fn every_planet_is_a_clickable_body() {
    let a = planets();
    let area = Rect::new(0, 0, 60, 14);
    let (buffer, hits) = orbit_hits(&a, 60, 14, area);
    for name in ["w0", "w1", "w2"] {
        let hit = hits
            .iter()
            .find(|h| matches!(&h.action, Action::OrbitEnter(Level::Workspace(p)) if *p == path(name)))
            .unwrap_or_else(|| panic!("no hit for {name}"));
        assert!(
            disc_in(&buffer, hit.area),
            "{name}'s hit {:?} misses its planet:\n{}",
            hit.area,
            text_of(&buffer).join("\n")
        );
    }
}
#[test]
fn the_focused_label_is_never_clipped() {
    let mut a = planets();
    a.orbit.focus = 2;
    for second in 0..32 {
        a.animation_elapsed = Duration::from_secs(second);
        let (buffer, _) = orbit_hits(&a, 40, 10, Rect::new(0, 0, 40, 10));
        assert!(
            text_of(&buffer)
                .iter()
                .any(|row| row.contains("w2 · 0 agents")),
            "label clipped at {second}s:\n{}",
            text_of(&buffer).join("\n")
        );
    }
}
#[test]
fn an_empty_galaxy_explains_itself() {
    let (buffer, hits) = orbit_hits(&app(), 60, 12, Rect::new(0, 0, 60, 12));
    assert!(
        text_of(&buffer)
            .join("")
            .contains("Pin or open a workspace")
    );
    assert!(text_of(&buffer).join("").contains('✦'));
    assert!(hits.is_empty());
}
#[test]
fn zooming_starts_from_the_galaxy_and_ends_on_the_system() {
    let area = Rect::new(0, 0, 70, 14);
    let mut a = planets();
    a.animation_elapsed = Duration::from_secs(10);
    a.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    let start = orbit_hits(&a, 70, 14, area);
    assert!(
        text_of(&start.0).join("").contains('✦'),
        "the zoom starts at the star"
    );
    assert!(start.1.is_empty(), "no click targets while bodies fly");

    a.animation_elapsed += TRANSITION;
    let end = orbit_hits(&a, 70, 14, area).0;
    a.orbit.transition = None;
    assert_eq!(end, orbit_hits(&a, 70, 14, area).0);
    assert_ne!(start.0, end);
    assert!(
        !text_of(&end).join("").contains('✦'),
        "the star is out of sight"
    );
}
#[test]
fn a_system_offers_its_moons_and_a_way_out() {
    let area = Rect::new(0, 0, 70, 14);
    let mut a = planets();
    let (ma, mb) = (moon(&a, "a"), moon(&a, "b"));
    a.update(Action::OrbitEnter(Level::Workspace(path("w1"))));
    a.orbit.transition = None;
    let (_, hits) = orbit_hits(&a, 70, 14, area);
    let has = |hits: &[HitRegion], f: &dyn Fn(&Action) -> bool| hits.iter().any(|h| f(&h.action));
    for m in [ma, mb] {
        assert!(
            has(&hits, &|x| matches!(x, Action::OrbitAttach(id) if *id == m)),
            "clicking a moon opens its agent"
        );
    }
    assert!(has(&hits, &|x| matches!(x, Action::OrbitOut)));

    let mut empty = planets();
    empty.update(Action::OrbitEnter(Level::Workspace(path("w0"))));
    empty.orbit.transition = None;
    let (buffer, _) = orbit_hits(&empty, 70, 14, area);
    assert!(text_of(&buffer).join("").contains("No moons yet"));
}
#[test]
fn bodies_share_rings_evenly() {
    let placed = orbit::place(5, 3);
    assert_eq!(
        placed.iter().map(|p| p.0).collect::<Vec<_>>(),
        [0, 1, 2, 0, 1]
    );
    assert_eq!(placed[0].1, 0.0);
    assert!((placed[3].1 - std::f64::consts::PI).abs() < 1e-9);
    assert_eq!(placed[2].1, 0.0, "a body alone on its ring");
}
fn zoomed(level: Level) -> App {
    let mut a = planets();
    a.pane = 0;
    a.update(Action::OrbitEnter(level));
    a.orbit.transition = None;
    a
}
#[test]
fn a_zoomed_orbit_takes_the_top_row() {
    let a = zoomed(Level::Workspace(path("w1")));
    let text = screen(&a, 120, 38);
    for want in [
        "Machine pulse",
        "Services & ports",
        "Orbit · w1",
        "a · ● running",
    ] {
        assert!(text.contains(want), "missing {want:?}");
    }
    let rows = rows_of(&a, 120, 38);
    assert!(label_pos(&rows, "Orbit · w1").1 < label_pos(&rows, "Machine pulse").1);
}
#[test]
fn a_highlighted_moon_shows_no_preview() {
    let mut a = zoomed(Level::Workspace(path("w1")));
    a.update(Action::Move(1));
    let text = screen(&a, 120, 38);
    assert!(!text.contains("Enter attaches") && !text.contains("Connecting"));
    assert!(
        footer(&a, 120, 38).contains("Enter open"),
        "Enter opens the agent full screen"
    );
}
#[test]
fn a_narrow_orbit_zooms_in_place() {
    let a = zoomed(Level::Workspace(path("w1")));
    let text = screen(&a, 80, 24);
    assert!(text.contains("a · ● running"), "{text}");
    assert!(
        text.contains("Esc"),
        "the footer or title says how to get out"
    );
}
#[test]
fn a_long_label_never_hides_its_planet() {
    let mut a = planets();
    let long = "a-workspace-name-far-too-long-for-the-pane";
    a.workspaces.iter_mut().for_each(|w| w.name = long.into());
    a.orbit.focus = 1;
    for second in 0..32 {
        a.animation_elapsed = Duration::from_secs(second);
        let (buffer, hits) = orbit_hits(&a, 40, 10, Rect::new(0, 0, 40, 10));
        let text = text_of(&buffer).join("\n");
        let w1 = hits
            .iter()
            .find(|h| matches!(&h.action, Action::OrbitEnter(Level::Workspace(p)) if *p == path("w1")))
            .unwrap();
        assert!(
            disc_in(&buffer, w1.area),
            "the selected planet vanished at {second}s:\n{text}"
        );
        assert!(
            text.contains('…'),
            "the label is shortened, not cut, at {second}s:\n{text}"
        );
    }
}
fn footer(app: &App, width: u16, height: u16) -> String {
    rows_of(app, width, height).pop().unwrap()
}
#[test]
fn the_footer_only_offers_what_the_orbit_level_does() {
    let mut a = planets();
    a.pane = 0;
    let galaxy = footer(&a, 120, 38);
    assert!(galaxy.contains("Enter land"), "{galaxy}");
    assert!(
        !galaxy.contains("Esc") && galaxy.contains("n new workspace"),
        "{galaxy}"
    );
    let z = zoomed(Level::Workspace(path("w1")));
    for (w, h) in [(120, 38), (80, 24)] {
        let text = footer(&z, w, h);
        assert!(
            text.contains("Esc out") && text.contains("n new"),
            "{w}x{h}: {text}"
        );
    }
}
fn planet_bodies(app: &App, w: u16, h: u16) -> Vec<(PathBuf, f64, Color)> {
    orbit::scene(app, w, h)
        .bodies
        .into_iter()
        .filter_map(|b| match (b.action, b.look) {
            (Some(Action::OrbitEnter(Level::Workspace(p))), orbit::Look::Planet { color, .. }) => {
                Some((p, b.radius, color))
            }
            _ => None,
        })
        .collect()
}
#[test]
fn orbits_use_the_whole_width_of_a_wide_pane() {
    let (w, h) = (118, 17);
    let (buffer, _) = orbit_hits(&planets(), w, h, Rect::new(0, 0, w, h));
    let column = |x: u16| {
        (0..h).any(|y| {
            buffer[(x, y)]
                .symbol()
                .chars()
                .any(|c| ('\u{2801}'..='\u{28ff}').contains(&c))
        })
    };
    assert!(
        column(5) && column(w - 6),
        "{}",
        text_of(&buffer).join("\n")
    );
}
#[test]
fn planets_wear_distinct_stable_looks_and_grow_with_their_agents() {
    let a = planets();
    let bodies = planet_bodies(&a, 80, 16);
    assert_eq!(bodies.len(), 3);
    for (i, x) in bodies.iter().enumerate() {
        for y in &bodies[i + 1..] {
            assert_ne!(x.2, y.2, "{:?} and {:?} share a colour", x.0, y.0);
        }
    }
    let mut later = a;
    later.animation_elapsed = Duration::from_secs(7);
    let colors =
        |b: &[(PathBuf, f64, Color)]| b.iter().map(|x| (x.0.clone(), x.2)).collect::<Vec<_>>();
    assert_eq!(colors(&bodies), colors(&planet_bodies(&later, 80, 16)));
    let radius = |name: &str| bodies.iter().find(|b| b.0 == path(name)).unwrap().1;
    assert!(radius("w1") > radius("w0"), "w1 has two agents, w0 none");
}
