//! Stamp and seal impressions (port of Kit42 `SealGenerator` and `SealCatalog`).
//!
//! A stamp is a catalogue template plus the user's texts and an ink colour ([`SealSpec`]). [`render`] lays it out
//! in the same units as macOS and presses it on paper: every impression seed gives a different pressing of the same
//! stamp (tilt, uneven pad pressure, grain, gaps and an occasional faint double strike), and the same seed gives
//! the same pressing as on macOS wherever the fonts agree. [`impress`] presses a user's own stamp picture.

mod catalog;
mod layout;
mod text;

pub use catalog::{named, templates, SealCategory, SealField, SealTemplate};

use crate::rng::SeededRandom;
use crate::texture;
use crate::Ink;
use layout::{Affine, Drawing, Piece};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tiny_skia::{FillRule, Pixmap, PixmapPaint};

/// A stamp to render: a catalogue template, the user's texts (missing keys use the template's examples) and ink.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SealSpec {
    pub template_id: String,
    pub values: HashMap<String, String>,
    pub ink: Ink,
    /// Reserved for layout variations of one stamp; impressions never change the layout.
    pub design_seed: u64,
}

impl SealSpec {
    /// A stamp with the template's example texts and its default stamp-pad ink.
    pub fn example(template: &SealTemplate) -> SealSpec {
        SealSpec {
            template_id: template.id.to_string(),
            values: HashMap::new(),
            ink: template.ink.stamp_pad(),
            design_seed: 42,
        }
    }

    pub fn template(&self) -> Option<&'static SealTemplate> {
        named(&self.template_id)
    }

    /// The text drawn for a field; an empty date field means today.
    pub fn text(&self, key: &str) -> String {
        self.template().map(|t| self.text_for(t, key)).unwrap_or_default()
    }

    pub(crate) fn text_for(&self, t: &SealTemplate, key: &str) -> String {
        let raw = self.values.get(key).map(String::as_str).unwrap_or_else(|| t.example(key));
        let value = raw.trim();
        let is_date = t.fields.iter().any(|f| f.key == key && f.label.starts_with("Date"));
        if value.is_empty() && is_date && self.values.get(key).map(String::as_str) != Some(" ") && t.example(key) != " "
        {
            return today();
        }
        value.to_string()
    }
}

/// The editable fields of a template (key, label, example, multiline), in the editor's order.
pub fn fields(template_id: &str) -> Option<&'static [SealField]> {
    named(template_id).map(|t| t.fields())
}

/// Today's date as the macOS sample set shows it (dd/MM/yyyy). The calendar day is taken in UTC; the app can pass
/// its own local date as the field value instead.
pub fn today() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{d:02}/{m:02}/{y:04}")
}

/// Days since 1970-01-01 to (year, month, day) in the proleptic Gregorian calendar.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn fill_drawing(pixmap: &mut Pixmap, drawing: &Drawing, ink: Ink, t: Affine) {
    let paint = texture::paint(ink, 1.0);
    let ts = t.ts();
    for piece in &drawing.pieces {
        match piece {
            Piece::Fill(path) => pixmap.fill_path(path, &paint, FillRule::Winding, ts, None),
            Piece::Stroke(path, stroke) => pixmap.stroke_path(path, &paint, stroke, ts, None),
        }
    }
}

/// Draws a stamp as if pressed on paper (Kit42 `SealGenerator.render`). `pixel_scale`: pixels per layout unit
/// (1: preview size, about 1000 px for a round seal); the impression is the same at every scale. Returns a trimmed,
/// straight-alpha image, or `None` for an unknown template.
pub fn render(spec: &SealSpec, impression: u64, pixel_scale: f64) -> Option<image::RgbaImage> {
    let template = spec.template()?;
    let scale = pixel_scale;
    let mut rng = SeededRandom::new(impression);
    let mut drawing = Drawing::default();
    let (w, h) = layout::draw(spec, template, &mut drawing);
    let (lo, hi) = template.tilt();
    let tilt = rng.range(lo, hi) * if rng.chance(0.5) { 1.0 } else { -1.0 };
    let side = (w.max(h) * 1.25 * scale).round();
    if !(1.0..=30_000.0).contains(&side) {
        return None;
    }
    let mut canvas = texture::canvas(side as u32, side as u32)?;
    let base = Affine::flip(side)
        .translated(side / 2.0, side / 2.0)
        .scaled(scale, scale)
        .rotated(tilt)
        .translated(-w / 2.0, -h / 2.0);
    fill_drawing(&mut canvas, &drawing, spec.ink, base);
    // Sometimes the stamp rocks a little: a faint offset second impression.
    if rng.chance(0.3) {
        let alpha = rng.range(0.12, 0.25);
        let dx = rng.range(-6.0, 6.0);
        let dy = rng.range(-6.0, 6.0);
        let mut second = texture::canvas(side as u32, side as u32)?;
        fill_drawing(&mut second, &drawing, spec.ink, base.translated(dx, dy));
        let paint = PixmapPaint { opacity: alpha as f32, ..PixmapPaint::default() };
        canvas.draw_pixmap(0, 0, second.as_ref(), &paint, tiny_skia::Transform::identity(), None);
    }
    texture::apply(&mut canvas, rng.next_u64(), texture::STAMP, scale);
    Some(texture::to_image(&texture::trim(&canvas, (10.0 * scale).round() as u32)))
}

/// A new impression of an imported stamp picture (Kit42 `SealGenerator.impress`): tilt (round stamps more than
/// rectangular), pad pressure, grain, gaps and sometimes a faint double strike. The picture keeps its own colours.
/// `scale` enlarges the picture (smoothly) before pressing, so grain and bleed are finer than its pixels; below 1 it
/// is drawn smaller. The result stays under about 4000 px.
pub fn impress(picture: &image::RgbaImage, round: bool, impression: u64, scale: f64) -> Option<image::RgbaImage> {
    let (iw, ih) = (picture.width() as f64, picture.height() as f64);
    if iw < 1.0 || ih < 1.0 {
        return None;
    }
    let source = premultiplied(picture)?;
    let mut rng = SeededRandom::new(impression);
    let k = scale.min(4000.0 / (iw.max(ih) * 1.25)).max(0.25);
    let (w, h) = (iw * k, ih * k);
    let (lo, hi) = if round { (0.035, 0.14) } else { (0.005, 0.035) };
    let tilt = rng.range(lo, hi) * if rng.chance(0.5) { 1.0 } else { -1.0 };
    let side = (w.max(h) * 1.25).round();
    let mut canvas = texture::canvas(side as u32, side as u32)?;
    let base = Affine::flip(side).translated(side / 2.0, side / 2.0).rotated(tilt);
    // The picture's top row goes to the top of its rect (CGContext.draw(_:in:)).
    let place = |dx: f64, dy: f64| base.translated(-w / 2.0 + dx, h / 2.0 + dy).scaled(k, -k).ts();
    let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..PixmapPaint::default() };
    canvas.draw_pixmap(0, 0, source.as_ref(), &paint, place(0.0, 0.0), None);
    if rng.chance(0.3) {
        let alpha = rng.range(0.12, 0.25);
        let d = w.max(h) / 160.0;
        let dx = rng.range(-d, d);
        let dy = rng.range(-d, d);
        let faint = PixmapPaint { opacity: alpha as f32, ..paint };
        canvas.draw_pixmap(0, 0, source.as_ref(), &faint, place(dx, dy), None);
    }
    texture::apply(&mut canvas, rng.next_u64(), texture::STAMP, k);
    Some(texture::to_image(&texture::trim(&canvas, (10.0 * k).round() as u32)))
}

fn premultiplied(image: &image::RgbaImage) -> Option<Pixmap> {
    let mut pixmap = Pixmap::new(image.width(), image.height())?;
    for (dst, src) in pixmap.pixels_mut().iter_mut().zip(image.pixels()) {
        let [r, g, b, a] = src.0;
        *dst = tiny_skia::ColorU8::from_rgba(r, g, b, a).premultiply();
    }
    Some(pixmap)
}

#[cfg(test)]
mod tests;
