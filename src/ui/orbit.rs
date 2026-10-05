//! A decorative solar system with a clock supplied by the UI, not a background task.

use super::theme::{BLUE, LINE, MANTLE, MAUVE, PEACH, TEAL};
use ratatui::{
    Frame,
    layout::Rect,
    style::Color,
    widgets::canvas::{Canvas, Points},
};
use std::{f64::consts::TAU, time::Duration};

/// Render a clipped, deterministic frame of three slow planetary orbits.
pub fn render(frame: &mut Frame, area: Rect, elapsed: Duration) {
    let area = area.intersection(frame.area());
    if area.is_empty() {
        return;
    }
    let cx = f64::from(area.width.saturating_sub(1)) / 2.0;
    let cy = f64::from(area.height.saturating_sub(1)) / 2.0;
    let radius = (cx - 2.0).clamp(0.0, 30.0);
    let vertical = (cy - 0.25).max(0.0);
    // Quantize to eight frames per second, independent of input and provider refreshes.
    let seconds = (elapsed.as_millis() / 125) as f64 / 8.0;
    let orbits = [
        (0.34, 0.48, 12.0, 0.3, TEAL, "●"),
        (0.65, 0.75, 20.0, 2.5, BLUE, "◉"),
        (1.0, 1.0, 32.0, 4.3, MAUVE, "●"),
    ];
    // Braille gives each terminal cell 2 × 4 dots, enough for shallow, smooth ellipses.
    frame.render_widget(
        Canvas::default()
            .background_color(MANTLE)
            .x_bounds([0.0, (cx * 2.0).max(1.0)])
            .y_bounds([0.0, (cy * 2.0).max(1.0)])
            .paint(|ctx| {
                for &(horizontal, height, _, _, _, _) in &orbits {
                    let points = (0..180)
                        .map(|sample| {
                            let angle = f64::from(sample) / 180.0 * TAU;
                            (
                                cx + radius * horizontal * angle.cos(),
                                cy + vertical * height * angle.sin(),
                            )
                        })
                        .collect::<Vec<_>>();
                    ctx.draw(&Points::new(&points, LINE));
                }
            }),
        area,
    );
    let buffer = frame.buffer_mut();
    let mut put = |x: i32, y: i32, symbol: &str, color: Color| {
        if x >= 0 && y >= 0 && x < i32::from(area.width) && y < i32::from(area.height) {
            buffer[(area.x + x as u16, area.y + y as u16)]
                .set_symbol(symbol)
                .set_fg(color);
        }
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
        put(i32::from(area.width) * x / 100, y, symbol, LINE);
    }

    let position = |rx: f64, ry: f64, angle: f64| {
        (
            (cx + rx * angle.cos()).round() as i32,
            (cy + ry * angle.sin()).round() as i32,
        )
    };
    for &(horizontal, height, period, phase, color, symbol) in &orbits {
        let angle = seconds.rem_euclid(period) / period * TAU + phase;
        let rx = radius * horizontal;
        let ry = vertical * height;
        for offset in [0.3, 0.15] {
            let (x, y) = position(rx, ry, angle - offset);
            put(x, y, "·", color);
        }
        let (x, y) = position(rx, ry, angle);
        if horizontal == 1.0 {
            put(x - 1, y, "─", color);
            put(x + 1, y, "─", color);
        }
        put(x, y, symbol, color);
    }
    put(cx.round() as i32, cy.round() as i32, "✦", PEACH);
}
