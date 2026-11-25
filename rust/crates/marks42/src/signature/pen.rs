//! Draws timed pen trajectories as ink on paper (port of Kit42 `PenRenderer`). Every effect comes from the pen's
//! real speed and direction:
//! - ballpoint: near-constant width, more ink on downstrokes (more pressure), blobs where the pen is slow, skips
//!   where it is fast, fine striations left by the ball along the stroke;
//! - gel: wider, even and saturated, few skips;
//! - fountain: width follows the angle between stroke and nib, ink pools at slow ends;
//! - felt-tip: broad soft line that dries out (lightens) at speed;
//! - paper: a fibre texture under every pen.

use super::kinematics::PenSample;
use crate::rng::SeededRandom;
use crate::texture::{self, canvas, cg, pt, tapered_stroke, Pt, Strength};
use crate::Ink;
use serde::{Deserialize, Serialize};
use tiny_skia::{FillRule, LineCap, Mask, Path, PathBuilder, Pixmap, Rect, Stroke, Transform};

/// Writing instruments for signatures (Kit42 `PenType`, same raw values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Pen {
    #[default]
    Ballpoint,
    Gel,
    Fountain,
    Felt,
}

impl Pen {
    pub const ALL: [Pen; 4] = [Pen::Ballpoint, Pen::Gel, Pen::Fountain, Pen::Felt];

    pub fn title(self) -> &'static str {
        match self {
            Pen::Ballpoint => "Ballpoint pen",
            Pen::Gel => "Gel pen",
            Pen::Fountain => "Fountain pen",
            Pen::Felt => "Felt-tip pen",
        }
    }

    /// Kit42 `PenType.rawValue`.
    pub fn raw_value(self) -> u8 {
        self as u8
    }

    pub fn from_raw_value(raw: u8) -> Option<Pen> {
        Pen::ALL.get(raw as usize).copied()
    }
}

/// `strokes` in pixels (y up), `pen` = nominal line width in pixels. `scale`: pixels per base pixel.
/// Returns the textured, trimmed premultiplied canvas.
pub(crate) fn draw(strokes: &[Vec<PenSample>], kind: Pen, pen: f64, ink: Ink, rng: &mut SeededRandom, scale: f64) -> Option<Pixmap> {
    let paths: Vec<Vec<PenSample>> = strokes.iter().map(|s| resample(s, scale)).filter(|p| p.len() > 2).collect();
    if paths.is_empty() {
        return None;
    }
    let reach = pen * if kind == Pen::Fountain { 3.0 } else { 4.0 };
    let all: Vec<&PenSample> = paths.iter().flatten().collect();
    let fold = |f: fn(f64, f64) -> f64, g: fn(&PenSample) -> f64| all.iter().map(|s| g(s)).reduce(f).unwrap();
    let min_x = fold(f64::min, |s| s.p.x) - reach;
    let max_x = fold(f64::max, |s| s.p.x) + reach;
    let min_y = fold(f64::min, |s| s.p.y) - reach;
    let max_y = fold(f64::max, |s| s.p.y) + reach;
    let (wf, hf) = ((max_x - min_x).ceil(), (max_y - min_y).ceil());
    if !(wf > 0.0 && hf > 0.0 && wf < 6000.0 * scale && hf < 4000.0 * scale) {
        return None;
    }
    let (w, h) = (wf as u32, hf as u32);
    let mut out = canvas(w, h)?;
    let to_canvas = cg(h);
    // Speeds normalised over the whole signature (robust to outliers).
    let mut speeds: Vec<f64> = all.iter().map(|s| s.speed).collect();
    speeds.sort_by(|a, b| a.total_cmp(b));
    let lo = speeds[((speeds.len() - 1) as f64 * 0.08) as usize];
    let hi = (lo + 1e-6).max(speeds[((speeds.len() - 1) as f64 * 0.92) as usize]);
    let nib = rng.range(0.55, 0.85); // fountain nib angle (≈ 30–50°)
    let skip_noise = SeededRandom::new(rng.next_u64());
    let norm = |speed: f64| ((speed - lo) / (hi - lo)).clamp(0.0, 1.0);

    for path in &paths {
        let pts: Vec<Pt> = path.iter().map(|s| pt(s.p.x - min_x, s.p.y - min_y)).collect();
        let n = pts.len();
        let mut width = Vec::with_capacity(n);
        let mut alpha = Vec::with_capacity(n);
        let mut length = 0.0;
        let mut lengths = vec![0.0];
        for i in 1..n {
            length += (pts[i].x - pts[i - 1].x).hypot(pts[i].y - pts[i - 1].y);
            lengths.push(length);
        }
        for i in 0..n {
            let s = &path[i];
            let v = norm(s.speed);
            let t = lengths[i] / length.max(1.0);
            // Pen lands and lifts while moving: thin, faint ends.
            let land = (t / 0.04).min(1.0);
            let lift = ((1.0 - t) / 0.08).min(1.0);
            let touch = land.min(lift);
            let down = (-s.direction.sin()).max(0.0); // moving down the page: more pressure
            let (wd, a) = match kind {
                Pen::Ballpoint => (pen * (1.0 - 0.16 * v + 0.2 * down), 0.95 - 0.25 * v + 0.12 * down),
                Pen::Gel => (pen * 1.25 * (1.0 - 0.08 * v + 0.1 * down), 1.0 - 0.06 * v),
                Pen::Fountain => {
                    let across = (s.direction - nib).sin().abs();
                    (pen * (0.25 + 2.6 * across), 0.96 - 0.12 * v + 0.08 * (1.0 - v))
                }
                Pen::Felt => (pen * 3.0, 0.82 - 0.35 * v),
            };
            // Touch-down: the pen lands pressing; the lift tapers.
            let taper = if t < 0.5 { 0.75 + 0.25 * land } else { 0.3 + 0.7 * lift };
            width.push(wd * taper);
            alpha.push((a - 0.3 * (1.0 - touch)).clamp(0.15, 1.0));
        }
        let Some(ribbon) = tapered_stroke(&pts, |t| width[(n - 1).min((t * (n - 1) as f64).round() as usize)]) else { continue };
        let Some(mut clip) = Mask::new(w, h) else { continue };
        clip.fill_path(&ribbon, FillRule::Winding, true, to_canvas);
        let mut layer = Layer::new(w, h);
        // Density along the stroke without darker overlaps ("copy": the last segment over a pixel wins).
        let line = Stroke { width: (pen * if kind == Pen::Fountain { 5.0 } else { 3.0 }) as f32, line_cap: LineCap::Round, miter_limit: 10.0, ..Stroke::default() };
        let mut skip = skip_noise.clone();
        let mut skip_left = 0.0;
        for i in 1..n {
            let mut a = alpha[i];
            if kind == Pen::Ballpoint || kind == Pen::Felt {
                let v = norm(path[i].speed);
                // Skips: short dropouts on the fastest parts.
                if skip_left <= 0.0 && v > 0.85 && skip.chance(if kind == Pen::Ballpoint { 0.002 } else { 0.003 }) {
                    skip_left = skip.range(1.0, 2.5) * scale;
                }
                if skip_left > 0.0 {
                    a *= if kind == Pen::Ballpoint { 0.4 } else { 0.6 };
                    skip_left -= lengths[i] - lengths[i - 1];
                }
            }
            let mut b = PathBuilder::new();
            b.move_to(pts[i - 1].x as f32, pts[i - 1].y as f32);
            b.line_to(pts[i].x as f32, pts[i].y as f32);
            if let Some(segment) = b.finish() {
                let [r, g, bl] = ink.to_u8().map(|c| c as f32 * a as f32);
                layer.stroke(&segment, &line, to_canvas, &clip, |px, c| {
                    // kCGBlendModeCopy under a clip: dst = lerp(dst, src, coverage).
                    for (d, s) in px.iter_mut().zip([r, g, bl, 255.0 * a as f32]) {
                        *d += (s - *d) * c;
                    }
                });
            }
        }
        if kind == Pen::Ballpoint {
            // Striations: the ball leaves fine lighter grooves along the stroke.
            for offset in [-0.28, 0.3] {
                let mut groove = PathBuilder::new();
                for i in 0..n {
                    let a = pts[i.saturating_sub(1)];
                    let b = pts[(i + 1).min(n - 1)];
                    let d = (b.x - a.x).hypot(b.y - a.y).max(0.001);
                    let q = pt(pts[i].x - (b.y - a.y) / d * width[i] * offset, pts[i].y + (b.x - a.x) / d * width[i] * offset);
                    if i == 0 {
                        groove.move_to(q.x as f32, q.y as f32);
                    } else {
                        groove.line_to(q.x as f32, q.y as f32);
                    }
                }
                if let Some(groove) = groove.finish() {
                    let stroke = Stroke { width: (0.45 * scale).max(pen * 0.16) as f32, line_cap: LineCap::Round, miter_limit: 10.0, ..Stroke::default() };
                    // Destination-out with 10 % black.
                    layer.stroke(&groove, &stroke, to_canvas, &clip, |px, c| {
                        let k = 1.0 - 0.1 * c;
                        px.iter_mut().for_each(|d| *d *= k);
                    });
                }
            }
        }
        layer.draw_over(&mut out);
        // Blobs where the pen moved slowly (ink gathers on the ball / nib), mostly at landings and turns.
        let step = ((pen * 2.0 / scale) as usize).max(1);
        for i in (0..n).step_by(step) {
            let v = norm(path[i].speed);
            let chance = if kind == Pen::Gel { 0.0 } else if v < 0.06 { 0.08 } else { 0.0 };
            if rng.chance(chance) || (i == 0 && rng.chance(if kind == Pen::Fountain { 0.5 } else { 0.18 })) {
                let r = width[i] * rng.range(0.55, 0.8);
                if let Some(oval) = Rect::from_xywh((pts[i].x - r) as f32, (pts[i].y - r) as f32, (2.0 * r) as f32, (2.0 * r) as f32)
                    .and_then(PathBuilder::from_oval)
                {
                    out.fill_path(&oval, &texture::paint(ink, 0.9), FillRule::Winding, to_canvas, None);
                }
            }
        }
    }
    // Ballpoint ink is dense: grain and fibres show only at the edges of the line, not as faded stretches.
    let mut strength = match kind {
        Pen::Ballpoint => {
            let pressure = rng.range(0.05, 0.1);
            let gaps = rng.range(0.004, 0.012);
            let along_x = rng.range(0.03, 0.07);
            Strength { pressure, gaps, bleed: 0.3, along_x, paper: 0.0 }
        }
        Pen::Gel => Strength { pressure: 0.04, gaps: 0.004, bleed: 0.35, along_x: 0.03, paper: 0.0 },
        Pen::Fountain => Strength { pressure: 0.08, gaps: 0.006, bleed: 0.55, along_x: 0.05, paper: 0.0 },
        Pen::Felt => Strength { pressure: 0.12, gaps: 0.02, bleed: 0.9, along_x: 0.08, paper: 0.0 },
    };
    strength.paper = if kind == Pen::Ballpoint || kind == Pen::Gel { rng.range(0.04, 0.08) } else { rng.range(0.1, 0.18) };
    texture::apply(&mut out, rng.next_u64(), strength, scale);
    Some(texture::trim(&out, (6.0 * scale).round() as u32))
}

/// Even spacing along the path (`step` pixels), interpolating speed and direction.
fn resample(s: &[PenSample], step: f64) -> Vec<PenSample> {
    if s.len() <= 2 {
        return s.to_vec();
    }
    let mut out = vec![s[0]];
    let mut carry = 0.0;
    for i in 1..s.len() {
        let (a, b) = (s[i - 1], s[i]);
        let len = (b.p.x - a.p.x).hypot(b.p.y - a.p.y);
        if !(len > 0.0) {
            continue;
        }
        let mut t = step - carry;
        while t <= len {
            let k = t / len;
            out.push(PenSample {
                p: pt(a.p.x + (b.p.x - a.p.x) * k, a.p.y + (b.p.y - a.p.y) * k),
                t: a.t + (b.t - a.t) * k,
                speed: a.speed + (b.speed - a.speed) * k,
                direction: (b.p.y - a.p.y).atan2(b.p.x - a.p.x),
            });
            t += step;
        }
        carry = len - (t - step);
    }
    out.push(s[s.len() - 1]);
    out
}

/// A layer with CoreGraphics compositing semantics that tiny-skia does not offer: each stroke's anti-aliased
/// coverage (times the clip's) blends into the layer one stroke at a time, so repeated `copy` strokes converge on
/// the source colour at clip edges as they do in a CGContext. Premultiplied RGBA in 0..=255 floats.
struct Layer {
    w: usize,
    h: usize,
    data: Vec<f32>,
}

impl Layer {
    fn new(w: u32, h: u32) -> Self {
        Self { w: w as usize, h: h as usize, data: vec![0.0; w as usize * h as usize * 4] }
    }

    /// Strokes a y-up path (through `transform`) and calls `blend(pixel, coverage)` for every covered pixel.
    fn stroke(&mut self, path: &Path, stroke: &Stroke, transform: Transform, clip: &Mask, mut blend: impl FnMut(&mut [f32], f32)) {
        let Some(outline) = path.stroke(stroke, 1.0).and_then(|o| o.transform(transform)) else { return };
        let b = outline.bounds();
        let x0 = (b.left().floor() as i64).max(0) as usize;
        let y0 = (b.top().floor() as i64).max(0) as usize;
        let x1 = (b.right().ceil() as i64).clamp(0, self.w as i64) as usize;
        let y1 = (b.bottom().ceil() as i64).clamp(0, self.h as i64) as usize;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let Some(mut mask) = Mask::new((x1 - x0) as u32, (y1 - y0) as u32) else { return };
        mask.fill_path(&outline, FillRule::Winding, true, Transform::from_translate(-(x0 as f32), -(y0 as f32)));
        let (mw, cov, clip) = (x1 - x0, mask.data(), clip.data());
        for y in y0..y1 {
            for x in x0..x1 {
                let m = cov[(y - y0) * mw + (x - x0)];
                let c = clip[y * self.w + x];
                if m == 0 || c == 0 {
                    continue;
                }
                let k = m as f32 / 255.0 * c as f32 / 255.0;
                let i = (y * self.w + x) * 4;
                blend(&mut self.data[i..i + 4], k);
            }
        }
    }

    /// Source-over onto a canvas of the same size.
    fn draw_over(&self, out: &mut Pixmap) {
        for (d, s) in out.data_mut().chunks_exact_mut(4).zip(self.data.chunks_exact(4)) {
            let sa = s[3].clamp(0.0, 255.0);
            if sa <= 0.0 {
                continue;
            }
            let keep = 1.0 - sa / 255.0;
            let a = (sa + d[3] as f32 * keep).round().clamp(0.0, 255.0);
            for c in 0..3 {
                d[c] = (s[c].clamp(0.0, sa) + d[c] as f32 * keep).round().clamp(0.0, a) as u8;
            }
            d[3] = a as u8;
        }
    }
}
