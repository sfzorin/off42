use std::{fmt::Write, io::Cursor};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

use super::{Barcode, Format, PT_PER_MM};

const PREFIX: &str = "Suite42.TechnicalElement.v1:";
const FONT: &[u8] = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf");

/// A product mark from the shared catalogue (`resources/marks/marks.json`, imported from Lably into Kit42): conformity,
/// electrical safety, environmental, hazard, handling and interface marks. Ids are file names, so recipes made on
/// macOS and Windows resolve to the same artwork. Inserting a mark is artwork only: it certifies nothing.
pub struct TechnicalSymbol {
    pub id: &'static str,
    /// English name with its region, e.g. "CE — EU" (also accepted when typed into the format field).
    pub title: &'static str,
    /// SVG source; raster marks (OSHA GHS pictograms, ETL) wrap their PNG in an `<image>`.
    pub svg: &'static str,
    pub category: &'static str,
    /// Drawn from pixels: PDF placement uses an image appearance instead of vector paths.
    pub raster: bool,
    /// Lably withheld this drawing (licensing or provenance doubts); insertable with a warning.
    pub unverified: bool,
    pub legal: &'static str,
    pub needs_number: bool,
    copy: std::collections::BTreeMap<String, MarkCopy>,
}

#[derive(Clone, Debug, Default, Deserialize)]
struct MarkCopy {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    warning: Option<String>,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    region: Option<String>,
}

#[derive(Deserialize)]
struct MarkEntry {
    id: String,
    file: String,
    category: String,
    legal: String,
    #[serde(rename = "registrationNumber")]
    registration_number: bool,
    unverified: bool,
    copy: std::collections::BTreeMap<String, MarkCopy>,
}

/// Category order used by pickers (same as macOS).
pub const CATEGORIES: [&str; 8] = ["certification", "safety", "environmental", "hazardous", "handling", "wireless", "medical", "quality"];

impl TechnicalSymbol {
    fn copy(&self, language: &str) -> &MarkCopy {
        self.copy.get(language).or_else(|| self.copy.get("en")).expect("every mark has English copy")
    }
    /// Name in the interface language (Lably copy exists for 11 languages; others use English), with its region.
    pub fn name_in(&self, language: &str) -> String {
        let c = self.copy(language);
        if !self.regional() {
            return c.name.clone();
        }
        match c.region.as_deref().filter(|r| !r.is_empty() && !c.name.contains(*r)) {
            Some(region) => format!("{} — {region}", c.name),
            None => c.name.clone(),
        }
    }
    /// Marks for one market show their region ("CE — EU"); universal symbols do not.
    fn regional(&self) -> bool {
        self.copy.get("en").and_then(|c| c.region.as_deref()).is_some_and(|r| !r.is_empty() && r != "Universal")
    }
    pub fn category_in(&self, language: &str) -> String {
        self.copy(language).category.clone().unwrap_or_else(|| self.category.to_owned())
    }
    /// What the mark is for.
    pub fn purpose_in(&self, language: &str) -> String {
        self.copy(language).description.clone().unwrap_or_default()
    }
    /// The mark's own condition of use (conformity assessment, licence, applicability), if any.
    pub fn warning_in(&self, language: &str) -> Option<String> {
        self.copy(language).warning.clone().filter(|w| !w.is_empty())
    }
}

pub fn symbols() -> &'static [TechnicalSymbol] {
    static CATALOGUE: std::sync::LazyLock<Vec<TechnicalSymbol>> = std::sync::LazyLock::new(|| {
        let entries: Vec<MarkEntry> =
            serde_json::from_str(include_str!("../resources/marks/marks.json")).expect("bundled mark catalogue must be valid JSON");
        entries
            .into_iter()
            .map(|e| {
                let bytes = crate::marks_data::asset(&e.file).expect("every catalogue entry has bundled artwork");
                let raster = e.file.ends_with(".png");
                let svg: String = if raster {
                    let (w, h) = png_size(bytes).expect("bundled raster marks are PNG");
                    format!(
                        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\"><image width=\"{w}\" height=\"{h}\" href=\"data:image/png;base64,{}\"/></svg>",
                        STANDARD.encode(bytes)
                    )
                } else {
                    nestable(&String::from_utf8(bytes.to_vec()).expect("bundled SVG marks are UTF-8"))
                };
                let english = e.copy.get("en").expect("every mark has English copy");
                let title = match english.region.as_deref().filter(|r| !r.is_empty() && *r != "Universal" && !english.name.contains(*r)) {
                    Some(region) => format!("{} — {region}", english.name),
                    None => english.name.clone(),
                };
                TechnicalSymbol {
                    id: Box::leak(e.id.into_boxed_str()),
                    title: Box::leak(title.into_boxed_str()),
                    svg: Box::leak(svg.into_boxed_str()),
                    category: Box::leak(e.category.into_boxed_str()),
                    raster,
                    unverified: e.unverified,
                    legal: Box::leak(e.legal.into_boxed_str()),
                    needs_number: e.registration_number,
                    copy: e.copy,
                }
            })
            .collect()
    });
    &CATALOGUE
}

/// SVG text that can be nested inside another `<svg>`: XML declarations, DOCTYPEs and leading comments removed.
fn nestable(svg: &str) -> String {
    let mut rest = svg.trim_start_matches('\u{feff}').trim_start();
    loop {
        rest = if let Some(tail) = rest.strip_prefix("<?") {
            tail.split_once("?>").map_or("", |(_, after)| after).trim_start()
        } else if let Some(tail) = rest.strip_prefix("<!--") {
            tail.split_once("-->").map_or("", |(_, after)| after).trim_start()
        } else if let Some(tail) = rest.strip_prefix("<!DOCTYPE") {
            tail.split_once('>').map_or("", |(_, after)| after).trim_start()
        } else {
            break;
        };
    }
    rest.to_owned()
}

fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    (bytes.len() > 24 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")).then(|| {
        (u32::from_be_bytes(bytes[16..20].try_into().unwrap()), u32::from_be_bytes(bytes[20..24].try_into().unwrap()))
    })
}

/// Uses the same encoded PNG Description as the macOS applications.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TechnicalElement {
    #[serde(rename = "symbolID", skip_serializing_if = "Option::is_none")]
    pub symbol_id: Option<String>,
    #[serde(rename = "codeType")]
    pub code_type: Format,
    pub value: String,
    #[serde(rename = "widthMM")]
    pub width_mm: f64,
    pub caption: bool,
}

impl TechnicalElement {
    pub fn validate(&self) -> Result<(), String> {
        if !self.width_mm.is_finite() || !(3.0..=250.0).contains(&self.width_mm) {
            return Err("Choose a width between 3 and 250 mm.".into());
        }
        if let Some(id) = &self.symbol_id {
            if !symbols().iter().any(|symbol| symbol.id == id) {
                return Err("Unknown technical symbol.".into());
            }
        } else {
            Barcode::new(self.code_type, &self.value)?.geometry(self.width_mm)?;
        }
        Ok(())
    }

    pub fn description(&self) -> Result<String, String> {
        self.validate()?;
        let json = serde_json::to_vec(self).map_err(|error| error.to_string())?;
        Ok(format!("{PREFIX}{}", STANDARD.encode(json)))
    }

    pub fn from_description(description: &str) -> Option<Self> {
        if description.len() >= 24_000 {
            return None;
        }
        let data = STANDARD.decode(description.strip_prefix(PREFIX)?).ok()?;
        let element: Self = serde_json::from_slice(&data).ok()?;
        element.validate().ok()?;
        Some(element)
    }

    pub fn from_png(data: &[u8]) -> Option<Self> {
        if data.len() > 32_000_000 {
            return None;
        }
        let reader = png::Decoder::new(Cursor::new(data)).read_info().ok()?;
        let description = reader
            .info()
            .uncompressed_latin1_text
            .iter()
            .find(|text| text.keyword == "Description")?;
        Self::from_description(&description.text)
    }

    pub fn svg(&self) -> Result<String, String> {
        self.validate()?;
        let description = self.description()?;
        if let Some(id) = &self.symbol_id {
            let source = symbols().iter().find(|symbol| symbol.id == id).unwrap().svg;
            let tree = resvg::usvg::Tree::from_str(source, &resvg::usvg::Options::default())
                .map_err(|error| error.to_string())?;
            let height_mm = self.width_mm * f64::from(tree.size().height() / tree.size().width());
            return Ok(format!(
                "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{height_mm}mm\" viewBox=\"0 0 {} {}\"><desc>{description}</desc>{source}</svg>",
                self.width_mm, tree.size().width(), tree.size().height(),
            ));
        }
        let barcode = Barcode::new(self.code_type, &self.value)?;
        let (module_mm, rows) = barcode.geometry(self.width_mm)?;
        let module = module_mm * PT_PER_MM;
        let width = self.width_mm * PT_PER_MM;
        let code_height = f64::from(rows) * module;
        let caption_height = if self.caption { 14.0 } else { 0.0 };
        let height = code_height + caption_height;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}mm\" height=\"{}mm\" viewBox=\"0 0 {width} {height}\"><desc>{description}</desc><rect width=\"{width}\" height=\"{height}\" fill=\"white\"/><g fill=\"black\">", self.width_mm, height / PT_PER_MM
        );
        let quiet = self.code_type.quiet_zone();
        barcode.runs(|x, y, length| {
            let (top, bar_height) = if self.code_type == Format::Code128 {
                (0, rows)
            } else {
                (quiet + y, 1)
            };
            let x = f64::from(x + quiet) * module;
            let y = f64::from(top) * module;
            let width = f64::from(length) * module;
            let height = f64::from(bar_height) * module;
            write!(
                svg,
                "<rect x=\"{x:.6}\" y=\"{y:.6}\" width=\"{width:.6}\" height=\"{height:.6}\"/>"
            )
            .unwrap();
        });
        if self.caption {
            let face = ttf_parser::Face::parse(FONT, 0).map_err(|error| error.to_string())?;
            let mut caption_width = 0.0;
            for character in self.value.chars() {
                let glyph = face
                    .glyph_index(character)
                    .ok_or_else(|| format!("The caption font lacks {character:?}."))?;
                caption_width +=
                    f64::from(face.glyph_hor_advance(glyph).unwrap_or(face.units_per_em())) * 7.0
                        / f64::from(face.units_per_em());
            }
            if caption_width + 2.0 > width {
                return Err(
                    "The caption does not fit. Increase the width or turn off the caption.".into(),
                );
            }
            let value = self
                .value
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;");
            write!(svg, "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-family=\"Noto Sans\" font-size=\"7\">{value}</text>", width / 2.0, code_height + 10.0).unwrap();
        }
        svg.push_str("</g></svg>");
        Ok(svg)
    }

    pub fn png(&self) -> Result<Vec<u8>, String> {
        let svg = self.svg()?;
        let mut options = resvg::usvg::Options::default();
        options.fontdb_mut().load_font_data(FONT.to_vec());
        let tree =
            resvg::usvg::Tree::from_str(&svg, &options).map_err(|error| error.to_string())?;
        let target_width = if self.symbol_id.is_some() {
            (self.width_mm * 600.0 / 25.4).ceil() as u32
        } else {
            let barcode = Barcode::new(self.code_type, &self.value)?;
            let (module, _) = barcode.geometry(self.width_mm)?;
            let scale = (module * 600.0 / 25.4).ceil().max(4.0) as u32;
            (barcode.matrix.getWidth() + 2 * self.code_type.quiet_zone()) * scale
        };
        let scale = target_width as f32 / tree.size().width();
        let height = (tree.size().height() * scale).ceil() as u32;
        if target_width > 4096
            || height > 4096
            || u64::from(target_width) * u64::from(height) > 4_000_000
        {
            return Err("The image is too large. Choose a smaller width.".into());
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(target_width, height)
            .ok_or("Could not create technical image.")?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );
        let mut pixels = pixmap.take();
        for pixel in pixels.chunks_exact_mut(4) {
            let alpha = u32::from(pixel[3]);
            if alpha != 0 {
                for channel in &mut pixel[..3] {
                    *channel = ((u32::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
                }
            }
        }
        let mut output = Vec::new();
        let mut encoder = png::Encoder::new(&mut output, target_width, height);
        encoder.set_color(png::ColorType::Rgba);
        let ppm = (f64::from(target_width) * 1000.0 / self.width_mm).round() as u32;
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
        encoder
            .add_text_chunk("Description".into(), self.description()?)
            .map_err(|error| error.to_string())?;
        let mut writer = encoder.write_header().map_err(|error| error.to_string())?;
        writer
            .write_image_data(&pixels)
            .map_err(|error| error.to_string())?;
        writer.finish().map_err(|error| error.to_string())?;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_symbols_preserve_their_editable_recipe() {
        for symbol in symbols() {
            let element = TechnicalElement {
                symbol_id: Some(symbol.id.into()),
                code_type: Format::Qr,
                value: String::new(),
                width_mm: 20.0,
                caption: true,
            };
            let png = element.png().unwrap();
            assert_eq!(TechnicalElement::from_png(&png), Some(element));
        }
    }

    #[test]
    fn compatible_mac_recipe_round_trip_and_rejection() {
        let json = br#"{"symbolID":null,"codeType":"dataMatrix","value":"PCB-007","widthMM":35,"caption":true}"#;
        let description = format!("{PREFIX}{}", STANDARD.encode(json));
        let element = TechnicalElement::from_description(&description).unwrap();
        assert_eq!(element.code_type, Format::DataMatrix);
        assert_eq!(
            TechnicalElement::from_png(&element.png().unwrap()),
            Some(element)
        );
        assert!(TechnicalElement::from_description("untrusted image text").is_none());
    }
}

#[cfg(test)]
mod catalogue_tests {
    use super::*;

    fn element(id: &str) -> TechnicalElement {
        TechnicalElement { symbol_id: Some(id.into()), code_type: Format::Qr, value: String::new(), width_mm: 25.0, caption: false }
    }

    #[test]
    fn catalogue_has_the_97_lably_marks_and_keeps_the_earlier_ids() {
        let all = symbols();
        assert_eq!(all.len(), 97);
        let ids: std::collections::BTreeSet<_> = all.iter().map(|s| s.id).collect();
        assert_eq!(ids.len(), 97);
        for id in ["protective-earth", "functional-earth", "chassis-ground", "equipotentiality", "dc", "ac", "positive-polarity",
                   "negative-polarity", "esd", "fuse", "high-voltage", "warning", "insulation-class", "fan-cooled",
                   "ce", "eac", "ukca", "iso-9001", "osha-ghs-02-flame", "weee", "etl"] {
            assert!(ids.contains(id), "{id}");
        }
        assert!(all.iter().all(|s| CATEGORIES.contains(&s.category)));
        assert_eq!(all.iter().filter(|s| s.raster).count(), 10);
    }

    #[test]
    fn marks_carry_localized_names_and_conditions_of_use() {
        let ce = symbols().iter().find(|s| s.id == "ce").unwrap();
        assert_eq!(ce.title, "CE — EU");
        assert!(ce.warning_in("en").is_some_and(|w| w.contains("conformity assessment")));
        let eac = symbols().iter().find(|s| s.id == "eac").unwrap();
        assert!(eac.purpose_in("ru").contains("ЕАЭС"));
        assert_eq!(eac.name_in("ar"), eac.name_in("en"), "languages without Lably copy fall back to English");
        assert!(symbols().iter().find(|s| s.id == "iso-9001").unwrap().unverified);
        assert!(symbols().iter().find(|s| s.id == "bis").unwrap().needs_number);
    }

    #[test]
    fn every_mark_renders_and_raster_marks_have_no_vector_form() {
        for symbol in symbols() {
            let e = element(symbol.id);
            let png = e.png().unwrap_or_else(|err| panic!("{}: {err}", symbol.id));
            assert!(png.len() > 100, "{}", symbol.id);
            assert_eq!(TechnicalElement::from_png(&png), Some(e.clone()), "{} recipe round trip", symbol.id);
            let (w, h) = e.size_points().unwrap();
            assert!((w - 25.0 * PT_PER_MM).abs() < 1e-6 && h > 0.0, "{}", symbol.id);
            assert_eq!(e.vector().is_err(), symbol.raster, "{}", symbol.id);
        }
    }
}
