use std::{fmt::Write, io::Cursor};

use base64::{engine::general_purpose::STANDARD, Engine};
use serde::{Deserialize, Serialize};

use super::{Barcode, Format, PT_PER_MM};

const PREFIX: &str = "Suite42.TechnicalElement.v1:";
const FONT: &[u8] = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf");

pub struct TechnicalSymbol {
    pub id: &'static str,
    pub title: &'static str,
    pub svg: &'static str,
}

pub fn symbols() -> &'static [TechnicalSymbol] {
    macro_rules! symbol {
        ($id:literal, $title:literal) => {
            TechnicalSymbol {
                id: $id,
                title: $title,
                svg: include_str!(concat!("../resources/symbols/", $id, ".svg")),
            }
        };
    }
    &[
        symbol!("protective-earth", "Protective earth"),
        symbol!("functional-earth", "Functional earth"),
        symbol!("chassis-ground", "Chassis ground"),
        symbol!("equipotentiality", "Equipotential bonding"),
        symbol!("dc", "Direct current"),
        symbol!("ac", "Alternating current"),
        symbol!("positive-polarity", "Centre-positive connector"),
        symbol!("negative-polarity", "Centre-negative connector"),
        symbol!("esd", "ESD-sensitive device"),
        symbol!("fuse", "Fuse"),
        symbol!("high-voltage", "High voltage"),
        symbol!("warning", "Warning"),
        symbol!("insulation-class", "Class II / double insulation"),
        symbol!("fan-cooled", "Fan cooled"),
    ]
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
