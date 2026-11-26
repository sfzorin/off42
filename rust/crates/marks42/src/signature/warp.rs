//! Copies of a signature made from the ink of a real sample rather than from a traced centre line (port of Kit42
//! `SignatureWarp` and the `SignatureLearning.darkness`/`inkThreshold` helpers it uses): the sample's ink (darkness
//! against the local paper) is bent the way one hand varies between signatures — slant, width, a slight turn and a
//! smooth, low-frequency bend — and inked again in the chosen colour with the pen's texture.

use crate::rng::SeededRandom;
use crate::texture::{self, canvas, Strength};
use crate::Ink;
use image::{imageops, GrayImage, Rgba, RgbaImage};
use std::f64::consts::PI;

/// A sample picture as stored with a learned signature: grey, at most 800 px tall.
pub fn encode(picture: &RgbaImage) -> Option<GrayImage> {
    let (pw, ph) = picture.dimensions();
    if pw == 0 || ph == 0 {
        return None;
    }
    let k = (800.0 / ph.max(1) as f64).min(1.0);
    let w = ((pw as f64 * k) as u32).max(1);
    let h = ((ph as f64 * k) as u32).max(1);
    let flat = on_white(picture);
    let resized = if (w, h) == (pw, ph) { flat } else { imageops::resize(&flat, w, h, imageops::FilterType::CatmullRom) };
    Some(GrayImage::from_fn(w, h, |x, y| {
        let p = resized.get_pixel(x, y).0;
        // sRGB to grey as CoreGraphics' device grey does (luma).
        image::Luma([(0.299 * p[0] as f64 + 0.587 * p[1] as f64 + 0.114 * p[2] as f64).round().clamp(0.0, 255.0) as u8])
    }))
}

/// The picture composited over white paper (opaque).
pub(crate) fn on_white(picture: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(picture.width(), picture.height(), |x, y| {
        let [r, g, b, a] = picture.get_pixel(x, y).0;
        let a = a as f64 / 255.0;
        let c = |v: u8| (v as f64 * a + 255.0 * (1.0 - a)).round() as u8;
        Rgba([c(r), c(g), c(b), 255])
    })
}

/// A new copy of the sampled signature in the given ink. `height` is the target picture height in pixels (300 is
/// the usual copy). Trimmed straight-alpha RGBA, or `None` for a picture too small to use.
pub fn copy(picture: &RgbaImage, ink: Ink, seed: u64, height: f64) -> Option<RgbaImage> {
    if !(height.is_finite() && height > 0.0) || picture.width() == 0 || picture.height() == 0 {
        return None;
    }
    // Work at a usable size (a signature cut from a sheet photo is often under 100 px tall).
    let scale = (height / 300.0).max(1.0);
    let k = (4.0 * scale).min(height / picture.height().max(1) as f64).max(1.0);
    let wf = picture.width() as f64 * k;
    let hf = picture.height() as f64 * k;
    if !(wf > 8.0 && hf > 8.0 && wf < 20_000.0 && hf < 20_000.0) {
        return None;
    }
    let (w, h) = (wf as usize, hf as usize);
    if w <= 8 || h <= 8 {
        return None;
    }
    let flat = on_white(picture);
    let src = imageops::resize(&flat, w as u32, h as u32, imageops::FilterType::CatmullRom);
    let lum: Vec<f64> = src.pixels().map(|p| (p.0[0] as f64 * 3.0 + p.0[1] as f64 * 6.0 + p.0[2] as f64) / 10.0).collect();
    let dark = darkness(&lum, w, h);
    let t = ink_threshold(&dark);
    // Ink coverage 0…1: paper below half the threshold, full ink at twice it.
    let cover: Vec<f64> = dark.iter().map(|d| ((d - t * 0.5) / (t * 1.5)).clamp(0.0, 1.0)).collect();
    // This copy's hand: slant, width, height, a slight turn, and a smooth bend.
    let mut r = SeededRandom::new(seed);
    let shear = r.range(-0.07, 0.07);
    let sx = r.range(0.94, 1.06);
    let sy = r.range(0.96, 1.04);
    let turn = r.range(-0.035, 0.035);
    let (ct, st) = (turn.cos(), turn.sin());
    let bends: Vec<(f64, f64, f64, f64, f64)> = (0..3)
        .map(|_| {
            let a = r.range(0.006, 0.014) * h as f64;
            let fx = r.range(0.6, 1.6) / w as f64;
            let fy = r.range(0.4, 1.2) / h as f64;
            let p = r.range(0.0, 6.28);
            let q = r.range(0.0, 6.28);
            (a, fx, fy, p, q)
        })
        .collect();
    // Output canvas with room for the bend.
    let pad = (h as f64 * 0.12) as usize;
    let ow = (w as f64 * 1.12) as usize + 2 * pad;
    let oh = h + 2 * pad;
    let mut out = canvas(ow as u32, oh as u32)?;
    let (cx, cy, scx, scy) = (ow as f64 / 2.0, oh as f64 / 2.0, w as f64 / 2.0, h as f64 / 2.0);
    let sample = |x: f64, y: f64| -> f64 {
        if !(x >= 0.0 && y >= 0.0 && x < (w - 1) as f64 && y < (h - 1) as f64) {
            return 0.0;
        }
        let (x0, y0) = (x as usize, y as usize);
        let (fx, fy) = (x - x0 as f64, y - y0 as f64);
        let a = cover[y0 * w + x0];
        let b = cover[y0 * w + x0 + 1];
        let c2 = cover[(y0 + 1) * w + x0];
        let d = cover[(y0 + 1) * w + x0 + 1];
        (a + (b - a) * fx) * (1.0 - fy) + (c2 + (d - c2) * fx) * fy
    };
    let od = out.data_mut();
    for oy in 0..oh {
        for ox in 0..ow {
            // Inverse map: output → source (undo turn, scale, shear; then the bend).
            let (mut x, mut y) = (ox as f64 - cx, oy as f64 - cy);
            (x, y) = (x * ct + y * st, -x * st + y * ct);
            x /= sx;
            y /= sy;
            x -= shear * y;
            let (mut bx, mut by) = (0.0, 0.0);
            for b in &bends {
                bx += b.0 * (oy as f64 * b.2 * 2.0 * PI + b.3).sin();
                by += b.0 * (ox as f64 * b.1 * 2.0 * PI + b.4).sin();
            }
            let v = sample(x + scx + bx, y + scy + by);
            if v <= 0.01 {
                continue;
            }
            let i = (oy * ow + ox) * 4;
            // Premultiplied ink.
            od[i] = (ink.r * v * 255.0) as u8;
            od[i + 1] = (ink.g * v * 255.0) as u8;
            od[i + 2] = (ink.b * v * 255.0) as u8;
            od[i + 3] = (v * 255.0) as u8;
        }
    }
    let pressure = r.range(0.05, 0.1);
    let gaps = r.range(0.004, 0.012);
    let along_x = r.range(0.03, 0.07);
    let mut strength = Strength { pressure, gaps, bleed: 0.3, along_x, paper: 0.0 };
    strength.paper = r.range(0.04, 0.08);
    texture::apply(&mut out, r.next_u64(), strength, scale);
    Some(texture::to_image(&texture::trim(&out, (6.0 * scale).round() as u32)))
}

/// Otsu's threshold between paper and ink on darkness values, kept between 16 and 70.
pub(crate) fn ink_threshold(dark: &[f64]) -> f64 {
    let mut histogram = [0.0f64; 256];
    for &d in dark {
        histogram[(d as i64).clamp(0, 255) as usize] += 1.0;
    }
    let total = dark.len() as f64;
    let sum: f64 = histogram.iter().enumerate().map(|(i, c)| i as f64 * c).sum();
    let (mut best, mut threshold, mut below, mut sum_below) = (0.0, 40i64, 0.0, 0.0);
    for t in 0..255usize {
        below += histogram[t];
        sum_below += t as f64 * histogram[t];
        if !(below > 0.0 && below < total) {
            continue;
        }
        let m0 = sum_below / below;
        let m1 = (sum - sum_below) / (total - below);
        let between = below * (total - below) * (m0 - m1) * (m0 - m1);
        if between > best {
            best = between;
            threshold = t as i64;
        }
    }
    // Clean white paper gives Otsu a tiny threshold (noise); a very faint picture a huge one.
    threshold.clamp(16, 70) as f64
}

/// How much darker each pixel is than the paper around it (0…255): the paper level is the brightest part of a
/// wide neighbourhood, so ink, shadows and lighting gradients do not pull it down.
pub(crate) fn darkness(lum: &[f64], w: usize, h: usize) -> Vec<f64> {
    let r = (w.min(h) / 6).max(6);
    let cell = (r / 3).max(2);
    let (cw, chh) = (w.div_ceil(cell), h.div_ceil(cell));
    let mut peaks = vec![0.0f64; cw * chh];
    for y in 0..h {
        for x in 0..w {
            let i = (y / cell) * cw + x / cell;
            peaks[i] = peaks[i].max(lum[y * w + x]);
        }
    }
    let mut smooth = peaks.clone();
    let k = (r / cell).max(1);
    for cy in 0..chh {
        for cx in 0..cw {
            let (mut s, mut n) = (0.0, 0.0);
            for yy in cy.saturating_sub(k)..=(cy + k).min(chh - 1) {
                for xx in cx.saturating_sub(k)..=(cx + k).min(cw - 1) {
                    s += peaks[yy * cw + xx];
                    n += 1.0;
                }
            }
            smooth[cy * cw + cx] = s / n;
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let paper = smooth[(y / cell) * cw + x / cell];
            out[y * w + x] = (paper - lum[y * w + x]).max(0.0) * 255.0 / paper.max(60.0);
        }
    }
    out
}
