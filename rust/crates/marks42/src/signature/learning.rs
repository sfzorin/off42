//! A signature learned from the user's own samples (port of Kit42 `SignatureSample`, `LearnedSignature` and
//! `SignatureLearning`).
//!
//! - Learned from pictures (scans, photos): every copy is made from the ink of one sample, bent the way a hand
//!   varies ([`super::warp`]); larger samples are used more often.
//! - Without pictures (pad strokes with timing, or strokes traced from pictures): copies are re-synthesised with the
//!   sigma-lognormal model. Via-points (Ramer–Douglas–Peucker) and their recorded timing come from the samples, two
//!   samples are blended when they have the same structure, and each copy perturbs the model.
//!
//! Pictures are traced into strokes like on macOS: ink against the local paper (Otsu threshold with hysteresis),
//! Zhang–Suen thinning, good-continuation tracing, crossings joined into pen paths and tips taken back from the ink.
//! Every random draw happens in the same order as in Swift, so the same samples and impression give the same copy.

// Negated float comparisons mirror the Swift guards (NaN rejected the same way).
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::needless_range_loop)]

use super::kinematics::{Hand, PenSample};
use super::{draw, warp, DrawOptions, Pen, MAXIMUM_SAMPLE_POINTS};
use crate::rng::SeededRandom;
use crate::texture::{self, pt, Pt};
use crate::Ink;
use image::{imageops, GrayImage, RgbaImage};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::f64::consts::PI;
use tiny_skia::Pixmap;

/// Most samples in one learned signature (Kit42 `SignatureLearning.maximumSamples`).
pub const MAXIMUM_SAMPLES: usize = 64;

/// One signature written by the user: strokes of `[x, y, time, pressure, band]` (y up; time in seconds, −1 when
/// unknown, as for a scan). Index 4 (pictures only) is the ink thickness around the centre line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignatureSample {
    pub strokes: Vec<Vec<Vec<f64>>>,
    /// Line width divided by max(1, traced ink height) in picture pixels (`None` for pad samples).
    pub pen_width: Option<f64>,
    /// The picture the sample was traced from (grey, at most 800 px tall); copies are made from its ink.
    pub picture: Option<GrayImage>,
}

impl SignatureSample {
    pub fn new(strokes: Vec<Vec<Vec<f64>>>) -> Self {
        Self { strokes, pen_width: None, picture: None }
    }

    /// A sample from a picture of one signature: traced, and the picture kept for copies made from its ink.
    /// `None` when no signature is found (no ink, or only specks).
    pub fn from_picture(image: &RgbaImage) -> Option<Self> {
        let mut sample = trace(image)?;
        sample.picture = Some(warp::encode(image)?);
        Some(sample)
    }

    pub fn is_timed(&self) -> bool {
        self.strokes.first().and_then(|s| s.first()).is_some_and(|p| p.len() > 2 && p[2] >= 0.0)
    }

    /// The same strokes without the picture: copies are then redrawn by the pen model.
    pub fn traced_only(&self) -> Self {
        Self { strokes: self.strokes.clone(), pen_width: self.pen_width, picture: None }
    }
}

/// A signature learned from the user's samples.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LearnedSignature {
    pub samples: Vec<SignatureSample>,
}

impl LearnedSignature {
    pub fn new(samples: Vec<SignatureSample>) -> Self {
        Self { samples }
    }

    /// One copy: trimmed straight-alpha RGBA. `pixel_height` is the capital height of a redrawn copy (a copy from
    /// the pictures' ink is 2.5 times as tall, as on macOS). `None` for unusable samples.
    pub fn render(&self, impression: u64, ink: Ink, pen: Pen, pixel_height: f64) -> Option<RgbaImage> {
        if self.samples.len() > MAXIMUM_SAMPLES {
            return None;
        }
        // All samples from pictures: copies from their ink.
        if !self.samples.is_empty() && self.samples.iter().all(|s| s.picture.is_some()) {
            let pictures: Vec<&GrayImage> = self.samples.iter().filter_map(|s| s.picture.as_ref()).collect();
            let mut pick = SeededRandom::new(impression);
            let weights: Vec<f64> = pictures.iter().map(|p| (p.height().min(800) as f64).powi(2)).collect();
            let mut r = (pick.next_u64() % 1_000_000) as f64 / 1_000_000.0 * weights.iter().sum::<f64>();
            let mut base = pictures.len() - 1;
            for (k, w) in weights.iter().enumerate() {
                if r < *w {
                    base = k;
                    break;
                }
                r -= w;
            }
            let picture = image::DynamicImage::ImageLuma8(pictures[base].clone()).to_rgba8();
            return warp::copy(&picture, ink, pick.next_u64(), pixel_height * 2.5);
        }
        self.redraw(impression, ink, pen, pixel_height).map(|(image, _)| texture::to_image(&image))
    }

    /// A copy redrawn by the pen model from the samples' strokes (what macOS does for pad samples, or when the
    /// pen's trajectories are captured), with the pen's trajectories in pixels.
    pub fn redraw(&self, impression: u64, ink: Ink, pen: Pen, pixel_height: f64) -> Option<(Pixmap, Vec<Vec<PenSample>>)> {
        if self.samples.len() > MAXIMUM_SAMPLES {
            return None;
        }
        let usable: Vec<&SignatureSample> = self.samples.iter().filter(|s| normalized_sample(s).is_some()).collect();
        let accepted: Vec<Normalized> = usable.iter().filter_map(|s| normalized_sample(s)).collect();
        let samples: Vec<&Vec<Vec<Point>>> = accepted.iter().map(|a| &a.strokes).collect();
        if samples.is_empty() {
            return None;
        }
        let mut copy = SeededRandom::new(impression);
        // The base of this copy: detailed samples (large, sharp pictures trace into more of the real movement) are
        // chosen more often. Detail = traced length per height × picture height of the centre line.
        let detail = |k: usize| -> f64 {
            let length: f64 = samples[k].iter().map(|s| path_length(s)).sum();
            let ys: Vec<f64> = usable[k].strokes.iter().flat_map(|s| s.iter().map(|p| p[1])).collect();
            let pixels = if usable[k].is_timed() {
                400.0
            } else {
                ys.iter().copied().reduce(f64::max).unwrap_or(0.0) - ys.iter().copied().reduce(f64::min).unwrap_or(0.0)
            };
            length * (pixels / 300.0).min(1.0)
        };
        let weights: Vec<f64> = (0..samples.len()).map(|k| detail(k).powi(2)).collect();
        let mut pick = (copy.next_u64() % 1_000_000) as f64 / 1_000_000.0 * weights.iter().sum::<f64>().max(1e-9);
        let mut ai = samples.len() - 1;
        for (k, w) in weights.iter().enumerate() {
            if pick < *w {
                ai = k;
                break;
            }
            pick -= w;
        }
        let a = samples[ai];
        let mut strokes: Vec<Vec<Point>> = a.clone();
        let partners: Vec<usize> = (0..samples.len()).filter(|&k| k != ai && samples[k].len() == a.len()).collect();
        if !partners.is_empty() {
            let b = samples[partners[swift_index(&mut copy, partners.len())]];
            if similar(a, b) {
                // Close to one of the two: a half-and-half mix smears features that sit at slightly different places.
                let w = copy.range(0.05, 0.3);
                strokes = a
                    .iter()
                    .zip(b.iter())
                    .map(|(sa, sb)| {
                        let n = sa.len().max(sb.len()).max(24);
                        resample(sa, n)
                            .iter()
                            .zip(resample(sb, n).iter())
                            .map(|(pa, pb)| Point {
                                p: pt(pa.p.x * (1.0 - w) + pb.p.x * w, pa.p.y * (1.0 - w) + pb.p.y * w),
                                t: if pa.t >= 0.0 && pb.t >= 0.0 { pa.t * (1.0 - w) + pb.t * w } else { pa.t },
                                band: pa.band,
                            })
                            .collect()
                    })
                    .collect();
            }
        }
        let mut via_strokes: Vec<Vec<Pt>> = Vec::new();
        let mut durations: Vec<Vec<f64>> = Vec::new();
        let mut any_timed = false;
        let own_width = accepted[ai].pen_width.unwrap_or(0.0);
        for s in &strokes {
            let v = via(&garland(s, own_width, &mut copy), 0.012);
            // Corners are marked for the synthesiser by repeating the point.
            let mut pts = Vec::new();
            for (k, p) in v.points.iter().enumerate() {
                pts.push(*p);
                if v.corners.contains(&k) {
                    pts.push(*p);
                }
            }
            via_strokes.push(pts);
            any_timed = any_timed || v.durations.is_some();
            durations.push(v.durations.unwrap_or_default());
        }
        let hand = Hand { overlap: if any_timed { 0.6 } else { 0.68 }, corner_overlap: 0.3, size_scaled: false, curvature_gain: 0.4, ..Hand::default() };
        // The line width of the user's own pen when it was measured on a picture (else the average, else a usual
        // ballpoint), scaled back for the renderer's ink spread.
        let widths: Vec<f64> = accepted.iter().filter_map(|a| a.pen_width).collect();
        let own = accepted[ai].pen_width.or(if widths.is_empty() { None } else { Some(widths.iter().sum::<f64>() / widths.len() as f64) });
        let pen_ratio = own.map(|w| (w * 0.79).clamp(0.012, 0.06)).unwrap_or(0.024);
        let n = samples.len();
        let options = DrawOptions {
            variation: if n > 2 { 0.3 } else if n > 1 { 0.5 } else { 0.85 },
            durations: if any_timed { Some(durations) } else { None },
            run_out: any_timed,
            smoothing: if any_timed { None } else { Some(0.012) },
            sharp_corners: true,
            merge: 0.002,
            layout: Some(1.0),
            stroke_shift: if n > 2 { 0.025 } else { 0.015 },
        };
        draw(&via_strokes, &hand, 0.0, pen_ratio, pen, ink, pixel_height, &mut copy, &options)
    }
}

/// Swift `Array.randomElement(using:)` index: `next(upperBound:)` with Lemire's method on 64 bits.
fn swift_index(rng: &mut SeededRandom, count: usize) -> usize {
    let upper = count as u64;
    let mut random = rng.next_u64();
    let mut m = random as u128 * upper as u128;
    if (m as u64) < upper {
        let t = 0u64.wrapping_sub(upper) % upper;
        while (m as u64) < t {
            random = rng.next_u64();
            m = random as u128 * upper as u128;
        }
    }
    (m >> 64) as usize
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Point {
    pub p: Pt,
    pub t: f64,
    pub band: f64,
}

pub(crate) struct Normalized {
    pub strokes: Vec<Vec<Point>>,
    pub pen_width: Option<f64>,
}

fn path_length(s: &[Point]) -> f64 {
    s.windows(2).map(|w| (w[1].p.x - w[0].p.x).hypot(w[1].p.y - w[0].p.y)).sum()
}

/// Normal cap height is 1. Very flat samples fit within 32 cap widths without changing their aspect ratio. The
/// measured pen width keeps the same scale. Invalid samples give `None`.
pub(crate) fn normalized_sample(s: &SignatureSample) -> Option<Normalized> {
    if s.strokes.len() > 1_024 {
        return None;
    }
    let mut count = 0;
    for stroke in &s.strokes {
        if stroke.len() > MAXIMUM_SAMPLE_POINTS - count {
            return None;
        }
        count += stroke.len();
        if !stroke.iter().all(|p| p.len() >= 2 && p.len() <= 5 && p.iter().all(|v| v.is_finite())) {
            return None;
        }
    }
    let all: Vec<&Vec<f64>> = s.strokes.iter().flatten().collect();
    let min_x = all.iter().map(|p| p[0]).reduce(f64::min)?;
    let max_x = all.iter().map(|p| p[0]).reduce(f64::max)?;
    let min_y = all.iter().map(|p| p[1]).reduce(f64::min)?;
    let max_y = all.iter().map(|p| p[1]).reduce(f64::max)?;
    let (width, height) = (max_x - min_x, max_y - min_y);
    if !(width.is_finite() && height.is_finite()) {
        return None;
    }
    let h = height.max(width / 32.0);
    if !(h.is_finite() && h > 0.0) {
        return None;
    }
    let mut pen_width = None;
    if let Some(measured) = s.pen_width {
        if !(measured.is_finite() && measured > 0.0) {
            return None;
        }
        // The measured-width range applies in the original units, before scaling.
        let scaled = 0.06f64.min(0.012f64.max(measured)) * (height.max(1.0) / h);
        if !(scaled.is_finite() && scaled > 0.0) {
            return None;
        }
        pen_width = Some(scaled);
    }
    let strokes: Vec<Vec<Point>> = s
        .strokes
        .iter()
        .map(|stroke| {
            stroke
                .iter()
                .map(|p| Point {
                    p: pt((p[0] - min_x) / h, (p[1] - min_y) / h),
                    t: if p.len() > 2 { p[2] } else { -1.0 },
                    band: if p.len() > 4 { p[4] / h } else { 0.0 },
                })
                .collect::<Vec<_>>()
        })
        .filter(|s| s.len() > 1)
        .collect();
    if strokes.is_empty() {
        None
    } else {
        Some(Normalized { strokes, pen_width })
    }
}

/// Even spacing along a stroke (`n` points), carrying time and band.
pub(crate) fn resample(s: &[Point], n: usize) -> Vec<Point> {
    if s.len() > MAXIMUM_SAMPLE_POINTS || n > MAXIMUM_SAMPLE_POINTS {
        return Vec::new();
    }
    if s.len() <= 1 || n <= 1 {
        return s.to_vec();
    }
    let mut lengths = vec![0.0];
    for i in 1..s.len() {
        lengths.push(lengths[i - 1] + (s[i].p.x - s[i - 1].p.x).hypot(s[i].p.y - s[i - 1].p.y));
    }
    let total = lengths[lengths.len() - 1].max(1e-6);
    let mut out = Vec::with_capacity(n);
    let mut j = 0;
    for k in 0..n {
        let target = total * k as f64 / (n - 1) as f64;
        while j < s.len() - 2 && lengths[j + 1] < target {
            j += 1;
        }
        let seg = (lengths[j + 1] - lengths[j]).max(1e-9);
        let u = (target - lengths[j]) / seg;
        out.push(Point {
            p: pt(s[j].p.x + (s[j + 1].p.x - s[j].p.x) * u, s[j].p.y + (s[j + 1].p.y - s[j].p.y) * u),
            t: s[j].t + (s[j + 1].t - s[j].t) * u,
            band: s[j].band + (s[j + 1].band - s[j].band) * u,
        });
    }
    out
}

pub(crate) struct Via {
    pub points: Vec<Pt>,
    pub corners: HashSet<usize>,
    pub durations: Option<Vec<f64>>,
}

fn signed_turn(a: f64, b: f64) -> f64 {
    let mut d = b - a;
    while d > PI {
        d -= 2.0 * PI;
    }
    while d < -PI {
        d += 2.0 * PI;
    }
    d
}

/// Via-points of a stroke: Ramer–Douglas–Peucker simplification plus the time spent between them when the stroke
/// was recorded. Sharp turns become corners; a tip that thinning rounded into a tight U is collapsed into one point.
pub(crate) fn via(stroke: &[Point], tolerance: f64) -> Via {
    let pts: Vec<Pt> = stroke.iter().map(|p| p.p).collect();
    if pts.len() <= 2 {
        return Via { points: pts, corners: HashSet::new(), durations: None };
    }
    let mut keep = vec![false; pts.len()];
    keep[0] = true;
    keep[pts.len() - 1] = true;
    let mut pending = vec![(0usize, pts.len() - 1)];
    while let Some((a, b)) = pending.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (pts[a], pts[b]);
        let (dx, dy) = (pb.x - pa.x, pb.y - pa.y);
        let len = dx.hypot(dy).max(1e-9);
        let (mut worst, mut dist) = (0usize, 0.0);
        let mut found = false;
        for i in (a + 1)..b {
            let d = ((pts[i].x - pa.x) * dy - (pts[i].y - pa.y) * dx).abs() / len;
            if d > dist {
                dist = d;
                worst = i;
                found = true;
            }
        }
        if dist > tolerance && found && worst > 0 {
            keep[worst] = true;
            pending.push((a, worst));
            pending.push((worst, b));
        }
    }
    let mut idx: Vec<usize> = (0..pts.len()).filter(|&i| keep[i]).collect();
    let heading = |a: usize, b: usize| (pts[b].y - pts[a].y).atan2(pts[b].x - pts[a].x);
    let mut k = 1;
    while k + 2 < idx.len() {
        let (p1, p2) = (pts[idx[k]], pts[idx[k + 1]]);
        let short = (p2.x - p1.x).hypot(p2.y - p1.y) < 0.04;
        let t1 = signed_turn(heading(idx[k - 1], idx[k]), heading(idx[k], idx[k + 1]));
        let t2 = signed_turn(heading(idx[k], idx[k + 1]), heading(idx[k + 1], idx[k + 2]));
        // A tip, not a small loop: on a loop the line keeps turning the same way before and after.
        let t0 = if k >= 2 { signed_turn(heading(idx[k - 2], idx[k - 1]), heading(idx[k - 1], idx[k])) } else { 0.0 };
        let t3 = if k + 3 < idx.len() { signed_turn(heading(idx[k + 1], idx[k + 2]), heading(idx[k + 2], idx[k + 3])) } else { 0.0 };
        let curling = (t0 * t1 > 0.0 && t0.abs() > 0.5) || (t3 * t2 > 0.0 && t3.abs() > 0.5);
        if short && !curling && t1 * t2 > 0.0 && (t1 + t2).abs() > 2.2 {
            // The tip: the point of the U farthest along the incoming direction.
            let dir = heading(idx[k - 1], idx[k]);
            let (c, s) = (dir.cos(), dir.sin());
            let mut tip = idx[k];
            for i in idx[k]..=idx[k + 1] {
                if pts[tip].x * c + pts[tip].y * s < pts[i].x * c + pts[i].y * s {
                    tip = i;
                }
            }
            idx.splice(k..=k + 1, [tip]);
        }
        k += 1;
    }
    let points: Vec<Pt> = idx.iter().map(|&i| pts[i]).collect();
    let timed = stroke.first().is_some_and(|p| p.t >= 0.0);
    let mut corners = HashSet::new();
    for k in 1..(points.len().saturating_sub(1)).max(1) {
        let a = (points[k].y - points[k - 1].y).atan2(points[k].x - points[k - 1].x);
        let b = (points[k + 1].y - points[k].y).atan2(points[k + 1].x - points[k].x);
        let mut d = (b - a).abs();
        if d > PI {
            d = 2.0 * PI - d;
        }
        // Cusp: more than ~115°.
        if d > 2.0 {
            corners.insert(k);
        }
    }
    let durations = timed.then(|| idx.windows(2).map(|w| (stroke[w[1]].t - stroke[w[0]].t).max(0.02) * 2.2).collect());
    Via { points, corners, durations }
}

/// Dense up-and-down strokes (a garland like «ини», «mm») merge into a solid band on a small photo. Where the traced
/// ink is much thicker than the pen over a stretch, the pen writes a garland again: humps across the band.
pub(crate) fn garland(s: &[Point], pen: f64, rng: &mut SeededRandom) -> Vec<Point> {
    if !(pen > 0.0 && s.len() > 3 && s.iter().any(|p| p.band > pen * 2.2)) {
        return s.to_vec();
    }
    let mut lengths = vec![0.0];
    for i in 1..s.len() {
        lengths.push(lengths[i - 1] + (s[i].p.x - s[i - 1].p.x).hypot(s[i].p.y - s[i - 1].p.y));
    }
    let thickest = s.iter().map(|p| p.band).fold(0.0, f64::max);
    let step = pen.max(thickest) / 6.0;
    let dense = resample(s, s.len().max((lengths[lengths.len() - 1] / step.max(1e-4)) as usize + 1));
    let mut out = dense.clone();
    let mut i = 0;
    while i < dense.len() {
        if !(dense[i].band > pen * 2.2) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < dense.len() && dense[j + 1].band > pen * 1.8 {
            j += 1;
        }
        let band = dense[i..=j].iter().map(|p| p.band).sum::<f64>() / (j - i + 1) as f64;
        let wave = (band * 1.5).max(pen * 4.0) * rng.range(0.9, 1.1);
        let run = (j - i) as f64 * step;
        if run > wave * 1.5 {
            let amp = (band - pen) / 2.0 * 0.9;
            for k in i..=j {
                let a = dense[i.max(k.saturating_sub(2))].p;
                let b = dense[j.min(k + 2)].p;
                let len = (b.x - a.x).hypot(b.y - a.y).max(1e-6);
                let (nx, ny) = (-(b.y - a.y) / len, (b.x - a.x) / len);
                let along = (k - i) as f64 * step;
                let taper = 1f64.min(along / (wave / 2.0)).min((run - along) / (wave / 2.0));
                let off = amp * taper.max(0.0) * (along / wave * 2.0 * PI).sin();
                out[k].p = pt(dense[k].p.x + nx * off, dense[k].p.y + ny * off);
            }
        }
        i = j + 1;
    }
    out
}

/// Same number of strokes with similar lengths: blending them gives a plausible in-between copy.
fn similar(a: &[Vec<Point>], b: &[Vec<Point>]) -> bool {
    a.iter().zip(b).all(|(sa, sb)| {
        let (la, lb) = (path_length(sa), path_length(sb));
        (la - lb).abs() / la.max(lb).max(1e-6) < 0.3 && (sa[0].p.x - sb[0].p.x).hypot(sa[0].p.y - sb[0].p.y) < 0.35
    })
}

// MARK: - Pictures: ink → centre lines → strokes

type P = (f64, f64);

fn dist(a: P, b: P) -> f64 {
    (a.0 - b.0).hypot(a.1 - b.1)
}

fn length(s: &[P]) -> f64 {
    s.windows(2).map(|w| dist(w[0], w[1])).sum()
}

fn unit(a: P, b: P) -> P {
    let l = dist(a, b).max(1e-9);
    ((b.0 - a.0) / l, (b.1 - a.1) / l)
}

/// Traces a picture of a signature into strokes (no timing). Dark or opaque pixels are ink.
pub fn trace(image: &RgbaImage) -> Option<SignatureSample> {
    let (iw, ih) = image.dimensions();
    if iw == 0 || ih == 0 {
        return None;
    }
    // Work at about 360 px height; small pictures are enlarged up to 4×.
    let k = 4f64.min(360.0 / ih.max(1) as f64).min(1600.0 / iw.max(1) as f64);
    let w = ((iw as f64 * k) as usize).max(8);
    let h = ((ih as f64 * k) as usize).max(8);
    let flat = warp::on_white(image);
    let src = if (w, h) == (iw as usize, ih as usize) { flat } else { imageops::resize(&flat, w as u32, h as u32, imageops::FilterType::Triangle) };
    let lum: Vec<f64> = src.pixels().map(|p| (p.0[0] as f64 * 3.0 + p.0[1] as f64 * 6.0 + p.0[2] as f64) / 10.0).collect();
    let mut m = ink_mask(&lum, w, h);
    // Line width from clearly inked pixels only (the faint hysteresis fringe would widen every line).
    let dark = warp::darkness(&lum, w, h);
    let strong = warp::ink_threshold(&dark);
    let ink_area = dark.iter().filter(|&&d| d > strong).count();
    // Ink thickness around every pixel (distance to paper, both ways).
    let mut depth: Vec<f64> = m.iter().map(|&v| if v == 1 { f64::INFINITY } else { 0.0 }).collect();
    for y in 0..h {
        for x in 0..w {
            if !(depth[y * w + x] > 0.0) {
                continue;
            }
            let mut v = depth[y * w + x];
            if x > 0 {
                v = v.min(depth[y * w + x - 1] + 1.0);
            }
            if y > 0 {
                v = v.min(depth[(y - 1) * w + x] + 1.0);
            }
            if x > 0 && y > 0 {
                v = v.min(depth[(y - 1) * w + x - 1] + 1.414);
            }
            if x < w - 1 && y > 0 {
                v = v.min(depth[(y - 1) * w + x + 1] + 1.414);
            }
            depth[y * w + x] = v;
        }
    }
    for y in (0..h).rev() {
        for x in (0..w).rev() {
            if !(depth[y * w + x] > 0.0) {
                continue;
            }
            let mut v = depth[y * w + x];
            if x < w - 1 {
                v = v.min(depth[y * w + x + 1] + 1.0);
            }
            if y < h - 1 {
                v = v.min(depth[(y + 1) * w + x] + 1.0);
            }
            if x < w - 1 && y < h - 1 {
                v = v.min(depth[(y + 1) * w + x + 1] + 1.414);
            }
            if x > 0 && y < h - 1 {
                v = v.min(depth[(y + 1) * w + x - 1] + 1.414);
            }
            depth[y * w + x] = v;
        }
    }
    let solid = m.clone();
    thin(&mut m, w, h);
    let centre_line = m.iter().map(|&v| v as usize).sum::<usize>().max(1);
    let width = ink_area as f64 / centre_line as f64;
    let all = trace_paths(&m, w, h);
    let attached = |p: P, i: usize| all.iter().enumerate().any(|(j, s)| j != i && s.iter().any(|q| dist(*q, p) < 3f64.max(width * 1.2)));
    // Spurs (short pieces with a free end) are dropped; a short piece between two crossings is part of a letter.
    let paths: Vec<Vec<P>> = all
        .iter()
        .enumerate()
        .filter(|(i, p)| p.len() as f64 >= 8f64.max(width * 1.6) || (p.len() >= 3 && attached(p[0], *i) && attached(p[p.len() - 1], *i)))
        .map(|(_, p)| p.clone())
        .collect();
    if paths.is_empty() {
        return None;
    }
    let ys = paths.iter().flatten().map(|p| p.1);
    let (lo, hi) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), y| (lo.min(y), hi.max(y)));
    let height = (hi - lo).max(1.0);
    let assembled = detached(joined(&paths, (width * 1.8).max(4.0), height, width), height);
    let mut strokes: Vec<Vec<Vec<f64>>> = sharpened(&assembled, &solid, w, h, width)
        .iter()
        .map(|s| {
            s.iter()
                .map(|p| {
                    let x = (p.0.round() as i64).clamp(0, w as i64 - 1) as usize;
                    let y = (p.1.round() as i64).clamp(0, h as i64 - 1) as usize;
                    vec![p.0, (h - 1) as f64 - p.1, -1.0, -1.0, (h as f64).min(2.0 * depth[y * w + x])]
                })
                .collect()
        })
        .collect();
    // Rows in memory run top-down: flipped to y up above. Strokes left to right by their start.
    strokes.sort_by(|a, b| {
        let (x, y) = (a.first().map_or(0.0, |p| p[0]), b.first().map_or(0.0, |p| p[0]));
        x.partial_cmp(&y).unwrap_or(std::cmp::Ordering::Equal)
    });
    Some(SignatureSample { strokes, pen_width: Some(width / height), picture: None })
}

fn find(parent: &mut [usize], mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

fn union(parent: &mut [usize], a: usize, b: usize) {
    let (ra, rb) = (find(parent, a), find(parent, b));
    parent[ra] = rb;
}

/// Pieces of one movement, cut apart where the centre line turns sharply or touches itself, joined back into pen
/// strokes: ends that meet are one pen path. Each stroke is then written left to right.
pub(crate) fn joined(paths: &[Vec<P>], gap: f64, height: f64, width: f64) -> Vec<Vec<P>> {
    let mut pieces: Vec<Vec<P>> = paths.iter().filter(|p| p.len() > 1).cloned().collect();
    // T-junctions: a piece that ends on the middle of another; split the other one there.
    let mut split = true;
    while split {
        split = false;
        'outer: for i in 0..pieces.len() {
            for end in [pieces[i][0], pieces[i][pieces[i].len() - 1]] {
                for j in 0..pieces.len() {
                    if j == i || pieces[j].len() <= 8 {
                        continue;
                    }
                    let s = &pieces[j];
                    let mut k = 4;
                    for q in 5..s.len() - 4 {
                        if dist(s[q], end) < dist(s[k], end) {
                            k = q;
                        }
                    }
                    if !(dist(s[k], end) < gap && dist(s[0], end) > gap && dist(s[s.len() - 1], end) > gap) {
                        continue;
                    }
                    let (head, tail) = (s[..=k].to_vec(), s[k..].to_vec());
                    pieces[j] = head;
                    pieces.push(tail);
                    split = true;
                    break 'outer;
                }
            }
        }
    }
    // Spurs: a short piece with one free end hanging off another line. Free at both ends (a dot, a dash) stays.
    {
        let near = |p: P, i: usize| pieces.iter().enumerate().any(|(j, s)| j != i && s.iter().any(|q| dist(*q, p) < gap));
        pieces = pieces
            .iter()
            .enumerate()
            .filter(|(i, s)| !(length(s) < height * 0.2) || near(s[0], *i) == near(s[s.len() - 1], *i))
            .map(|(_, s)| s.clone())
            .collect();
    }
    let n = pieces.len();
    let point = |e: usize| -> P {
        let s = &pieces[e / 2];
        if e.is_multiple_of(2) {
            s[0]
        } else {
            s[s.len() - 1]
        }
    };
    let mut parent: Vec<usize> = (0..n * 2).collect();
    for a in 0..n * 2 {
        for b in (a + 1)..n * 2 {
            if a / 2 != b / 2 && dist(point(a), point(b)) < gap {
                union(&mut parent, a, b);
            }
        }
    }
    // A short piece between two nodes is the crossing itself: the nodes become one and the bridge is left out.
    let mut removed = HashSet::new();
    for k in 0..n {
        if !(length(&pieces[k]) < gap * 1.6) {
            continue;
        }
        let a = find(&mut parent, k * 2);
        let b = find(&mut parent, k * 2 + 1);
        let others = |parent: &mut Vec<usize>, node: usize| (0..n * 2).any(|e| e / 2 != k && find(parent, e) == node);
        if a != b && others(&mut parent, a) && others(&mut parent, b) {
            union(&mut parent, a, b);
            removed.insert(k);
        }
    }
    // Direction the pen moves arriving at end `e` (from inside the piece out through that end).
    let arriving = |e: usize| -> P {
        let s = &pieces[e / 2];
        let m = (s.len() - 1).min(2usize.max((gap * 1.5) as usize));
        let (p, q) = if e.is_multiple_of(2) { (s[0], s[m]) } else { (s[s.len() - 1], s[s.len() - 1 - m]) };
        let len = dist(p, q).max(1e-9);
        ((p.0 - q.0) / len, (p.1 - q.1) / len)
    };
    let turn = |x: usize, y: usize| -> f64 {
        let (a, b) = (arriving(x), arriving(y));
        (a.0 * -b.0 + a.1 * -b.1).clamp(-1.0, 1.0).acos()
    };
    let short = height * 0.25;
    // At every node, pair the ends the way a pen passes: straight through, back from a pointed tip, and a sharper
    // turn only into a short piece. Best pairs first.
    let mut partner: HashMap<usize, usize> = HashMap::new();
    let mut nodes: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for e in 0..n * 2 {
        if !removed.contains(&(e / 2)) {
            let root = find(&mut parent, e);
            nodes.entry(root).or_default().push(e);
        }
    }
    for ends in nodes.values().filter(|ends| ends.len() > 1) {
        let mut options: Vec<(f64, usize, usize)> = Vec::new();
        for x in 0..ends.len() {
            for y in (x + 1)..ends.len() {
                if ends[x] / 2 == ends[y] / 2 {
                    continue;
                }
                let t = turn(ends[x], ends[y]);
                let score = if t < PI / 3.0 {
                    t / PI
                } else if t > PI * 5.0 / 6.0 {
                    0.5
                } else if length(&pieces[ends[x] / 2]).min(length(&pieces[ends[y] / 2])) < short {
                    1.0 + t / PI
                } else {
                    continue;
                };
                options.push((score, ends[x], ends[y]));
            }
        }
        options.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        for (_, x, y) in options {
            if !partner.contains_key(&x) && !partner.contains_key(&y) {
                partner.insert(x, y);
                partner.insert(y, x);
            }
        }
    }
    // Loops that leave and return to one crossing are spliced into the stroke passing through it.
    for k in 0..n {
        if removed.contains(&k)
            || find(&mut parent, k * 2) != find(&mut parent, k * 2 + 1)
            || partner.contains_key(&(k * 2))
            || partner.contains_key(&(k * 2 + 1))
        {
            continue;
        }
        let node = find(&mut parent, k * 2);
        let here: Vec<usize> = nodes.get(&node).map(|v| v.iter().copied().filter(|e| e / 2 != k).collect()).unwrap_or_default();
        let mut best: Option<(f64, usize, Option<usize>, usize, usize)> = None;
        for &x in &here {
            let y = partner.get(&x).copied();
            for (a, b) in [(k * 2, k * 2 + 1), (k * 2 + 1, k * 2)] {
                let cost = turn(x, a) + y.map(|y| turn(b, y)).unwrap_or(0.0);
                let eligible = y.is_some() || here.iter().any(|e| !partner.contains_key(e) && *e == x);
                if eligible && cost < best.map_or(f64::INFINITY, |b| b.0) {
                    best = Some((cost, x, y, a, b));
                }
            }
        }
        if let Some((_, x, y, a, b)) = best {
            partner.insert(x, a);
            partner.insert(a, x);
            if let Some(y) = y {
                partner.insert(b, y);
                partner.insert(y, b);
            }
        }
    }
    // Walk the pairings into pen strokes.
    let mut used: HashSet<usize> = removed.clone();
    let mut strokes: Vec<Vec<P>> = Vec::new();
    let walk = |start: usize, used: &mut HashSet<usize>| -> Vec<P> {
        let mut path: Vec<P> = Vec::new();
        let mut e = start;
        while !used.contains(&(e / 2)) {
            used.insert(e / 2);
            let s = &pieces[e / 2];
            if e.is_multiple_of(2) {
                path.extend_from_slice(s);
            } else {
                path.extend(s.iter().rev());
            }
            let exit = if e.is_multiple_of(2) { e + 1 } else { e - 1 };
            match partner.get(&exit) {
                Some(&next) if !used.contains(&(next / 2)) => e = next,
                _ => break,
            }
        }
        path
    };
    // Strokes start at free ends, then any remaining closed loops.
    for e in 0..n * 2 {
        if !used.contains(&(e / 2)) && !partner.contains_key(&e) {
            strokes.push(walk(e, &mut used));
        }
    }
    for k in 0..n {
        if !used.contains(&k) {
            strokes.push(walk(k * 2, &mut used));
        }
    }
    strokes.retain(|s| s.len() > 1);
    // Fragments left over at crossings: short and lying on another stroke.
    let contact = 3f64.max(width * 1.5);
    strokes = strokes
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            if !(length(s) < height * 0.12) {
                return true;
            }
            !s.iter().any(|p| strokes.iter().enumerate().any(|(k, o)| k != *i && o.iter().any(|q| dist(*q, *p) < contact)))
        })
        .map(|(_, s)| s.clone())
        .collect();
    // An end that meets another line stops there.
    let touch = 2f64.max(width * 0.6);
    for i in 0..strokes.len() {
        for at_end in [false, true] {
            let mut s = if at_end { strokes[i].clone() } else { strokes[i].iter().rev().copied().collect() };
            let reach = (s.len() as i64 - 2).min(3i64.max((gap * 1.5) as i64));
            if reach <= 2 {
                continue;
            }
            let mut cut = None;
            for k in (s.len() - reach as usize)..s.len() {
                if strokes.iter().enumerate().any(|(j, o)| j != i && o.iter().any(|q| dist(*q, s[k]) < touch)) {
                    cut = Some(k);
                    break;
                }
            }
            if let Some(c) = cut.filter(|&c| c > 1) {
                s.truncate(c + 1);
                strokes[i] = if at_end { s } else { s.into_iter().rev().collect() };
            }
        }
    }
    strokes
        .into_iter()
        .map(|s| if s[s.len() - 1].0 < s[0].0 - 1.0 { s.into_iter().rev().collect() } else { s })
        .collect()
}

/// Tips and stroke ends taken back from the ink: free ends are extended to the edge of the ink (less half the pen
/// width), and reversals get their apex moved out along the bisector with straight arms.
pub(crate) fn sharpened(strokes: &[Vec<P>], solid: &[u8], w: usize, h: usize, width: f64) -> Vec<Vec<P>> {
    let inside = |p: P| {
        let (x, y) = (p.0.round() as i64, p.1.round() as i64);
        x >= 0 && y >= 0 && x < w as i64 && y < h as i64 && solid[y as usize * w + x as usize] == 1
    };
    let edge = |p: P, u: P, limit: f64| {
        let mut t = 0.0;
        while t < limit && inside((p.0 + u.0 * (t + 0.5), p.1 + u.1 * (t + 0.5))) {
            t += 0.5;
        }
        t
    };
    let half = width / 2.0;
    strokes
        .iter()
        .map(|stroke| {
            let mut s = stroke.clone();
            if s.len() <= 4 {
                return s;
            }
            // Free ends.
            let back = (s.len() - 1).min(2usize.max((width * 1.5) as usize));
            let end_dir = unit(s[s.len() - 1 - back], s[s.len() - 1]);
            let end_reach = edge(s[s.len() - 1], end_dir, width * 3.0) - half;
            if end_reach > 0.5 {
                let e = s[s.len() - 1];
                for k in 1..=(end_reach.ceil() as usize) {
                    let t = end_reach.min(k as f64);
                    s.push((e.0 + end_dir.0 * t, e.1 + end_dir.1 * t));
                }
            }
            let start_dir = unit(s[back], s[0]);
            let start_reach = edge(s[0], start_dir, width * 3.0) - half;
            if start_reach > 0.5 {
                let b = s[0];
                let lead: Vec<P> = (1..=(start_reach.ceil() as usize))
                    .rev()
                    .map(|k| {
                        let t = start_reach.min(k as f64);
                        (b.0 + start_dir.0 * t, b.1 + start_dir.1 * t)
                    })
                    .collect();
                s.splice(0..0, lead);
            }
            // Reversals.
            let arm = 3usize.max((width * 2.0) as usize);
            let mut i = arm;
            while i + arm < s.len() {
                let (a, c, p) = (s[i - arm], s[i + arm], s[i]);
                let (u1, u2) = (unit(a, p), unit(p, c));
                let turn = (u1.0 * u2.0 + u1.1 * u2.1).clamp(-1.0, 1.0).acos();
                if !(turn > 2.1) {
                    i += 1;
                    continue;
                }
                // The sharpest point of this bend: farthest from the chord between the arms.
                let mid = ((a.0 + c.0) / 2.0, (a.1 + c.1) / 2.0);
                let mut apex = i - arm + 1;
                for j in (i - arm + 2)..=(i + arm - 1) {
                    if dist(s[apex], mid) < dist(s[j], mid) {
                        apex = j;
                    }
                }
                let q = s[apex];
                let (ua, uc) = (unit(q, a), unit(q, c));
                let out = unit((ua.0 + uc.0, ua.1 + uc.1), (0.0, 0.0));
                let reach = edge(q, out, width * 3.0) - half;
                if reach > 0.5 {
                    let tip = (q.0 + out.0 * reach, q.1 + out.1 * reach);
                    let lo = apex.saturating_sub(arm);
                    let hi = (s.len() - 1).min(apex + arm);
                    let (n1, n2) = ((apex - lo).max(1), (hi - apex).max(1));
                    let mut run: Vec<P> = Vec::new();
                    for k in 0..n1 {
                        let t = k as f64 / n1 as f64;
                        run.push((s[lo].0 + (tip.0 - s[lo].0) * t, s[lo].1 + (tip.1 - s[lo].1) * t));
                    }
                    for k in 0..=n2 {
                        let t = k as f64 / n2 as f64;
                        run.push((tip.0 + (s[hi].0 - tip.0) * t, tip.1 + (s[hi].1 - tip.1) * t));
                    }
                    let count = run.len();
                    s.splice(lo..=hi, run);
                    i = lo + count + arm;
                } else {
                    i = apex + arm;
                }
            }
            s
        })
        .collect()
}

/// Without specks: short pieces lying apart from the rest of the signature. A short piece close to other strokes
/// (an i-dot, a bar) stays.
pub(crate) fn detached(strokes: Vec<Vec<P>>, height: f64) -> Vec<Vec<P>> {
    let longest = strokes.iter().map(|s| length(s)).fold(0.0, f64::max);
    strokes
        .iter()
        .enumerate()
        .filter(|(i, s)| {
            if !(length(s) < longest * 0.2) {
                return true;
            }
            let mut gap = f64::INFINITY;
            for (k, other) in strokes.iter().enumerate() {
                if k == *i {
                    continue;
                }
                for q in other {
                    let d = s.iter().map(|p| dist(*p, *q)).fold(f64::INFINITY, f64::min);
                    gap = gap.min(d);
                }
            }
            gap < height * 0.3
        })
        .map(|(_, s)| s.clone())
        .collect()
}

/// Ink pixels of a photo or scan: darkness against the local paper, Otsu's threshold, hysteresis for faint fast
/// strokes (never filling the insides of small loops), and a closing for a faint, thin line.
pub(crate) fn ink_mask(lum: &[f64], w: usize, h: usize) -> Vec<u8> {
    let dark = warp::darkness(lum, w, h);
    let t = warp::ink_threshold(&dark);
    let mut m: Vec<u8> = dark.iter().map(|&d| u8::from(d > t)).collect();
    // The insides of small loops as clear ink encloses them.
    let mut holes = vec![false; w * h];
    {
        let mut outside = vec![false; w * h];
        let mut stack: Vec<usize> = Vec::new();
        for x in 0..w {
            for y in [0, h - 1] {
                if m[y * w + x] == 0 {
                    stack.push(y * w + x);
                }
            }
        }
        for y in 0..h {
            for x in [0, w - 1] {
                if m[y * w + x] == 0 {
                    stack.push(y * w + x);
                }
            }
        }
        for &i in &stack {
            outside[i] = true;
        }
        while let Some(i) = stack.pop() {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
                if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                    let j = ny as usize * w + nx as usize;
                    if m[j] == 0 && !outside[j] {
                        outside[j] = true;
                        stack.push(j);
                    }
                }
            }
        }
        for i in 0..w * h {
            if m[i] == 0 && !outside[i] {
                holes[i] = true;
            }
        }
    }
    let low = 12f64.max(t * 0.65);
    let mut stack: Vec<usize> = (0..m.len()).filter(|&i| m[i] == 1).collect();
    while let Some(i) = stack.pop() {
        let (x, y) = ((i % w) as i64, (i / w) as i64);
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let (nx, ny) = (x + dx, y + dy);
                if !(nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64) {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if m[j] == 0 && !holes[j] && dark[j] > low {
                    m[j] = 1;
                    stack.push(j);
                }
            }
        }
    }
    let shift = |src: &[u8], grow: bool| -> Vec<u8> {
        let mut out = src.to_vec();
        for y in 0..h as i64 {
            for x in 0..w as i64 {
                let mut v = src[y as usize * w + x as usize];
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, ny) = (x + dx, y + dy);
                    if !(nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64) {
                        if !grow {
                            v = 0;
                        }
                        continue;
                    }
                    let n = src[ny as usize * w + nx as usize];
                    v = if grow { v.max(n) } else { v.min(n) };
                }
                out[y as usize * w + x as usize] = v;
            }
        }
        out
    };
    // Closing only for a faint, thin line: on a normal line it would merge strokes that run close together.
    let share = m.iter().map(|&v| v as usize).sum::<usize>() as f64 / (w * h).max(1) as f64;
    if share < 0.012 {
        m = shift(&shift(&m, true), false);
    }
    for i in 0..w * h {
        if holes[i] {
            m[i] = 0;
        }
    }
    m
}

/// Zhang–Suen thinning to one-pixel centre lines.
fn thin(m: &mut [u8], w: usize, h: usize) {
    let mut changed = true;
    let mut remove: Vec<usize> = Vec::new();
    while changed {
        changed = false;
        for step in 0..2 {
            remove.clear();
            for y in 1..h - 1 {
                let row = y * w;
                for x in 1..w - 1 {
                    if m[row + x] != 1 {
                        continue;
                    }
                    let (p2, p3, p4, p5) = (m[row - w + x], m[row - w + x + 1], m[row + x + 1], m[row + w + x + 1]);
                    let (p6, p7, p8, p9) = (m[row + w + x], m[row + w + x - 1], m[row + x - 1], m[row - w + x - 1]);
                    let b = [p2, p3, p4, p5, p6, p7, p8, p9].iter().map(|&v| v as u32).sum::<u32>();
                    if !(2..=6).contains(&b) {
                        continue;
                    }
                    let seq = [p2, p3, p4, p5, p6, p7, p8, p9, p2];
                    let a = (0..8).filter(|&k| seq[k] == 0 && seq[k + 1] == 1).count();
                    if a != 1 {
                        continue;
                    }
                    let keep = if step == 0 { p2 * p4 * p6 != 0 || p4 * p6 * p8 != 0 } else { p2 * p4 * p8 != 0 || p2 * p6 * p8 != 0 };
                    if keep {
                        continue;
                    }
                    remove.push(row + x);
                }
            }
            if !remove.is_empty() {
                changed = true;
            }
            for &i in &remove {
                m[i] = 0;
            }
        }
    }
}

/// Walks the centre-line graph into paths: from the leftmost free end, always continuing in the straightest
/// direction at crossings (good continuation), until every pixel is used; closed loops are picked up too.
fn trace_paths(m: &[u8], w: usize, h: usize) -> Vec<Vec<P>> {
    let mut used = vec![false; w * h];
    const OFFSETS: [(i64, i64); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];
    let on = |x: i64, y: i64| x >= 0 && y >= 0 && x < w as i64 && y < h as i64 && m[y as usize * w + x as usize] == 1;
    let neighbours = |x: i64, y: i64| -> Vec<(i64, i64)> { OFFSETS.iter().filter(|o| on(x + o.0, y + o.1)).map(|o| (x + o.0, y + o.1)).collect() };
    let mut ends: Vec<(i64, i64)> = Vec::new();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if m[y as usize * w + x as usize] == 1 && neighbours(x, y).len() == 1 {
                ends.push((x, y));
            }
        }
    }
    ends.sort_by_key(|e| e.0);
    let mut paths: Vec<Vec<(i64, i64)>> = Vec::new();
    let mut walk = |start: (i64, i64), used: &mut Vec<bool>| {
        let mut path = vec![start];
        used[start.1 as usize * w + start.0 as usize] = true;
        let mut cur = start;
        let mut dir = (1.0f64, 0.0f64);
        loop {
            let free: Vec<(i64, i64)> = neighbours(cur.0, cur.1).into_iter().filter(|n| !used[n.1 as usize * w + n.0 as usize]).collect();
            if free.is_empty() {
                break;
            }
            // Prefer the step closest to the current direction (looking back a few pixels).
            let back = path[path.len().saturating_sub(6)];
            let (dx, dy) = ((cur.0 - back.0) as f64, (cur.1 - back.1) as f64);
            let dl = (dx * dx + dy * dy).sqrt().max(1e-6);
            if path.len() > 2 {
                dir = (dx / dl, dy / dl);
            }
            let score = |n: (i64, i64)| {
                let (ax, ay) = ((n.0 - cur.0) as f64, (n.1 - cur.1) as f64);
                (ax * dir.0 + ay * dir.1) / (ax * ax + ay * ay).sqrt()
            };
            let mut next = free[0];
            for &f in &free[1..] {
                if score(next) < score(f) {
                    next = f;
                }
            }
            used[next.1 as usize * w + next.0 as usize] = true;
            path.push(next);
            cur = next;
        }
        if path.len() > 1 {
            paths.push(path);
        }
    };
    for e in ends {
        if !used[e.1 as usize * w + e.0 as usize] {
            walk(e, &mut used);
        }
    }
    // Closed loops (an "o" with no free end).
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            if m[y as usize * w + x as usize] == 1 && !used[y as usize * w + x as usize] {
                walk((x, y), &mut used);
            }
        }
    }
    // Smooth pixel steps a little (5-point average) so via-points follow the line, not the pixel grid.
    paths
        .iter()
        .map(|p| {
            (0..p.len())
                .map(|i| {
                    let r = i.saturating_sub(2)..=(p.len() - 1).min(i + 2);
                    let n = (r.end() - r.start() + 1) as f64;
                    let sx: i64 = p[r.clone()].iter().map(|q| q.0).sum();
                    let sy: i64 = p[r].iter().map(|q| q.1).sum();
                    (sx as f64 / n, sy as f64 / n)
                })
                .collect()
        })
        .collect()
}

#[cfg(test)]
#[path = "learning_tests.rs"]
mod tests;
