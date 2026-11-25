//! Sigma-lognormal model of rapid handwriting (port of Kit42 `Kinematics.swift`, Plamondon's kinematic theory).
//!
//! A stroke is a sum of overlapping movement components. Each component moves the pen a distance `D` along a
//! circular arc whose direction turns from `θs` to `θe`, with a lognormal speed profile (fast rise, slower decay).
//! Only the parts used by the generator are ported: components, the scalar speed profile and copy perturbation.

use crate::rng::SeededRandom;
use crate::texture::Pt;
use std::collections::HashSet;

/// A pen sample with time: position, time (s), speed and direction (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PenSample {
    pub p: Pt,
    pub t: f64,
    pub speed: f64,
    pub direction: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Component {
    /// Distance along the arc.
    pub d: f64,
    /// Onset (s).
    pub t0: f64,
    /// Log-time delay.
    pub mu: f64,
    /// Log-response time.
    pub sigma: f64,
    pub theta_start: f64,
    pub theta_end: f64,
}

/// How a stroke is written: how much consecutive components overlap (fluency) and their timing spread.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hand {
    pub overlap: f64,
    pub corner_overlap: f64,
    pub sigma: (f64, f64),
    pub tempo: f64,
    pub curvature_gain: f64,
    pub size_scaled: bool,
}

impl Default for Hand {
    fn default() -> Self {
        Self { overlap: 0.55, corner_overlap: 0.22, sigma: (0.22, 0.32), tempo: 0.16, curvature_gain: 0.45, size_scaled: true }
    }
}

/// Swift `Double.random(in: lo...hi, using: &rng)` with `SeededRandom` as the generator: a 53-bit integer drawn with
/// Lemire's nearly divisionless method below 2^53 + 1, scaled by `ulpOfOne / 2`. Bit-identical to the Swift stdlib.
pub(crate) fn swift_double(rng: &mut SeededRandom, lo: f64, hi: f64) -> f64 {
    const MAX_SIGNIFICAND: u64 = 1 << 53;
    let upper = MAX_SIGNIFICAND + 1;
    let mut random = rng.next_u64();
    let mut m = random as u128 * upper as u128;
    if (m as u64) < upper {
        let t = 0u64.wrapping_sub(upper) % upper;
        while (m as u64) < t {
            random = rng.next_u64();
            m = random as u128 * upper as u128;
        }
    }
    let rand = (m >> 64) as u64;
    if rand == MAX_SIGNIFICAND {
        return hi;
    }
    let unit = rand as f64 * (f64::EPSILON / 2.0);
    (hi - lo) * unit + lo
}

fn wrap(a: f64) -> f64 {
    let mut x = a;
    while x > std::f64::consts::PI {
        x -= 2.0 * std::f64::consts::PI;
    }
    while x < -std::f64::consts::PI {
        x += 2.0 * std::f64::consts::PI;
    }
    x
}

/// Components through via-points. `corners` marks sharp turns at via-points (the pen nearly stops).
/// `durations` (optional, one per segment) come from a recorded hand; otherwise isochrony sets them.
pub(crate) fn components(p: &[Pt], corners: &HashSet<usize>, durations: Option<&[f64]>, hand: &Hand, rng: &mut SeededRandom) -> Vec<Component> {
    if p.len() <= 1 {
        return Vec::new();
    }
    let n = p.len() - 1;
    let mut chords = Vec::with_capacity(n);
    let mut alphas = Vec::with_capacity(n);
    for k in 0..n {
        chords.push((p[k + 1].x - p[k].x).hypot(p[k + 1].y - p[k].y));
        alphas.push((p[k + 1].y - p[k].y).atan2(p[k + 1].x - p[k].x));
    }
    let mut turn = vec![0.0; n + 1];
    for k in 1..n {
        turn[k] = wrap(alphas[k] - alphas[k - 1]);
    }
    let mut out = Vec::new();
    let mut t0 = 0.0;
    let mut previous_duration = 0.0;
    for k in 0..n {
        if chords[k] <= 1e-4 {
            continue;
        }
        let a = if corners.contains(&k) { 0.0 } else { turn[k] };
        let b = if corners.contains(&(k + 1)) { 0.0 } else { turn[k + 1] };
        let delta = (hand.curvature_gain * (a + b)).clamp(-2.2, 2.2);
        let d = if delta.abs() > 1e-3 { chords[k] * (delta / 2.0) / (delta / 2.0).sin() } else { chords[k] };
        let duration = durations
            .and_then(|ds| ds.get(k).copied())
            .unwrap_or_else(|| hand.tempo * chords[k].max(0.02).powf(0.35));
        let sigma = swift_double(rng, hand.sigma.0, hand.sigma.1);
        let mu = (duration.max(0.02) / 2.2).ln();
        if k > 0 {
            let size_factor = if hand.size_scaled { (chords[k] / 0.45).clamp(0.45, 1.0) } else { 1.0 };
            let ov = if corners.contains(&k) { hand.corner_overlap } else { hand.overlap * size_factor };
            t0 += previous_duration * (1.0 - ov);
        }
        out.push(Component { d, t0, mu, sigma, theta_start: alphas[k] - delta / 2.0, theta_end: alphas[k] + delta / 2.0 });
        previous_duration = duration;
    }
    out
}

/// Scalar speed of the summed components over time: (time, distance moved in this step), with the silent tails
/// (under 2 % of the peak step) trimmed.
pub(crate) fn speed_profile(components: &[Component], dt: f64) -> Vec<(f64, f64)> {
    let Some(first) = components.first() else { return Vec::new() };
    let end = components.iter().map(|c| c.t0 + (c.mu + 3.2 * c.sigma).exp()).fold(f64::NAN, f64::max);
    let end = if end.is_nan() { first.t0 } else { end };
    let mut out = Vec::new();
    let mut t = first.t0 + 1e-4;
    let root = (2.0 * std::f64::consts::PI).sqrt();
    while t <= end {
        let mut v = 0.0;
        for c in components {
            if t > c.t0 {
                let z = ((t - c.t0).ln() - c.mu) / c.sigma;
                v += c.d / (c.sigma * root * (t - c.t0)) * (-0.5 * z * z).exp();
            }
        }
        out.push((t, v * dt));
        t += dt;
    }
    let peak = out.iter().map(|s| s.1).fold(f64::NAN, f64::max);
    let peak = if peak.is_nan() { 0.0 } else { peak };
    let a = out.iter().position(|s| s.1 > peak * 0.02).unwrap_or(0);
    let b = out.iter().rposition(|s| s.1 > peak * 0.02).unwrap_or(out.len().saturating_sub(1));
    if out.is_empty() {
        return out;
    }
    out[a..=a.max(b)].to_vec()
}

/// Copy variation in the model's own terms: each component's size, direction, timing and spread change a little.
pub(crate) fn perturb(components: &[Component], amount: f64, rng: &mut SeededRandom) -> Vec<Component> {
    components
        .iter()
        .map(|c| {
            let mut c = *c;
            c.d *= 1.0 + swift_double(rng, -0.06, 0.06) * amount;
            let d_angle = swift_double(rng, -0.045, 0.045) * amount;
            c.theta_start += d_angle + swift_double(rng, -0.03, 0.03) * amount;
            c.theta_end += d_angle + swift_double(rng, -0.03, 0.03) * amount;
            c.t0 *= 1.0 + swift_double(rng, -0.035, 0.035) * amount;
            c.mu += swift_double(rng, -0.05, 0.05) * amount;
            c.sigma *= 1.0 + swift_double(rng, -0.06, 0.06) * amount;
            c
        })
        .collect()
}
