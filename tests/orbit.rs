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
fn overview_shows_four_panels_with_orbit_upper_left() {
    for (w, h) in [(120, 38), (100, 32)] {
        let a = app();
        let text = screen(&a, w, h);
        // Row-major pane order: 0 Orbit upper-left, 1 Workspaces upper-right,
        // 2 System lower-left, 3 Services lower-right.
        for want in [
            "✦",
            "Orbit",
            "Continue working",
            "Machine pulse",
            "Services & ports",
        ] {
            assert!(text.contains(want), "missing {want:?} at {w}x{h}");
        }
        assert!(
            !text.contains("Tasks"),
            "Tasks must not be mentioned at {w}x{h}"
        );
        let rows = rows_of(&a, w, h);
        let orbit = label_pos(&rows, "Orbit");
        let workspaces = label_pos(&rows, "Continue working");
        let system = label_pos(&rows, "Machine pulse");
        let services = label_pos(&rows, "Services & ports");
        // Left column: orbit above system. Right column: workspaces above services.
        assert!(
            orbit.0 < workspaces.0,
            "orbit {orbit:?} must sit left of workspaces {workspaces:?} at {w}x{h}"
        );
        assert!(
            system.0 < services.0,
            "system {system:?} must sit left of services {services:?} at {w}x{h}"
        );
        assert!(
            workspaces.1 < system.1,
            "workspaces {workspaces:?} must sit above system {system:?} at {w}x{h}"
        );
        assert!(
            orbit.1 < services.1,
            "orbit {orbit:?} must sit above services {services:?} at {w}x{h}"
        );
    }
}
#[test]
fn narrow_default_hides_orbit_but_pane_zero_shows_it() {
    let a = app();
    // Default focus is pane 1 (Workspaces), so the single-pane view shows no sun.
    let single = screen(&a, 80, 24);
    assert!(!single.contains("✦"), "orbit leaked into default 80x24");
    assert!(single.contains("Continue working"));
    // Focusing pane 0 in a narrow layout renders the orbit itself.
    let mut orbit_pane = app();
    orbit_pane.pane = 0;
    for (w, h) in [(80, 24), (60, 18)] {
        let text = screen(&orbit_pane, w, h);
        assert!(text.contains("✦"), "pane 0 must show the sun at {w}x{h}");
        assert!(
            !text.contains("Continue working"),
            "pane 0 must display the orbit, not another pane, at {w}x{h}"
        );
    }
    // Pane 2 is the System pulse in the new row-major order.
    let mut focused = app();
    focused.pane = 2;
    let narrow = screen(&focused, 60, 18);
    assert!(!narrow.contains("✦"), "orbit leaked into pane 2 at 60x18");
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
        let symbols = (hit.area.x..hit.area.right())
            .map(|x| buffer[(x, hit.area.y)].symbol().to_string())
            .collect::<String>();
        assert!(
            symbols.contains('●') || symbols.contains('◉'),
            "{name}'s hit {:?} misses its planet: {symbols:?}",
            hit.area
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
    assert!(
        !text.contains("Continue working"),
        "the Workspaces pane gives way"
    );
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
        let (buffer, _) = orbit_hits(&a, 40, 10, Rect::new(0, 0, 40, 10));
        let text = text_of(&buffer).join("\n");
        assert!(
            text.contains('◉'),
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
        !galaxy.contains("Esc") && !galaxy.contains(" n "),
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
