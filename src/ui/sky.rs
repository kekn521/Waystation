//! A braille dot raster with depth, for the orbit's art. Each terminal cell holds 2 × 4
//! dots, which are square on a typical 1:2 cell, so circles in dot space look round.
//! Every dot keeps the depth of what was drawn there: nearer dots win, and a dark dot (the
//! night side of a planet) still hides what passes behind it. Shading uses an ordered
//! dither anchored to each body, so a moving planet keeps its texture instead of shimmering.

use super::theme::MANTLE;
use ratatui::style::Color;

/// Light that reaches a sphere's night side, so its whole disc stays faintly visible.
const AMBIENT: f64 = 0.18;
/// How far the light sits towards the viewer, so faces turned to us are partly lit.
const LIGHT_DEPTH: f64 = 0.45;
/// Spheres smaller than this radius in dots are drawn solid rather than dithered.
const SOLID_BELOW: f64 = 10.0;

#[derive(Clone, Copy)]
struct Dot {
    z: f64,
    color: Option<Color>,
}
pub struct Sky {
    w: u16,
    h: u16,
    dots: Vec<Option<Dot>>,
}
/// How a sphere is coloured.
pub enum Shade {
    /// Lit by a light source, optionally striped like a gas giant.
    Lit { color: Color, bands: bool },
    /// Gives off its own light, bright at the core and deeper towards the rim.
    Glow { core: Color, rim: Color },
}
impl Shade {
    pub fn plain(color: Color) -> Self {
        Self::Lit {
            color,
            bands: false,
        }
    }
}

/// The 4 × 4 ordered-dither threshold for a dot, in (0, 1).
pub fn bayer(x: i32, y: i32) -> f64 {
    const M: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
    (f64::from(M[y.rem_euclid(4) as usize][x.rem_euclid(4) as usize]) + 0.5) / 16.0
}
/// `a` moved towards `b` by `t` (0 to 1); colours that are not RGB pass through as `b`.
pub fn mix(a: Color, b: Color, t: f64) -> Color {
    match (a, b) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => {
            let t = t.clamp(0.0, 1.0);
            let c = |x: u8, y: u8| (f64::from(x) + (f64::from(y) - f64::from(x)) * t).round() as u8;
            Color::Rgb(c(r1, r2), c(g1, g2), c(b1, b2))
        }
        _ => b,
    }
}

impl Sky {
    pub fn new(w: u16, h: u16) -> Self {
        Self {
            w,
            h,
            dots: vec![None; usize::from(w) * 2 * usize::from(h) * 4],
        }
    }
    /// Draw one dot at depth `z` (larger is nearer); `None` is a dark dot that only hides.
    pub fn dot(&mut self, x: i32, y: i32, z: f64, color: Option<Color>) {
        let (w, h) = (i32::from(self.w) * 2, i32::from(self.h) * 4);
        if x < 0 || y < 0 || x >= w || y >= h {
            return;
        }
        let slot = &mut self.dots[(y * w + x) as usize];
        if slot.is_none_or(|d| z >= d.z) {
            *slot = Some(Dot { z, color });
        }
    }
    /// The braille character for a cell and the colour of its nearest lit dot, if any.
    pub fn cell(&self, col: u16, row: u16) -> Option<(char, Color)> {
        const BITS: [(usize, usize, u32); 8] = [
            (0, 0, 0x01),
            (0, 1, 0x02),
            (0, 2, 0x04),
            (1, 0, 0x08),
            (1, 1, 0x10),
            (1, 2, 0x20),
            (0, 3, 0x40),
            (1, 3, 0x80),
        ];
        let w = usize::from(self.w) * 2;
        let (mut mask, mut nearest) = (0, None::<(f64, Color)>);
        for (dx, dy, bit) in BITS {
            let at = (usize::from(row) * 4 + dy) * w + usize::from(col) * 2 + dx;
            if let Some(Dot {
                z,
                color: Some(color),
            }) = self.dots.get(at).copied().flatten()
            {
                mask |= bit;
                if nearest.is_none_or(|(near, _)| z > near) {
                    nearest = Some((z, color));
                }
            }
        }
        Some((char::from_u32(0x2800 + mask)?, nearest?.1))
    }
    /// A sphere of radius `r` dots centred on dot (`cx`, `cy`) at depth `z`, lit from the
    /// screen direction `light` (or glowing, for a star).
    pub fn sphere(&mut self, cx: f64, cy: f64, r: f64, z: f64, shade: &Shade, light: (f64, f64)) {
        if r <= 0.0 {
            return;
        }
        let norm = (light.0 * light.0 + light.1 * light.1 + LIGHT_DEPTH * LIGHT_DEPTH).sqrt();
        let (lx, ly, lz) = (light.0 / norm, light.1 / norm, LIGHT_DEPTH / norm);
        // The dither grid starts at the body's corner, so it travels with the body.
        let (left, top) = ((cx - r).floor() as i32, (cy - r).floor() as i32);
        for y in top..=(cy + r).ceil() as i32 {
            for x in left..=(cx + r).ceil() as i32 {
                let (nx, ny) = ((f64::from(x) + 0.5 - cx) / r, (f64::from(y) + 0.5 - cy) / r);
                let d2 = nx * nx + ny * ny;
                if d2 > 1.0 {
                    continue;
                }
                let nz = (1.0 - d2).sqrt();
                let (brightness, color) = match *shade {
                    Shade::Lit { color, bands } => {
                        let lambert = (nx * lx + ny * ly + nz * lz).max(0.0);
                        let mut b = AMBIENT + (1.0 - AMBIENT) * lambert;
                        if bands {
                            b *= 0.72 + 0.28 * (ny * 9.0).cos();
                        }
                        // Few distinct shades keep repaints small as the light turns.
                        let step = (b * 6.0).round() / 6.0;
                        (b, mix(MANTLE, color, 0.3 + 0.7 * step))
                    }
                    Shade::Glow { core, rim } => (1.0, mix(rim, core, nz.powf(0.7))),
                };
                // Too small to dither into a readable gradient: solid, shaded by colour alone.
                let on = r < SOLID_BELOW || brightness > bayer(x - left, y - top);
                self.dot(x, y, z + nz * 1e-3, on.then_some(color));
            }
        }
    }
}
