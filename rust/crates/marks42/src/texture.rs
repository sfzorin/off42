//! Shared drawing helpers (Kit42 `InkDraw` and `InkTexture`): canvases, tapered pen strokes, trimming and the ink
//! texture that makes a clean vector drawing look like real ink on paper.
//!
//! Coordinates: the macOS code draws in CoreGraphics space (origin bottom-left, y up) and then reads bitmap rows
//! top-down. Ports keep their geometry in that y-up space and draw through [`cg`], which flips it onto the
//! top-down pixmap, so noise and gradients see the same pixel rows as on macOS.

use crate::{rng::SeededRandom, Ink};
use tiny_skia::{FillRule, Paint, Path, PathBuilder, Pixmap, Transform};

/// A point in CoreGraphics (y-up) space.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f64,
    pub y: f64,
}

pub const fn pt(x: f64, y: f64) -> Pt {
    Pt { x, y }
}

/// Transparent premultiplied RGBA canvas (Kit42 `InkDraw.context`).
pub fn canvas(width: u32, height: u32) -> Option<Pixmap> {
    Pixmap::new(width.max(1), height.max(1))
}

/// Transform from CoreGraphics (y-up) coordinates to the top-down pixmap of the given height.
pub fn cg(height: u32) -> Transform {
    Transform::from_row(1.0, 0.0, 0.0, -1.0, 0.0, height as f32)
}

/// Solid paint in the ink colour with the given opacity (0..=1).
pub fn paint(ink: Ink, alpha: f64) -> Paint<'static> {
    let mut p = Paint::default();
    let [r, g, b] = ink.to_u8();
    p.set_color_rgba8(r, g, b, (alpha.clamp(0.0, 1.0) * 255.0).round() as u8);
    p.anti_alias = true;
    p
}

/// Fills a y-up path on the canvas.
pub fn fill(pixmap: &mut Pixmap, path: &Path, ink: Ink, alpha: f64, rule: FillRule) {
    let h = pixmap.height();
    pixmap.fill_path(path, &paint(ink, alpha), rule, cg(h), None);
}

/// A closed polygon through the points (y-up).
pub fn polygon(points: &[Pt]) -> Option<Path> {
    let mut b = PathBuilder::new();
    let first = points.first()?;
    b.move_to(first.x as f32, first.y as f32);
    for p in &points[1..] {
        b.line_to(p.x as f32, p.y as f32);
    }
    b.close();
    b.finish()
}

/// A pen stroke along the points: a filled outline whose width follows `width(t)`, t in 0..=1
/// (Kit42 `InkDraw.taperedStroke`).
pub fn tapered_stroke(points: &[Pt], width: impl Fn(f64) -> f64) -> Option<Path> {
    if points.len() < 2 {
        return None;
    }
    let n = points.len();
    let mut left = Vec::with_capacity(n);
    let mut right = Vec::with_capacity(n);
    for (i, p) in points.iter().enumerate() {
        let a = points[i.saturating_sub(1)];
        let b = points[(i + 1).min(n - 1)];
        let (mut dx, mut dy) = (b.x - a.x, b.y - a.y);
        let len = (dx * dx + dy * dy).sqrt().max(0.0001);
        dx /= len;
        dy /= len;
        let w = width(i as f64 / (n - 1) as f64) / 2.0;
        left.push(pt(p.x - dy * w, p.y + dx * w));
        right.push(pt(p.x + dy * w, p.y - dx * w));
    }
    right.reverse();
    left.extend(right);
    polygon(&left)
}

/// Points along a cubic Bézier (Kit42 `InkDraw.cubic`).
pub fn cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, steps: usize) -> Vec<Pt> {
    (0..=steps)
        .map(|i| {
            let t = i as f64 / steps as f64;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            pt(a * p0.x + b * p1.x + c * p2.x + d * p3.x, a * p0.y + b * p1.y + c * p2.y + d * p3.y)
        })
        .collect()
}

/// Copies a path, moving every point through `f` (Kit42 `InkDraw.warp`).
pub fn warp(path: &Path, f: impl Fn(Pt) -> Pt) -> Option<Path> {
    use tiny_skia::PathSegment::*;
    let m = |p: tiny_skia::Point| {
        let q = f(pt(p.x as f64, p.y as f64));
        (q.x as f32, q.y as f32)
    };
    let mut b = PathBuilder::new();
    for seg in path.segments() {
        match seg {
            MoveTo(p) => {
                let (x, y) = m(p);
                b.move_to(x, y)
            }
            LineTo(p) => {
                let (x, y) = m(p);
                b.line_to(x, y)
            }
            QuadTo(c, p) => {
                let ((cx, cy), (x, y)) = (m(c), m(p));
                b.quad_to(cx, cy, x, y)
            }
            CubicTo(c1, c2, p) => {
                let ((ax, ay), (bx, by), (x, y)) = (m(c1), m(c2), m(p));
                b.cubic_to(ax, ay, bx, by, x, y)
            }
            Close => b.close(),
        }
    }
    b.finish()
}

/// Crops fully transparent margins, keeping `pad` pixels (Kit42 `InkDraw.trim`).
pub fn trim(pixmap: &Pixmap, pad: u32) -> Pixmap {
    let (w, h) = (pixmap.width() as i64, pixmap.height() as i64);
    let data = pixmap.data();
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (w, h, -1i64, -1i64);
    for y in 0..h {
        for x in 0..w {
            if data[((y * w + x) * 4 + 3) as usize] > 8 {
                min_x = min_x.min(x);
                max_x = max_x.max(x);
                min_y = min_y.min(y);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        return pixmap.clone();
    }
    let pad = pad as i64;
    let (x0, y0) = ((min_x - pad).max(0), (min_y - pad).max(0));
    let (x1, y1) = ((max_x + pad + 1).min(w), (max_y + pad + 1).min(h));
    let rect = tiny_skia::IntRect::from_xywh(x0 as i32, y0 as i32, (x1 - x0) as u32, (y1 - y0) as u32).unwrap();
    pixmap.clone_rect(rect).unwrap_or_else(|| pixmap.clone())
}

/// How strongly the ink texture acts (Kit42 `InkTexture.Strength`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strength {
    /// How much the overall pressure varies (0 = even, 1 = some areas almost blank).
    pub pressure: f64,
    /// Share of fine gaps where the pad did not touch the paper.
    pub gaps: f64,
    /// Edge softening in pixels.
    pub bleed: f64,
    /// Pen only: pressure that rises and falls along the writing direction (left to right).
    pub along_x: f64,
    /// Paper fibres: fine elongated texture that takes up less ink (0 = smooth paper).
    pub paper: f64,
}

/// Stamp impressions (Kit42 `InkTexture.stamp`).
pub const STAMP: Strength = Strength { pressure: 0.55, gaps: 0.30, bleed: 0.9, along_x: 0.0, paper: 0.0 };

/// Smooth value noise in 0..=1 (Kit42 `InkTexture.Noise`).
struct Noise {
    seed: u64,
    cell: f64,
}

impl Noise {
    fn hash(&self, x: i64, y: i64) -> f64 {
        let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F)
            ^ self.seed;
        h = (h ^ (h >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        h ^= h >> 29;
        (h & 0xFFFF) as f64 / 65535.0
    }

    fn value(&self, px: f64, py: f64) -> f64 {
        let (x, y) = (px / self.cell, py / self.cell);
        let (x0, y0) = (x.floor() as i64, y.floor() as i64);
        let (mut fx, mut fy) = (x - x0 as f64, y - y0 as f64);
        fx = fx * fx * (3.0 - 2.0 * fx);
        fy = fy * fy * (3.0 - 2.0 * fy);
        let (a, b, c, d) = (self.hash(x0, y0), self.hash(x0 + 1, y0), self.hash(x0, y0 + 1), self.hash(x0 + 1, y0 + 1));
        (a + (b - a) * fx) * (1.0 - fy) + (c + (d - c) * fx) * fy
    }
}

/// Uneven pressure, grain, small gaps, paper fibres and slight bleed (Kit42 `InkTexture.apply`).
/// `scale`: pixels per pixel of the base rendering, so a high-resolution impression looks the same, only sharper.
pub fn apply(pixmap: &mut Pixmap, seed: u64, strength: Strength, scale: f64) {
    let (w, h) = (pixmap.width() as usize, pixmap.height() as usize);
    let mut rng = SeededRandom::new(seed);
    let size = w.max(h) as f64;
    let pressure = Noise { seed: rng.next_u64(), cell: size / 3.5 };
    let mottle = Noise { seed: rng.next_u64(), cell: size / 28.0 };
    let grain = Noise { seed: rng.next_u64(), cell: (2.2 * scale).max(size / 260.0) };
    let stroke = Noise { seed: rng.next_u64(), cell: w as f64 / 7.0 };
    let fibre = Noise { seed: rng.next_u64(), cell: 2.2 * scale };
    let fibre_angle = rng.range(0.0, std::f64::consts::PI);
    let (fc, fs) = (fibre_angle.cos(), fibre_angle.sin());
    // The pad presses harder on one side: a gentle gradient in a random direction.
    let angle = rng.range(0.0, 2.0 * std::f64::consts::PI);
    let tilt = rng.range(0.15, 0.45) * strength.pressure;
    let (gx, gy) = (angle.cos(), angle.sin());
    let data = pixmap.data_mut();
    for y in 0..h {
        let fy = y as f64;
        for x in 0..w {
            let i = (y * w + x) * 4;
            if data[i + 3] == 0 {
                continue;
            }
            let fx = x as f64;
            let side = ((fx / size - 0.5) * gx + (fy / size - 0.5) * gy) * 2.0;
            let mut k = 1.0 - strength.pressure * 0.55 * (1.0 - pressure.value(fx, fy)) - tilt * side.max(0.0);
            k -= strength.pressure * 0.25 * (1.0 - mottle.value(fx, fy));
            if strength.along_x > 0.0 {
                k -= strength.along_x * (1.0 - stroke.value(fx, 0.5));
            }
            if strength.paper > 0.0 {
                let u = (fx * fc + fy * fs) * 0.1;
                let v = -fx * fs + fy * fc;
                k -= strength.paper * (1.0 - fibre.value(u, v));
            }
            let g = grain.value(fx, fy);
            let gap_limit = strength.gaps * (0.55 + 0.9 * (1.0 - k));
            if g < gap_limit {
                k *= 0.12 + g / gap_limit.max(0.001) * 0.5;
            }
            k = (k * rng.range(0.94, 1.0)).clamp(0.05, 1.0);
            for c in 0..4 {
                data[i + c] = (data[i + c] as f64 * k) as u8;
            }
        }
    }
    blur(pixmap, strength.bleed * scale);
}

/// Gaussian blur of a premultiplied RGBA canvas with edge clamping (CIGaussianBlur on a clamped image).
pub fn blur(pixmap: &mut Pixmap, sigma: f64) {
    if sigma <= 0.0 {
        return;
    }
    let radius = (sigma * 3.0).ceil() as i64;
    let kernel: Vec<f64> = (-radius..=radius).map(|i| (-(i * i) as f64 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f64 = kernel.iter().sum();
    let kernel: Vec<f64> = kernel.iter().map(|k| k / total).collect();
    let (w, h) = (pixmap.width() as i64, pixmap.height() as i64);
    let src: Vec<f64> = pixmap.data().iter().map(|&v| v as f64).collect();
    let mut tmp = vec![0.0; src.len()];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let mut acc = 0.0;
                for (j, k) in kernel.iter().enumerate() {
                    let sx = (x + j as i64 - radius).clamp(0, w - 1);
                    acc += src[((y * w + sx) * 4 + c) as usize] * k;
                }
                tmp[((y * w + x) * 4 + c) as usize] = acc;
            }
        }
    }
    let data = pixmap.data_mut();
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let mut px = [0.0; 4];
            for (j, k) in kernel.iter().enumerate() {
                let sy = (y + j as i64 - radius).clamp(0, h - 1);
                let s = ((sy * w + x) * 4) as usize;
                for c in 0..4 {
                    px[c] += tmp[s + c] * k;
                }
            }
            // Premultiplied colour can never exceed its alpha.
            let a = px[3].round().clamp(0.0, 255.0);
            data[i + 3] = a as u8;
            for c in 0..3 {
                data[i + c] = px[c].round().clamp(0.0, a) as u8;
            }
        }
    }
}

/// Converts a premultiplied canvas into a straight-alpha image for PNG output.
pub fn to_image(pixmap: &Pixmap) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(pixmap.width(), pixmap.height());
    for (dst, src) in out.pixels_mut().zip(pixmap.pixels()) {
        let c = src.demultiply();
        dst.0 = [c.red(), c.green(), c.blue(), c.alpha()];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tapered_stroke_fills_ink_and_trim_crops_margins() {
        let mut p = canvas(200, 100).unwrap();
        let line: Vec<Pt> = (0..=20).map(|i| pt(20.0 + i as f64 * 8.0, 50.0)).collect();
        let path = tapered_stroke(&line, |t| 2.0 + 6.0 * (1.0 - t)).unwrap();
        fill(&mut p, &path, crate::InkColor::Blue.pen(), 1.0, FillRule::Winding);
        let t = trim(&p, 6);
        assert!(t.width() < 200 && t.width() > 150 && t.height() < 30, "{}x{}", t.width(), t.height());
    }

    #[test]
    fn texture_is_deterministic_and_keeps_premultiplied_pixels_valid() {
        let make = || {
            let mut p = canvas(120, 120).unwrap();
            let sq = polygon(&[pt(10., 10.), pt(110., 10.), pt(110., 110.), pt(10., 110.)]).unwrap();
            fill(&mut p, &sq, crate::InkColor::Red.stamp_pad(), 1.0, FillRule::Winding);
            apply(&mut p, 99, STAMP, 1.0);
            p
        };
        let (a, b) = (make(), make());
        assert_eq!(a.data(), b.data());
        assert!(a.pixels().iter().all(|px| px.red() <= px.alpha() && px.green() <= px.alpha() && px.blue() <= px.alpha()));
        let inked = a.pixels().iter().filter(|px| px.alpha() > 40).count();
        assert!(inked > 6000 && inked < 10400, "{inked}");
    }
}
