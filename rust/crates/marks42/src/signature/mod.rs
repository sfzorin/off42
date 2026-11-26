//! Generated handwritten signatures (port of Kit42 `SignatureGenerator`, `PenAlphabet`, `Kinematics`, `PenRenderer`,
//! `SignatureWarp`).
//!
//! Signatures are written with a pen by a kinematic model of the hand (no fonts). Letters are hand-drawn centre
//! lines ([`alphabet`]): the pen moves through their via-points on a smooth spline. Speed along the path sets the
//! line: fast parts are slightly thinner and lighter, and every stroke starts and ends thin and faint because the pen
//! lands and lifts while moving.
//!
//! A **style** (1320 of them) combines a plan (what is written: full name, initial and surname, surname, initials and
//! a scribble, a monogram…), a flourish and how the initial joins the surname; the **design** seed also fixes slant,
//! proportions, garland or arcade, loops and rhythm. The **impression** seed varies each copy.
//!
//! Geometry stays in CoreGraphics y-up space and is drawn through `texture::cg`, so the same design and impression
//! seeds give the same signature as on macOS.

// 6.28 and negated float comparisons mirror the Swift source (same draws, NaN rejected like its guards).
#![allow(clippy::approx_constant, clippy::neg_cmp_op_on_partial_ord)]

mod alphabet;
mod kinematics;
pub mod learning;
mod pen;
pub mod warp;

pub use kinematics::PenSample;
pub use learning::{LearnedSignature, SignatureSample};
pub use pen::Pen;

use crate::rng::SeededRandom;
use crate::texture::{self, pt, Pt};
use crate::Ink;
use alphabet::Mark;
use kinematics::Hand;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::f64::consts::PI;
use tiny_skia::Pixmap;

/// Bound work before spatial/timing resampling, not only when allocating the final bitmap.
pub const MAXIMUM_RENDER_SAMPLES: usize = 100_000;
/// Largest capital height in pixels: 130 px (the preview) at up to 8× for insertion.
pub const MAXIMUM_PIXEL_HEIGHT: f64 = 130.0 * 8.0;
/// Most via-points accepted in one signature (Kit42 `SignatureLearning.maximumSamplePoints`).
pub const MAXIMUM_SAMPLE_POINTS: usize = 32_768;
/// Capital height of the usual preview in pixels.
pub const DEFAULT_PIXEL_HEIGHT: f64 = 130.0;

/// How much of the name a signature shows (Kit42 `SignatureForm`, same raw values).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SignatureForm {
    #[default]
    Any,
    FullName,
    InitialSurname,
    Surname,
    InitialsScribble,
    CapitalScribble,
}

impl SignatureForm {
    pub const ALL: [SignatureForm; 6] = [
        Self::Any,
        Self::FullName,
        Self::InitialSurname,
        Self::Surname,
        Self::InitialsScribble,
        Self::CapitalScribble,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Any => "Any form",
            Self::FullName => "Full name",
            Self::InitialSurname => "Initial and surname",
            Self::Surname => "Surname",
            Self::InitialsScribble => "Initials and scribble",
            Self::CapitalScribble => "Capital and scribble",
        }
    }

    pub fn raw_value(self) -> u8 {
        self as u8
    }

    pub fn from_raw_value(raw: u8) -> Option<Self> {
        Self::ALL.get(raw as usize).copied()
    }

    fn archetypes(self) -> &'static [Archetype] {
        use Archetype::*;
        match self {
            Self::Any => &Archetype::ALL,
            Self::FullName => &[Full, FullFading],
            Self::InitialSurname => &[InitialSurname, InitialSurnameFading, Enclosed],
            Self::Surname => &[Surname, SurnameFading],
            Self::InitialsScribble => &[InitialsScribble, Monogram],
            Self::CapitalScribble => &[CapitalScribble],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Archetype {
    Full,
    FullFading,
    InitialSurname,
    InitialSurnameFading,
    Surname,
    SurnameFading,
    InitialsScribble,
    CapitalScribble,
    Monogram,
    Enclosed,
}

impl Archetype {
    const ALL: [Archetype; 10] = [
        Self::Full,
        Self::FullFading,
        Self::InitialSurname,
        Self::InitialSurnameFading,
        Self::Surname,
        Self::SurnameFading,
        Self::InitialsScribble,
        Self::CapitalScribble,
        Self::Monogram,
        Self::Enclosed,
    ];
}

const FLOURISHES: usize = 11;
const JOINS: usize = 3;
const CAPITAL_VARIANTS: usize = 4;
/// Number of distinct styles (archetype × flourish × join × capital form).
pub const STYLE_COUNT: usize = Archetype::ALL.len() * FLOURISHES * JOINS * CAPITAL_VARIANTS;

fn style_index(a: Archetype, flourish: usize, join: usize, capital: usize) -> usize {
    ((a as usize * FLOURISHES + flourish) * CAPITAL_VARIANTS + capital) * JOINS + join
}

/// Design seeds for `count` styles of the form, with different plans where possible (the three proposals of the
/// generator window; "Regenerate" asks again). Fresh randomness each call.
pub fn design_seeds(count: usize, form: SignatureForm) -> Vec<u64> {
    design_seeds_with(count, form, &mut SeededRandom::new(SeededRandom::fresh_seed()))
}

/// [`design_seeds`] drawing from the given generator (deterministic for tests).
pub fn design_seeds_with(count: usize, form: SignatureForm, rng: &mut SeededRandom) -> Vec<u64> {
    let archetypes = form.archetypes();
    let shuffled = |rng: &mut SeededRandom| {
        let mut v = archetypes.to_vec();
        for i in (1..v.len()).rev() {
            let j = (rng.next_u64() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        v
    };
    let mut plans = shuffled(rng);
    while plans.len() < count {
        plans.extend(shuffled(rng));
    }
    plans
        .into_iter()
        .take(count)
        .map(|a| {
            let style = style_index(
                a,
                (rng.next_u64() % FLOURISHES as u64) as usize,
                (rng.next_u64() % JOINS as u64) as usize,
                (rng.next_u64() % CAPITAL_VARIANTS as u64) as usize,
            );
            let seed = 1 + rng.next_u64() % (u64::MAX >> 16);
            (seed << 16) | style as u64
        })
        .collect()
}

/// A fixed design seed (sample sheets): the `variant`-th style of the form.
pub fn design(form: SignatureForm, variant: usize, seed: u64) -> u64 {
    let plans = form.archetypes();
    let style = style_index(plans[variant % plans.len()], (variant * 7 + 3) % FLOURISHES, variant % JOINS, (variant / 2) % CAPITAL_VARIANTS);
    (seed << 16) | style as u64
}

/// Style number (1-based) of a design seed.
pub fn style_number(design: u64) -> usize {
    (design & 0xFFFF) as usize % STYLE_COUNT + 1
}

// MARK: - Plan

#[derive(Clone, Debug)]
struct Style {
    archetype: Archetype,
    /// 0 none, 1 underline →, 2 return under ←, 3 rising tail, 4 loop around capital, 5 strike through capital,
    /// 6 double dash, 7 spiral end, 8 swoop, 9 zigzag, 10 hook, 99 loop around everything.
    flourish: usize,
    /// 0 initial joined, 1 pen lifted, 2 initial with a dot.
    join: usize,
    /// 0 as drawn, 1 lead-in loop, 2 simplified (fast), 3 tall.
    capital: usize,
    slant: f64,
    xh: f64,
    initial_scale: f64,
    cap_w: f64,
    letter_w: f64,
    decay: f64,
    legible_decay: f64,
    garland: bool,
    loops: bool,
    rise: f64,
    entry: bool,
    exit_length: f64,
    exit_lift: f64,
    pen: f64,
    keep: usize,
    legible: usize,
    wave_h: Vec<f64>,
    wave_w: Vec<f64>,
    /// Picks the form of each lowercase letter.
    allographs: u64,
    /// 0 none, 1 dot, 2 dash.
    dots: usize,
    /// 0 none, 1 short, 2 long.
    bars: usize,
}

fn style(design: u64) -> Style {
    let index = (design & 0xFFFF) as usize % STYLE_COUNT;
    let mut r = SeededRandom::new(design >> 16);
    let archetype = Archetype::ALL.get(index / (FLOURISHES * JOINS * CAPITAL_VARIANTS)).copied().unwrap_or(Archetype::Surname);
    // Field order is the Swift initialiser's argument order: it fixes the order of random draws.
    let slant = r.range(0.12, 0.6);
    let xh = r.range(0.26, 0.46);
    let initial_scale = r.range(0.7, 1.0);
    let cap_w = r.range(0.6, 1.0);
    let letter_w = r.range(0.95, 1.4);
    let decay = r.range(0.62, 0.9);
    let legible_decay = r.range(0.88, 0.98);
    let garland = r.chance(0.5);
    let loops = r.chance(0.6);
    let rise = rising(r.range(0.0, 1.0));
    let entry = r.chance(0.35);
    let exit_length = r.range(0.15, 0.6);
    let exit_lift = r.range(-0.2, 0.9);
    let pen = r.range(0.026, 0.034);
    let keep = r.range(3.0, 7.9) as usize;
    let legible = r.range(1.0, 3.9) as usize;
    let wave_h = (0..12).map(|_| r.range(0.75, 1.25)).collect();
    let wave_w = (0..12).map(|_| r.range(0.8, 1.25)).collect();
    let mut s = Style {
        archetype,
        flourish: (index / (JOINS * CAPITAL_VARIANTS)) % FLOURISHES,
        join: index % JOINS,
        capital: (index / JOINS) % CAPITAL_VARIANTS,
        slant,
        xh,
        initial_scale,
        cap_w,
        letter_w,
        decay,
        legible_decay,
        garland,
        loops,
        rise,
        entry,
        exit_length,
        exit_lift,
        pen,
        keep,
        legible,
        wave_h,
        wave_w,
        allographs: 0,
        dots: 1,
        bars: 1,
    };
    s.allographs = r.next_u64();
    s.dots = r.weighted(&[0.8, 1.4, 1.0]);
    s.bars = r.weighted(&[0.5, 1.4, 1.0]);
    if archetype == Archetype::Monogram && s.flourish == 0 {
        s.flourish = 4;
    }
    if archetype == Archetype::Enclosed {
        s.flourish = 99; // loop around the whole signature
    }
    s
}

/// Most signatures climb 2–8°; about one in eight runs level.
fn rising(u: f64) -> f64 {
    if u < 0.12 {
        -0.02 + u / 0.12 * 0.05
    } else {
        0.035 + (u - 0.12) / 0.88 * 0.105
    }
}

/// Bounding box of points (CGRect.null when empty).
#[derive(Clone, Copy, Debug)]
struct Bounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl Bounds {
    fn add(b: Option<Bounds>, p: Pt) -> Bounds {
        match b {
            None => Bounds { min_x: p.x, min_y: p.y, max_x: p.x, max_y: p.y },
            Some(b) => Bounds { min_x: b.min_x.min(p.x), min_y: b.min_y.min(p.y), max_x: b.max_x.max(p.x), max_y: b.max_y.max(p.y) },
        }
    }
    fn width(&self) -> f64 {
        self.max_x - self.min_x
    }
    fn height(&self) -> f64 {
        self.max_y - self.min_y
    }
    fn mid_x(&self) -> f64 {
        (self.min_x + self.max_x) / 2.0
    }
    fn mid_y(&self) -> f64 {
        (self.min_y + self.max_y) / 2.0
    }
}

/// The pen paths (cap-height units) for a name in a style; `copy` varies this written copy.
struct Planner<'a> {
    s: &'a Style,
    copy: &'a mut SeededRandom,
    strokes: Vec<Vec<Pt>>,
    current: Vec<Pt>,
    x: f64,
    capital_box: Option<Bounds>,
    rise: f64,
    marks: Vec<(f64, f64, Mark)>,
    delayed: Vec<Vec<Pt>>,
}

impl Planner<'_> {
    fn baseline(&self, x: f64) -> f64 {
        self.rise * x
    }

    fn r(&mut self, lo: f64, hi: f64) -> f64 {
        self.copy.range(lo, hi)
    }

    fn flush(&mut self) {
        if self.current.len() > 1 {
            self.strokes.push(std::mem::take(&mut self.current));
        }
        self.current.clear();
    }

    fn capital(&mut self, c: char, base_scale: f64) {
        let s = self.s;
        let mut parts = alphabet::capital(c);
        match s.capital {
            2 => {
                // Written fast: interior via-points thinned out.
                parts = parts
                    .into_iter()
                    .map(|p| {
                        if p.len() > 4 {
                            let n = p.len();
                            p.into_iter().enumerate().filter(|(i, _)| *i == 0 || *i == n - 1 || i % 2 == 1).map(|(_, q)| q).collect()
                        } else {
                            p
                        }
                    })
                    .collect();
            }
            3 => {
                // Tall upper strokes.
                parts = parts
                    .into_iter()
                    .map(|p| p.into_iter().map(|q| pt(q.x * 1.08, if q.y > 0.55 { 0.55 + (q.y - 0.55) * 1.35 } else { q.y })).collect())
                    .collect();
            }
            _ => {}
        }
        let scale = base_scale * self.r(0.96, 1.04);
        let width = s.cap_w * scale * self.r(0.96, 1.04);
        let dx = self.r(-0.03, 0.03);
        let dy = self.r(-0.03, 0.03);
        let x = self.x;
        for (i, part) in parts.into_iter().enumerate() {
            let mut pts: Vec<Pt> = part.iter().map(|p| pt(x + dx + p.x * width, self.baseline(x) + dy + p.y * scale)).collect();
            if i == 0 && s.capital == 1 && self.current.is_empty() {
                if let Some(&first) = pts.first() {
                    // Lead-in loop under the start of the capital.
                    pts.splice(
                        0..0,
                        [
                            pt(first.x - 0.22 * scale, first.y - 0.18 * scale),
                            pt(first.x - 0.02 * scale, first.y - 0.3 * scale),
                            pt(first.x + 0.08 * scale, first.y - 0.12 * scale),
                        ],
                    );
                }
            }
            if i == 0 && s.entry && self.current.is_empty() {
                pts.insert(0, pt(x - 0.25 * scale, self.baseline(x) - 0.12 * scale));
            }
            for &p in &pts {
                self.capital_box = Some(Bounds::add(self.capital_box, p));
            }
            if i == 0 {
                self.current.extend(pts);
            } else {
                self.flush();
                self.strokes.push(pts);
            }
        }
        self.x += width * 0.95;
    }

    /// `fade` (0…1): how far into the word — later letters are written faster, smaller and simpler.
    fn lower(&mut self, c: char, amp: f64, fade: f64) {
        let s = self.s;
        let mut h = s.allographs ^ (c as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        h ^= h >> 29;
        let Some((w, raw)) = alphabet::lower(c, (h % 7) as i64) else {
            self.wave(0, amp, false);
            return;
        };
        let mut pts = raw;
        if fade > 0.45 && pts.len() > 4 {
            // Interior turns dropped: the letter becomes a quicker arc between its ends.
            let n = pts.len();
            pts = pts.into_iter().enumerate().filter(|(i, _)| *i == 0 || *i == n - 1 || i % 2 == 0).map(|(_, p)| p).collect();
        }
        let flatten = 1.0 - 0.3 * fade;
        let width = w * s.xh * s.letter_w * (1.0 - 0.12 * fade) * self.r(0.95, 1.05);
        let a = amp * self.r(0.95, 1.05);
        let dy = self.r(-0.015, 0.015);
        let x = self.x;
        let b = self.baseline(x);
        self.current.extend(pts.iter().map(|p| pt(x + p.x * width, b + dy + (0.5 + (p.y - 0.5) * flatten) * s.xh * a)));
        match alphabet::mark(c) {
            Mark::None => {}
            Mark::Bar => self.marks.push((x + width * 0.45, b + s.xh * a * 1.25, Mark::Bar)),
            m => self.marks.push((x + width * 0.45, b + s.xh * a * 1.5, m)),
        }
        self.x += width;
    }

    fn add_marks(&mut self) {
        let s = self.s;
        for (mx, my, kind) in std::mem::take(&mut self.marks) {
            // Dots, breves and diaeresis are placed carelessly; some writers drop them (dots == 0).
            let jx = self.r(-0.03, 0.06);
            let jy = self.r(-0.03, 0.03);
            match kind {
                Mark::Dot if s.dots > 0 => {
                    let len = if s.dots == 1 { 0.03 } else { 0.1 };
                    let ex = mx + jx + len + self.r(0.0, 0.03);
                    let ey = my + jy + 0.015 + self.r(-0.01, 0.02);
                    self.delayed.push(vec![pt(mx + jx, my + jy), pt(ex, ey)]);
                }
                Mark::TwoDots if s.dots > 0 => {
                    if s.dots == 2 {
                        // Written as one short dash.
                        self.delayed.push(vec![pt(mx - 0.05 + jx, my + jy), pt(mx + 0.1 + jx, my + jy + 0.02)]);
                    } else {
                        for dx in [-0.045, 0.055] {
                            self.delayed.push(vec![pt(mx + dx + jx, my + jy), pt(mx + dx + jx + 0.03, my + jy + 0.012)]);
                        }
                    }
                }
                Mark::Breve if s.dots > 0 => {
                    let w = self.r(0.11, 0.15);
                    let d = self.r(0.04, 0.07);
                    self.delayed.push(vec![
                        pt(mx - w / 2.0 + jx, my + 0.05 + jy),
                        pt(mx - w * 0.15 + jx, my - d + jy),
                        pt(mx + w * 0.2 + jx, my - d * 0.8 + jy),
                        pt(mx + w / 2.0 + jx, my + 0.06 + jy),
                    ]);
                }
                Mark::Bar if s.bars > 0 => {
                    let len = if s.bars == 1 { 0.25 } else { 0.6 };
                    let ax = mx - 0.12 + self.r(-0.04, 0.04);
                    let ay = my + self.r(-0.04, 0.02);
                    let bx = mx - 0.12 + len * self.r(0.85, 1.15);
                    let by = my + self.r(0.0, 0.08);
                    self.delayed.push(vec![pt(ax, ay), pt(bx, by)]);
                }
                _ => {}
            }
        }
    }

    fn wave(&mut self, i: usize, amp: f64, tall: bool) {
        let s = self.s;
        let width = s.xh * s.letter_w * 0.9 * s.wave_w[i % 12] * self.r(0.92, 1.08);
        let h = s.xh * amp * s.wave_h[i % 12] * (if tall && s.loops { 2.0 } else { 1.0 }) * self.r(0.9, 1.1);
        let x = self.x;
        let b = self.baseline(x);
        if s.garland {
            self.current.extend([pt(x + width * 0.3, b), pt(x + width * 0.75, b + h)]);
            if tall && s.loops {
                self.current.push(pt(x + width * 0.55, b + h * 0.8));
            }
        } else {
            self.current.push(pt(x + width * 0.3, b + h));
            if tall && s.loops {
                self.current.push(pt(x + width * 0.12, b + h * 0.8));
            }
            self.current.push(pt(x + width * 0.75, b));
        }
        self.x += width;
    }

    /// Word: capital, `legible` letters written out, the rest as a fading garland (or all legible).
    fn word(&mut self, w: &[char], legible: Option<usize>, garland_count: Option<usize>) {
        let s = self.s;
        self.capital(w[0], 1.0);
        let rest = &w[1..];
        let mut amp = 1.0;
        let n = legible.unwrap_or(rest.len());
        // Legibility falls along the word: the first letters are written out, the last ones run together.
        for (i, &c) in rest.iter().take(n).enumerate() {
            let fade = if rest.len() > 2 { i as f64 / (rest.len() - 1) as f64 } else { 0.0 };
            self.lower(c, amp, fade * fade);
            amp *= s.legible_decay;
        }
        let remaining = &rest[n.min(rest.len())..];
        let garland = garland_count.unwrap_or(remaining.len().min(s.keep));
        for i in 0..garland {
            let c = remaining.get(i).copied().unwrap_or('n');
            self.wave(i, amp, alphabet::is_tall(c));
            amp *= s.decay;
        }
        self.add_marks();
    }

    fn initials(&mut self, letters: &[char], scale: f64) {
        let s = self.s;
        for &c in letters {
            self.capital(c, scale);
            if s.join == 1 {
                self.flush();
            }
            if s.join == 2 {
                self.flush();
                let x = self.x;
                self.strokes.push(dot(pt(x + 0.04, self.baseline(x) + 0.03)));
                self.x += 0.12;
            }
            self.x += if s.join == 0 { 0.02 } else { 0.1 };
        }
    }

    fn exit(&mut self) {
        let s = self.s;
        let length = s.exit_length * self.r(0.6, 1.4);
        let x = self.x;
        let y = self.baseline(x) + s.xh * (s.exit_lift + self.r(-0.3, 0.3));
        self.current.push(pt(x + length, y));
        self.x += length;
    }

    /// A flourish control point moved for this copy (more than the letters).
    fn v(&mut self, p: Pt) -> Pt {
        let x = p.x + self.r(-0.09, 0.09);
        let y = p.y + self.r(-0.06, 0.06);
        pt(x, y)
    }
}

fn dot(p: Pt) -> Vec<Pt> {
    vec![p, pt(p.x + 0.02, p.y + 0.015), pt(p.x + 0.035, p.y)]
}

fn plan(words: &[Vec<char>], s: &Style, copy: &mut SeededRandom) -> Vec<Vec<Pt>> {
    let surname = words.last().expect("words is not empty");
    let given = &words[..words.len() - 1];
    // What varies between one person's signatures is mostly the layout, while letter shapes stay put.
    let rise = s.rise + copy.range(-0.025, 0.025);
    let mut p = Planner { s, copy, strokes: Vec::new(), current: Vec::new(), x: 0.0, capital_box: None, rise, marks: Vec::new(), delayed: Vec::new() };
    let given_initials: Vec<char> = given.iter().filter_map(|g| g.first().copied()).collect();
    let first2: Vec<char> = given_initials.iter().take(2).copied().collect();
    let first1: Vec<char> = given_initials.iter().take(1).copied().collect();

    match s.archetype {
        Archetype::Full => {
            for g in given {
                p.word(g, None, None);
                p.flush();
                p.x += 0.22;
            }
            p.word(surname, None, None);
            p.exit();
        }
        Archetype::FullFading => {
            for g in given {
                p.word(g, None, None);
                if s.join == 1 {
                    p.flush();
                }
                p.x += 0.18;
            }
            p.word(surname, Some(s.legible + 1), None);
            p.exit();
        }
        Archetype::InitialSurname => {
            p.initials(&first2, s.initial_scale);
            p.word(surname, None, None);
            p.exit();
        }
        Archetype::InitialSurnameFading | Archetype::Enclosed => {
            p.initials(&first2, s.initial_scale);
            p.word(surname, Some(s.legible), None);
            p.exit();
        }
        Archetype::Surname => {
            p.word(surname, None, None);
            p.exit();
        }
        Archetype::SurnameFading => {
            p.word(surname, Some(s.legible), None);
            p.exit();
        }
        Archetype::InitialsScribble => {
            p.initials(&first1, s.initial_scale);
            p.word(surname, Some(0), Some(s.keep.max(3)));
            p.exit();
        }
        Archetype::CapitalScribble => {
            p.word(surname, Some(0), Some(s.keep.max(3)));
            p.exit();
        }
        Archetype::Monogram => {
            for &c in &first1 {
                p.capital(c, 1.15);
                p.x -= s.cap_w * 0.45;
            }
            p.capital(surname[0], 1.05);
            p.exit();
        }
    }
    p.flush();

    // Flourish, from the extent of what was written; its control points move more than the letters.
    let flourish_start = p.strokes.len();
    let main_end = p.strokes.last().map_or(0, |s| s.len());
    let all: Vec<Pt> = p.strokes.iter().flatten().copied().collect();
    let left = all.iter().map(|q| q.x).reduce(f64::min).unwrap_or(0.0);
    let right = all.iter().map(|q| q.x).reduce(f64::max).unwrap_or(p.x);
    let low = all.iter().map(|q| q.y).reduce(f64::min).unwrap_or(0.0);
    let high = all.iter().map(|q| q.y).reduce(f64::max);
    let mid = (left + right) / 2.0;
    let end = p.strokes.last().and_then(|s| s.last().copied()).unwrap_or(pt(right, 0.0));
    let has_last = !p.strokes.is_empty();
    let append_last = |strokes: &mut Vec<Vec<Pt>>, pts: Vec<Pt>| {
        if let Some(last) = strokes.last_mut() {
            last.extend(pts);
        }
    };
    match s.flourish {
        1 => p.strokes.push(vec![pt(left - 0.15, low - 0.05), pt(mid, low - 0.28), pt(right + 0.35, low + 0.1)]),
        2 => append_last(&mut p.strokes, vec![pt(end.x + 0.25, low - 0.05), pt(mid, low - 0.32), pt(left - 0.3, low - 0.12)]),
        3 => append_last(&mut p.strokes, vec![pt(end.x + 0.3, end.y + 0.5), pt(end.x + 0.4, end.y + 1.0)]),
        4 if p.capital_box.is_some() => {
            let b = p.capital_box.unwrap();
            let (cx, cy) = (b.mid_x(), b.mid_y());
            let (rx, ry) = (b.width() * 0.8 + 0.1, b.height() * 0.65);
            p.strokes.push(
                (0..=14)
                    .map(|i| {
                        let a = 0.9 + i as f64 / 14.0 * 1.8 * PI;
                        pt(cx + rx * a.cos() + i as f64 * 0.02, cy + ry * a.sin())
                    })
                    .collect(),
            );
        }
        5 if p.capital_box.is_some() => {
            // A slash across the foot of the capital only.
            let b = p.capital_box.unwrap();
            let reach = right.min(b.max_x + 0.25 + b.width() * 0.3);
            let y = b.min_y + b.height() * 0.22;
            p.strokes.push(vec![pt(b.min_x - 0.2, y - 0.06), pt(b.mid_x(), y), pt(reach, y + 0.1)]);
        }
        6 => {
            for k in 0..2 {
                let y = low - 0.12 - k as f64 * 0.12;
                let x0 = mid + k as f64 * 0.1;
                p.strokes.push(vec![pt(x0, y), pt(x0 + 0.35, y - 0.02), pt(x0 + 0.7, y + 0.04)]);
            }
        }
        7 => {
            let r = 0.18;
            let spiral = (1..=10)
                .map(|i| {
                    let a = i as f64 / 10.0 * 2.2 * PI;
                    pt(end.x + r + r * (PI + a).cos() * (1.0 - i as f64 * 0.05), end.y + r * (PI + a).sin())
                })
                .collect();
            append_last(&mut p.strokes, spiral);
        }
        8 => {
            // Big swoop under everything, from below the capital, rising at the end.
            p.strokes.push(vec![
                pt(left + 0.1, low + 0.25),
                pt(left - 0.1, low - 0.15),
                pt(mid, low - 0.32),
                pt(right, low - 0.1),
                pt(right + 0.3, low + 0.45),
            ]);
        }
        9 => {
            // Zigzag finish.
            let zig = (1..=4)
                .map(|k| pt(end.x + k as f64 * 0.13, end.y + (if k % 2 == 0 { 0.0 } else { 0.3 }) * (1.0 - k as f64 * 0.15)))
                .collect();
            append_last(&mut p.strokes, zig);
        }
        10 => {
            // Hook up and back over the name.
            append_last(&mut p.strokes, vec![pt(end.x + 0.2, end.y + 0.55), pt(end.x - 0.1, end.y + 0.95), pt(mid, end.y + 0.9)]);
        }
        99 if has_last => {
            // Loop around everything, starting from the end.
            let high = high.unwrap_or(low);
            let (cx, cy) = (mid, (high + low) / 2.0);
            let (rx, ry) = ((right - left) / 2.0 + 0.25, (high - low) / 2.0 + 0.2);
            let ring = (0..=16)
                .map(|i| {
                    let a = -0.3 - i as f64 / 16.0 * 1.9 * PI;
                    pt(cx + rx * a.cos(), cy + ry * a.sin())
                })
                .collect();
            append_last(&mut p.strokes, ring);
        }
        _ => {}
    }
    // Copy variation of the flourish: new strokes and the points appended to the last written stroke.
    for i in flourish_start..p.strokes.len() {
        let moved: Vec<Pt> = p.strokes[i].clone().into_iter().map(|q| p.v(q)).collect();
        p.strokes[i] = moved;
    }
    if flourish_start > 0 && p.strokes[flourish_start - 1].len() > main_end {
        let k = flourish_start - 1;
        let tail: Vec<Pt> = p.strokes[k][main_end..].to_vec();
        let moved: Vec<Pt> = tail.into_iter().map(|q| p.v(q)).collect();
        p.strokes[k].truncate(main_end);
        p.strokes[k].extend(moved);
    }
    let mut strokes = p.strokes;
    strokes.extend(p.delayed);
    strokes
}

/// Words of a name (split at white space and full stops), as characters.
fn words(name: &str) -> Vec<Vec<char>> {
    name.split(|c: char| c.is_whitespace() || c == '.').filter(|w| !w.is_empty()).map(|w| w.chars().collect()).collect()
}

// MARK: - Rendering

/// Renders one copy of a signature: trimmed straight-alpha RGBA. `pixel_height` is the capital height in pixels
/// (130 for the preview, at most [`MAXIMUM_PIXEL_HEIGHT`]). `None` for an empty name or invalid input.
pub fn render(name: &str, design: u64, impression: u64, ink: Ink, pen: Pen, pixel_height: f64) -> Option<image::RgbaImage> {
    render_pixmap(name, design, impression, ink, pen, pixel_height).map(|p| texture::to_image(&p))
}

/// [`render`] as a premultiplied canvas.
pub fn render_pixmap(name: &str, design: u64, impression: u64, ink: Ink, pen: Pen, pixel_height: f64) -> Option<Pixmap> {
    let words = words(name);
    if words.is_empty() {
        return None;
    }
    let s = style(design);
    let mut copy = SeededRandom::new(impression);
    let raw = plan(&words, &s, &mut copy);
    // The hand of this design (fluency and rhythm) — fixed per design.
    let mut r = SeededRandom::new(design.wrapping_mul(0x9E37_79B9).wrapping_add(17));
    let mut hand = Hand::default();
    hand.overlap = r.range(0.68, 0.82);
    hand.corner_overlap = 0.5;
    hand.size_scaled = false;
    let sigma_lo = r.range(0.2, 0.24);
    let sigma_hi = r.range(0.26, 0.31);
    hand.sigma = (sigma_lo, sigma_hi);
    hand.curvature_gain = r.range(0.3, 0.45);
    hand.tempo = r.range(0.13, 0.2);
    draw(&raw, &hand, s.slant, s.pen, pen, ink, pixel_height, &mut copy, &DrawOptions::default()).map(|(image, _)| image)
}

/// Optional parameters of [`draw`] (learned signatures use them; generated ones keep the defaults).
#[derive(Clone, Debug)]
pub(crate) struct DrawOptions {
    pub variation: f64,
    pub durations: Option<Vec<Vec<f64>>>,
    pub run_out: bool,
    pub smoothing: Option<f64>,
    pub sharp_corners: bool,
    pub merge: f64,
    pub layout: Option<f64>,
    pub stroke_shift: f64,
}

impl Default for DrawOptions {
    fn default() -> Self {
        Self { variation: 1.0, durations: None, run_out: true, smoothing: None, sharp_corners: false, merge: 0.012, layout: None, stroke_shift: 0.0 }
    }
}

/// Shared by generated and learned signatures: a smooth centripetal spline through the via-points, low-pass
/// filtered like a fast hand, with this copy's drift, size, slant and angle; the pen's speed along that line comes
/// from the sigma-lognormal model. Returns the image and the pen trajectories in pixels.
#[allow(clippy::too_many_arguments)]
pub(crate) fn draw(
    raw: &[Vec<Pt>],
    hand: &Hand,
    base_slant: f64,
    pen_ratio: f64,
    pen: Pen,
    ink: Ink,
    pixel_height: f64,
    copy: &mut SeededRandom,
    o: &DrawOptions,
) -> Option<(Pixmap, Vec<Vec<PenSample>>)> {
    if !(pixel_height.is_finite()
        && pixel_height > 0.0
        && pixel_height <= MAXIMUM_PIXEL_HEIGHT
        && base_slant.is_finite()
        && pen_ratio.is_finite()
        && pen_ratio > 0.0
        && pen_ratio <= 1.0
        && o.variation.is_finite()
        && (0.0..=2.0).contains(&o.variation)
        && raw.len() <= 1_024)
    {
        return None;
    }
    let mut raw_count = 0;
    for stroke in raw {
        if stroke.len() > MAXIMUM_SAMPLE_POINTS - raw_count || !stroke.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            return None;
        }
        raw_count += stroke.len();
    }
    let vary = o.variation;
    let jitter = 0.006 * vary;
    let drift_a = copy.range(0.02, 0.045) * vary;
    let drift_x = copy.range(1.1, 2.2);
    let drift_y = copy.range(0.9, 1.8);
    let drift_p1 = copy.range(0.0, 6.28);
    let drift_p2 = copy.range(0.0, 6.28);
    let size = pixel_height * copy.range(0.93, 1.07);
    // Layout (slant, angle) varies on its own scale.
    let lay = o.layout.unwrap_or(o.variation);
    let slant = base_slant + copy.range(-0.08, 0.08) * lay;
    let angle = copy.range(-0.06, 0.06) * lay;
    let x_stretch = copy.range(0.94, 1.06);
    let (ca, sa) = (angle.cos(), angle.sin());
    let width = (size * pen_ratio * copy.range(0.95, 1.05)).max(2.0);
    let to_pixels = |p: Pt| {
        let q = pt((p.x + p.y * slant) * size * x_stretch, p.y * size);
        pt(q.x * ca - q.y * sa, q.x * sa + q.y * ca)
    };
    let mut trajectories: Vec<Vec<PenSample>> = Vec::new();
    // Above the usual capital height the same pen is drawn at a higher resolution, not a finer pen.
    let scale = (pixel_height / 130.0).max(1.0);
    let mut spatial_budget = (MAXIMUM_RENDER_SAMPLES as f64 * scale) as i64;
    let mut timed_budget = MAXIMUM_RENDER_SAMPLES as i64;
    let mut timing_work_budget = 5_000_000.0;
    for (index, stroke) in raw.iter().enumerate() {
        // After a pen lift the next part lands a little elsewhere each time.
        let shift = pt(copy.range(-1.0, 1.0) * o.stroke_shift, copy.range(-1.0, 1.0) * o.stroke_shift);
        let mut via: Vec<Pt> = Vec::new();
        let mut corners = HashSet::new();
        for &p in stroke {
            if let Some(last) = via.last() {
                if (last.x - p.x).hypot(last.y - p.y) < o.merge {
                    corners.insert(via.len() - 1);
                    continue;
                }
            }
            via.push(p);
        }
        via = via
            .into_iter()
            .map(|p| {
                let x = p.x + shift.x + drift_a * (p.y / drift_y * 2.0 * PI + drift_p1).sin() + copy.range(-jitter, jitter);
                let y = p.y + shift.y + drift_a * (p.x / drift_x * 2.0 * PI + drift_p2).sin() + copy.range(-jitter, jitter);
                pt(x, y)
            })
            .collect();
        if via.len() <= 1 {
            if let Some(&p) = via.first() {
                trajectories.push(vec![PenSample { p: to_pixels(p), t: 0.0, speed: 0.0, direction: 0.0 }]);
            }
            continue;
        }
        // Geometry: smooth line in pixels, about one pixel between samples. Sharp corners (learned signatures):
        // the spline is broken at cusps.
        let mut path: Vec<Pt> = Vec::new();
        if o.sharp_corners && !corners.is_empty() {
            let mut sorted: Vec<usize> = corners.iter().copied().collect();
            sorted.sort_unstable();
            sorted.push(via.len() - 1);
            let mut start = 0;
            for c in sorted {
                if c <= start {
                    continue;
                }
                let piece = spline_through(&via[start..=c], 22);
                if path.is_empty() {
                    path.extend(piece);
                } else {
                    path.extend(piece.into_iter().skip(1));
                }
                start = c;
            }
        } else {
            path = spline_through(&via, 22);
        }
        let smoothing = o.smoothing.map(|k| size * k).unwrap_or((width * 3.0).max(size * 0.03));
        let pixels: Vec<Pt> = path.into_iter().map(to_pixels).collect();
        let line = fast_hand(pixels, smoothing, o.run_out, spatial_budget, scale)?;
        spatial_budget -= line.len() as i64;
        if line.len() <= 2 {
            continue;
        }
        let mut lengths = vec![0.0];
        for i in 1..line.len() {
            lengths.push(lengths[i - 1] + (line[i].x - line[i - 1].x).hypot(line[i].y - line[i - 1].y));
        }
        let total = lengths[lengths.len() - 1].max(1e-6);
        // Time: lognormal impulses for the movements between via-points (this copy's timing perturbed).
        let durations = o.durations.as_ref().and_then(|d| d.get(index)).map(|d| d.as_slice());
        let comps = kinematics::components(&via, &corners, durations, hand, copy);
        let comps = kinematics::perturb(&comps, o.variation, copy);
        // Reject invalid or excessive timing before integrating it.
        let first = comps.first()?;
        if !comps.iter().all(|c| c.d.is_finite() && c.t0.is_finite() && c.mu.is_finite() && c.sigma.is_finite() && c.sigma > 0.0) {
            return None;
        }
        let end = comps.iter().map(|c| c.t0 + (c.mu + 3.2 * c.sigma).exp()).reduce(f64::max)?;
        let estimated_steps = ((end - first.t0) * 400.0).ceil() + 1.0;
        let timing_work = estimated_steps * comps.len() as f64;
        if !(estimated_steps.is_finite() && estimated_steps > 0.0 && estimated_steps <= timed_budget as f64 && timing_work <= timing_work_budget) {
            return None;
        }
        timing_work_budget -= timing_work;
        let profile = kinematics::speed_profile(&comps, 1.0 / 400.0);
        if profile.len() as i64 > timed_budget {
            return None;
        }
        timed_budget -= profile.len() as i64;
        let travel: f64 = profile.iter().fold(0.0, |acc, s| acc + s.1);
        if !(travel > 0.0) {
            continue;
        }
        // Walk the line at the model's speed: distance travelled so far → position on the line.
        let mut samples = Vec::with_capacity(profile.len());
        let mut done = 0.0;
        let mut j = 0;
        for &(t, v) in &profile {
            done += v;
            let s = total * (done / travel).min(1.0);
            while j < line.len() - 2 && lengths[j + 1] < s {
                j += 1;
            }
            let seg = (lengths[j + 1] - lengths[j]).max(1e-6);
            let u = (s - lengths[j]) / seg;
            let p = pt(line[j].x + (line[j + 1].x - line[j].x) * u, line[j].y + (line[j + 1].y - line[j].y) * u);
            let dir = (line[j + 1].y - line[j].y).atan2(line[j + 1].x - line[j].x);
            samples.push(PenSample { p, t, speed: v / travel * total, direction: dir });
        }
        if samples.len() > 1 {
            trajectories.push(samples);
        }
    }
    if trajectories.is_empty() {
        return None;
    }
    let points: Vec<Pt> = trajectories.iter().flatten().map(|s| s.p).collect();
    if !points.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
        return None;
    }
    let min_x = points.iter().map(|p| p.x).reduce(f64::min)?;
    let max_x = points.iter().map(|p| p.x).reduce(f64::max)?;
    let min_y = points.iter().map(|p| p.y).reduce(f64::min)?;
    let max_y = points.iter().map(|p| p.y).reduce(f64::max)?;
    if !(max_x - min_x + width * 8.0 < 6_000.0 * scale && max_y - min_y + width * 8.0 < 4_000.0 * scale) {
        return None;
    }
    let ink = shift_ink(ink, copy);
    let image = pen::draw(&trajectories, pen, width, ink, copy, scale)?;
    Some((image, trajectories))
}

/// The hand at speed: even spacing along the path (about one pixel), a short straight run-out where the pen
/// lifts, and a low-pass filter over a few pen widths — fast movements carry no small wobbles.
fn fast_hand(path: Vec<Pt>, smoothing: f64, run_out: bool, point_budget: i64, scale: f64) -> Option<Vec<Pt>> {
    if !(path.iter().all(|p| p.x.is_finite() && p.y.is_finite()) && smoothing.is_finite() && smoothing >= 0.0 && smoothing <= 1_024.0 * scale && point_budget >= 2) {
        return None;
    }
    if path.len() < 2 {
        return Some(path);
    }
    let mut p = path;
    let (a, b) = (p[p.len() - 2], p[p.len() - 1]);
    let d = (b.x - a.x).hypot(b.y - a.y).max(0.001);
    if run_out {
        p.push(pt(b.x + (b.x - a.x) / d * smoothing * 3.0, b.y + (b.y - a.y) / d * smoothing * 3.0));
    }
    let min_x = p.iter().map(|q| q.x).reduce(f64::min)?;
    let max_x = p.iter().map(|q| q.x).reduce(f64::max)?;
    let min_y = p.iter().map(|q| q.y).reduce(f64::min)?;
    let max_y = p.iter().map(|q| q.y).reduce(f64::max)?;
    if !(max_x - min_x < 6_000.0 * scale && max_y - min_y < 4_000.0 * scale) {
        return None;
    }
    let mut length = 0.0;
    for i in 1..p.len() {
        length += (p[i].x - p[i - 1].x).hypot(p[i].y - p[i - 1].y);
        if !(length.is_finite() && length <= (point_budget - 2) as f64) {
            return None;
        }
    }
    let mut even = vec![p[0]];
    let mut carry = 0.0;
    for i in 1..p.len() {
        let (s, e) = (p[i - 1], p[i]);
        let len = (e.x - s.x).hypot(e.y - s.y);
        let mut t = 1.0 - carry;
        while t <= len {
            if even.len() as i64 >= point_budget - 1 {
                return None;
            }
            even.push(pt(s.x + (e.x - s.x) * t / len, s.y + (e.y - s.y) * t / len));
            t += 1.0;
        }
        carry = len - (t - 1.0);
    }
    even.push(p[p.len() - 1]);
    let k = (smoothing as usize).max(1);
    for _ in 0..2 {
        let src = even;
        let n = src.len();
        even = (0..n)
            .map(|i| {
                let (lo, hi) = (i.saturating_sub(k), (i + k).min(n - 1));
                let w = (hi - lo + 1) as f64;
                let (mut sx, mut sy) = (0.0, 0.0);
                for q in &src[lo..=hi] {
                    sx += q.x;
                    sy += q.y;
                }
                pt(sx / w, sy / w)
            })
            .collect();
    }
    Some(even)
}

/// Centripetal Catmull-Rom spline through the via-points (no overshoot loops between uneven points).
fn spline_through(p: &[Pt], steps: usize) -> Vec<Pt> {
    if p.len() <= 2 {
        return p.to_vec();
    }
    let knot = |a: Pt, b: Pt| (b.x - a.x).hypot(b.y - a.y).powf(0.5).max(1e-4);
    let mut out = Vec::with_capacity((p.len() - 1) * steps + 1);
    for i in 0..p.len() - 1 {
        let (p0, p1, p2, p3) = (p[i.saturating_sub(1)], p[i], p[i + 1], p[(i + 2).min(p.len() - 1)]);
        if p1 == p2 {
            continue;
        }
        let t1 = knot(p0, p1);
        let t2 = t1 + knot(p1, p2);
        let t3 = t2 + knot(p2, p3);
        for k in 0..steps {
            let t = t1 + (t2 - t1) * k as f64 / steps as f64;
            let lerp = |a: Pt, b: Pt, ta: f64, tb: f64| {
                let u = if tb - ta < 1e-6 { 0.0 } else { (t - ta) / (tb - ta) };
                pt(a.x + (b.x - a.x) * u, a.y + (b.y - a.y) * u)
            };
            let a1 = lerp(p0, p1, 0.0, t1);
            let a2 = lerp(p1, p2, t1, t2);
            let a3 = lerp(p2, p3, t2, t3);
            let b1 = lerp(a1, a2, 0.0, t2);
            let b2 = lerp(a2, a3, t1, t3);
            out.push(lerp(b1, b2, t1, t2));
        }
    }
    out.push(p[p.len() - 1]);
    out
}

/// The ink colour, nudged in brightness, saturation and hue for this copy (NSColor HSB round trip).
fn shift_ink(ink: Ink, copy: &mut SeededRandom) -> Ink {
    let (h, s, b) = rgb_to_hsb(ink.r, ink.g, ink.b);
    let h = (h + copy.range(-0.01, 0.01) + 1.0) % 1.0;
    let s = (s * copy.range(0.92, 1.08)).clamp(0.0, 1.0);
    let b = (b + copy.range(-0.05, 0.02)).clamp(0.0, 1.0);
    let (r, g, bl) = hsb_to_rgb(h, s, b);
    Ink::rgb(r, g, bl)
}

fn rgb_to_hsb(r: f64, g: f64, b: f64) -> (f64, f64, f64) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let s = if max > 0.0 { delta / max } else { 0.0 };
    let h = if delta <= 0.0 {
        0.0
    } else if max == r {
        ((g - b) / delta).rem_euclid(6.0) / 6.0
    } else if max == g {
        ((b - r) / delta + 2.0) / 6.0
    } else {
        ((r - g) / delta + 4.0) / 6.0
    };
    (h, s, max)
}

fn hsb_to_rgb(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let h6 = (h * 6.0).rem_euclid(6.0);
    let i = h6.floor();
    let f = h6 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as u32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

#[cfg(test)]
mod tests;
