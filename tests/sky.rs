use ratatui::style::Color;
use waystation::ui::sky::{Shade, Sky, bayer};

const A: Color = Color::Rgb(200, 100, 50);
const B: Color = Color::Rgb(50, 100, 200);

fn lit(sky: &Sky, col: u16, row: u16) -> u32 {
    sky.cell(col, row)
        .map_or(0, |(c, _)| (c as u32 - 0x2800).count_ones())
}

#[test]
fn dots_pack_into_braille_cells() {
    let mut sky = Sky::new(2, 1);
    sky.dot(0, 0, 0.0, Some(A));
    sky.dot(1, 3, 0.0, Some(A));
    assert_eq!(sky.cell(0, 0), Some(('⢁', A)));
    assert_eq!(sky.cell(1, 0), None, "an untouched cell stays empty");
    sky.dot(-1, 0, 0.0, Some(A));
    sky.dot(4, 0, 0.0, Some(A));
    sky.dot(0, 4, 0.0, Some(A));
}

#[test]
fn nearer_dots_win_and_dark_ones_still_hide_what_is_behind() {
    let mut sky = Sky::new(1, 1);
    sky.dot(0, 0, 0.0, Some(A));
    sky.dot(0, 0, -1.0, Some(B));
    assert_eq!(
        sky.cell(0, 0),
        Some(('⠁', A)),
        "a farther dot cannot cover a nearer one"
    );
    sky.dot(0, 0, 1.0, None);
    assert_eq!(
        sky.cell(0, 0),
        None,
        "a dark near dot hides the lit far one"
    );
    sky.dot(1, 0, 2.0, Some(B));
    assert_eq!(sky.cell(0, 0), Some(('⠈', B)));
}

#[test]
fn the_nearest_dot_colours_its_cell() {
    let mut sky = Sky::new(1, 1);
    sky.dot(0, 0, 0.0, Some(A));
    sky.dot(1, 1, 0.5, Some(B));
    assert_eq!(sky.cell(0, 0).unwrap().1, B);
}

#[test]
fn bayer_thresholds_cover_the_unit_interval_evenly() {
    let mut all = (0..4)
        .flat_map(|y| (0..4).map(move |x| bayer(x, y)))
        .collect::<Vec<_>>();
    all.sort_by(f64::total_cmp);
    for (i, t) in all.iter().enumerate() {
        assert!((t - (i as f64 + 0.5) / 16.0).abs() < 1e-9, "{all:?}");
    }
    assert_eq!(bayer(-1, -1), bayer(3, 3), "negative coordinates wrap");
}

#[test]
fn a_large_sphere_dithers_bright_towards_its_light_and_dim_away_from_it() {
    let mut sky = Sky::new(16, 8);
    // Radius 14 dots in the middle, lit from the right.
    sky.sphere(16.0, 16.0, 14.0, 0.0, &Shade::plain(A), (1.0, 0.0));
    let side = |col| (2..6).map(|row| lit(&sky, col, row)).sum::<u32>();
    let (dark, bright) = (side(3), side(12));
    assert!(bright > dark + 8, "lit {bright} vs dark {dark}");
    assert!(dark > 0, "the night side still shows a faint disc");
    assert_eq!(sky.cell(0, 0), None, "nothing outside the disc");
}

#[test]
fn a_small_sphere_is_solid_and_shaded_by_colour() {
    let mut sky = Sky::new(6, 2);
    sky.sphere(6.0, 4.0, 5.0, 0.0, &Shade::plain(A), (1.0, 0.0));
    let red = |col| match sky.cell(col, 0).or(sky.cell(col, 1)) {
        Some((_, Color::Rgb(r, _, _))) => r,
        other => panic!("cell {col}: {other:?}"),
    };
    assert_eq!(
        sky.cell(2, 0).map(|c| c.0),
        Some('⣿'),
        "solid, not dithered"
    );
    assert!(
        red(4) > red(1),
        "the lit side is brighter: {} vs {}",
        red(4),
        red(1)
    );
}

#[test]
fn a_sphere_covers_what_passes_behind_it() {
    let mut sky = Sky::new(12, 4);
    for x in 0..24 {
        sky.dot(x, 8, -1.0, Some(B));
    }
    sky.sphere(12.0, 8.0, 5.0, 0.0, &Shade::plain(A), (1.0, 0.0));
    for col in 4..8 {
        assert_ne!(
            sky.cell(col, 2).map(|c| c.1),
            Some(B),
            "ring shows through at {col}"
        );
    }
    assert_eq!(
        sky.cell(0, 2).map(|c| c.1),
        Some(B),
        "and stays visible beside it"
    );
}
