use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Stylize},
    widgets::Block,
};
use std::time::Duration;
use waystation::{
    app::App,
    config::Config,
    model::{AppState, Section},
    ui::{draw, orbit},
};
fn app() -> App {
    App::new(Config::default(), AppState::default())
}
/// Renders the orbit into `area` over a uniform DarkGray backdrop so that any
/// cell touched by the renderer is detectable against the baseline.
fn orbit_buffer(width: u16, height: u16, area: Rect, elapsed: Duration) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| {
            let full = f.area();
            f.render_widget(Block::default().bg(Color::DarkGray), full);
            orbit::render(f, area, elapsed);
        })
        .unwrap();
    terminal.backend().buffer().clone()
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
    let mut a = app();
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
    let mut workspaces = app();
    workspaces.section = Section::Workspaces;
    let before = buffer_of(&workspaces, 120, 38);
    workspaces.animation_elapsed = Duration::from_secs(3);
    assert_eq!(
        before,
        buffer_of(&workspaces, 120, 38),
        "Workspaces must ignore the animation clock"
    );
}
