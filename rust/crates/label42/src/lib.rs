use std::fmt::Write as _;

use lopdf::{dictionary, Document, Object, Stream};
use rxing::{common::BitMatrix, BarcodeFormat, EncodeHints, MultiFormatWriter, Writer};
use suite42_pdf_text::PdfFont;

const PT_PER_MM: f64 = 72.0 / 25.4;
const A4: [f64; 2] = [210.0, 297.0];
pub const MAX_LABELS: usize = 5000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Qr,
    DataMatrix,
    Code128,
    Aztec,
}

impl Format {
    pub fn parse(name: &str) -> Result<Self, String> {
        match name.trim().to_ascii_lowercase().as_str() {
            "qr" => Ok(Self::Qr),
            "data matrix" | "datamatrix" => Ok(Self::DataMatrix),
            "code 128" | "code128" => Ok(Self::Code128),
            "aztec" => Ok(Self::Aztec),
            _ => Err("Choose QR, Data Matrix, Code 128, or Aztec.".into()),
        }
    }

    fn engine(self) -> BarcodeFormat {
        match self {
            Self::Qr => BarcodeFormat::QR_CODE,
            Self::DataMatrix => BarcodeFormat::DATA_MATRIX,
            Self::Code128 => BarcodeFormat::CODE_128,
            Self::Aztec => BarcodeFormat::AZTEC,
        }
    }

    fn quiet_zone(self) -> u32 {
        match self {
            Self::Qr => 4,
            Self::Code128 => 10,
            Self::DataMatrix | Self::Aztec => 1,
        }
    }
}

struct Barcode {
    matrix: BitMatrix,
    format: Format,
}

impl Barcode {
    fn new(format: Format, value: &str) -> Result<Self, String> {
        if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
            return Err("Enter 1–1024 bytes of text without control characters.".into());
        }
        if format == Format::Code128 && !value.bytes().all(|b| (32..=126).contains(&b)) {
            return Err("Code 128 requires printable ASCII.".into());
        }
        let matrix = if format == Format::DataMatrix {
            let code = datamatrix::DataMatrix::encode_str(value, datamatrix::SymbolList::default())
                .map_err(|e| format!("Cannot encode this Data Matrix: {e:?}"))?;
            let bitmap = code.bitmap();
            let mut matrix = BitMatrix::new(bitmap.width() as u32, bitmap.height() as u32)
                .map_err(|e| e.to_string())?;
            for (x, y) in bitmap.pixels() {
                matrix.set(x as u32, y as u32);
            }
            matrix
        } else {
            let hints = EncodeHints {
                Margin: Some("0".into()),
                CharacterSet: Some("UTF-8".into()),
                ..Default::default()
            };
            MultiFormatWriter
                .encode_with_hints(value, &format.engine(), 0, 0, &hints)
                .map_err(|e| format!("Cannot encode this barcode: {e}"))?
        };
        if matrix.getWidth() == 0
            || matrix.getHeight() == 0
            || matrix.getWidth() > 4096
            || matrix.getHeight() > 512
        {
            return Err("Barcode exceeds the supported size.".into());
        }
        Ok(Self { matrix, format })
    }

    fn geometry(&self, width: f64) -> Result<(f64, u32), String> {
        let quiet = self.format.quiet_zone();
        let columns = self.matrix.getWidth() + 2 * quiet;
        let module = width / f64::from(columns);
        let min = if self.format == Format::DataMatrix {
            0.19
        } else {
            0.25
        };
        if module + 1e-9 < min {
            let required = (f64::from(columns) * min * 10.0).ceil() / 10.0;
            return Err(format!(
                "This barcode needs a width of at least {required:.1} mm."
            ));
        }
        let rows = if self.format == Format::Code128 {
            (10.0 / module).ceil().max(40.0) as u32
        } else {
            self.matrix.getHeight() + 2 * quiet
        };
        Ok((module, rows))
    }

    fn runs(&self, mut run: impl FnMut(u32, u32, u32)) {
        for y in 0..self.matrix.getHeight() {
            let mut x = 0;
            while x < self.matrix.getWidth() {
                if !self.matrix.get(x, y) {
                    x += 1;
                    continue;
                }
                let start = x;
                while x < self.matrix.getWidth() && self.matrix.get(x, y) {
                    x += 1;
                }
                run(start, y, x - start);
            }
        }
    }

    fn draw_pdf(&self, out: &mut String, x: f64, top: f64, module: f64, rows: u32) {
        let quiet = self.format.quiet_zone();
        self.runs(|col, row, length| {
            let (row, height) = if self.format == Format::Code128 {
                (rows, rows)
            } else {
                (quiet + row + 1, 1)
            };
            let x = x + f64::from(col + quiet) * module;
            let y = top - f64::from(row) * module;
            let w = f64::from(length) * module;
            let h = f64::from(height) * module;
            writeln!(out, "{x:.5} {y:.5} {w:.5} {h:.5} re f").unwrap();
        });
    }
}

pub fn barcode_png(format: Format, value: &str, width_mm: f64) -> Result<Vec<u8>, String> {
    if !width_mm.is_finite() || !(3.0..=250.0).contains(&width_mm) {
        return Err("Choose a barcode width between 3 and 250 mm.".into());
    }
    let barcode = Barcode::new(format, value)?;
    let (module, rows) = barcode.geometry(width_mm)?;
    let scale = (module * 300.0 / 25.4).ceil().max(4.0) as u32;
    let quiet = format.quiet_zone();
    let width = (barcode.matrix.getWidth() + 2 * quiet) * scale;
    let height = rows * scale;
    if u64::from(width) * u64::from(height) > 4_000_000 {
        return Err("Choose a smaller barcode width.".into());
    }
    let mut image = vec![255; width as usize * height as usize];
    barcode.runs(|col, row, length| {
        let left = (quiet + col) * scale;
        let (top, bottom) = if format == Format::Code128 {
            (0, height)
        } else {
            ((quiet + row) * scale, (quiet + row + 1) * scale)
        };
        for y in top..bottom {
            let start = (y * width + left) as usize;
            image[start..start + (length * scale) as usize].fill(0);
        }
    });
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Grayscale);
    let ppm = (f64::from(width) * 1000.0 / width_mm).round() as u32;
    encoder.set_pixel_dims(Some(png::PixelDimensions {
        xppu: ppm,
        yppu: ppm,
        unit: png::Unit::Meter,
    }));
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&image).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    Ok(out)
}

#[derive(Clone, Copy)]
pub struct Layout {
    pub width: f64,
    pub height: f64,
    pub horizontal_gap: f64,
    pub vertical_gap: f64,
    pub first_slot: usize,
}

pub struct Label {
    pub text: String,
    pub code: Option<String>,
    pub copies: usize,
}

pub fn sheet_pdf(
    layout: Layout,
    format: Option<Format>,
    labels: &[Label],
) -> Result<Vec<u8>, String> {
    let Layout {
        width,
        height,
        horizontal_gap,
        vertical_gap,
        first_slot,
    } = layout;
    if !(5.0..=A4[0]).contains(&width)
        || !(5.0..=A4[1]).contains(&height)
        || !(0.0..=50.0).contains(&horizontal_gap)
        || !(0.0..=50.0).contains(&vertical_gap)
    {
        return Err("Choose label dimensions that fit A4 and gaps between 0 and 50 mm.".into());
    }
    let cols = ((A4[0] + horizontal_gap) / (width + horizontal_gap) + 1e-9).floor() as usize;
    let rows = ((A4[1] + vertical_gap) / (height + vertical_gap) + 1e-9).floor() as usize;
    let capacity = cols * rows;
    if !(1..=capacity).contains(&first_slot) {
        return Err(format!("Choose a first slot from 1 through {capacity}."));
    }
    let count = labels.iter().try_fold(0, |count, label| {
        if !(1..=MAX_LABELS).contains(&label.copies) || count + label.copies > MAX_LABELS {
            return Err("Choose between 1 and 5,000 total labels.".to_string());
        }
        Ok(count + label.copies)
    })?;
    let page_count = (count + first_slot - 1).div_ceil(capacity);
    if count == 0 || page_count > 500 {
        return Err("Label output must contain at least one label and at most 500 pages.".into());
    }
    let mut text = String::new();
    for (index, label) in labels.iter().enumerate() {
        if label.text.len() > 4096
            || label
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r'))
        {
            return Err(format!(
                "Label {} contains too much text or invalid characters.",
                index + 1
            ));
        }
        text.extend(label.text.lines().flat_map(str::chars));
    }
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font = if text.is_empty() {
        None
    } else {
        Some(PdfFont::embed_subset(&mut doc, &text)?)
    };
    let margin_x = (A4[0] - cols as f64 * width - (cols - 1) as f64 * horizontal_gap) / 2.0;
    let margin_y = (A4[1] - rows as f64 * height - (rows - 1) as f64 * vertical_gap) / 2.0;
    let mut index = first_slot - 1;
    let mut render_bytes = 0;
    let mut streams = vec![String::new(); page_count];
    let mut resources = vec![lopdf::Dictionary::new(); page_count];
    for (row, label) in labels.iter().enumerate() {
        let commands = label_commands(label, format, width, height, font.as_ref())
            .map_err(|e| format!("Label {}: {e}", row + 1))?;
        render_bytes += commands.len();
        if render_bytes > 64 * 1024 * 1024 {
            return Err("The sheet is too complex. Split it into smaller batches.".into());
        }
        let mut dictionary = dictionary! {
            "Type" => "XObject",
            "Subtype" => "Form",
            "BBox" => vec![0.into(), 0.into(), pdf_num(width * PT_PER_MM), pdf_num(height * PT_PER_MM)],
        };
        if let Some(font) = &font {
            dictionary.set(
                "Resources",
                dictionary! {"Font" => dictionary! {"Label" => font.id}},
            );
        }
        let form = doc.add_object(Stream::new(dictionary, commands.into_bytes()));
        let key = format!("L{row}");
        for _ in 0..label.copies {
            let (page, slot) = (index / capacity, index % capacity);
            let x = (margin_x + (slot % cols) as f64 * (width + horizontal_gap)) * PT_PER_MM;
            let y = (A4[1] - margin_y - (slot / cols) as f64 * (height + vertical_gap) - height)
                * PT_PER_MM;
            resources[page].set(key.as_str(), form);
            writeln!(streams[page], "q 1 0 0 1 {x:.5} {y:.5} cm /{key} Do Q").unwrap();
            index += 1;
        }
    }

    let mut children = Vec::with_capacity(page_count);
    for (stream, xobjects) in streams.into_iter().zip(resources) {
        let content = doc.add_object(Stream::new(dictionary! {}, stream.into_bytes()));
        children.push(doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages,
            "Contents" => content,
            "Resources" => dictionary! {"XObject" => xobjects},
        }));
    }
    doc.objects.insert(pages, Object::Dictionary(dictionary! {
        "Type" => "Pages",
        "Kids" => children.into_iter().map(Object::Reference).collect::<Vec<_>>(),
        "Count" => page_count as i64,
        "MediaBox" => vec![0.into(), 0.into(), pdf_num(A4[0] * PT_PER_MM), pdf_num(A4[1] * PT_PER_MM)],
    }));
    let catalog = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages,
        "ViewerPreferences" => dictionary! {"PrintScaling" => "None"},
    });
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut output = Vec::new();
    doc.save_to(&mut output).map_err(|e| e.to_string())?;
    Ok(output)
}

fn pdf_num(value: f64) -> Object {
    Object::Real(value as f32)
}

fn label_commands(
    label: &Label,
    format: Option<Format>,
    width: f64,
    height: f64,
    font: Option<&PdfFont>,
) -> Result<String, String> {
    const PAD: f64 = 1.5;
    let mut out = String::from("0 g\n");
    let mut text_width = width - 2.0 * PAD;
    let mut text_height = height - 2.0 * PAD;
    let lines = label.text.lines().collect::<Vec<_>>();
    let has_text = !label.text.trim().is_empty();
    if let Some(value) = &label.code {
        let format = format.ok_or("Select a barcode format.")?;
        let barcode = Barcode::new(format, value)?;
        let code_width = if has_text && format != Format::Code128 {
            (text_width * 0.43).min(text_height)
        } else if format == Format::Code128 {
            text_width
        } else {
            text_width.min(text_height)
        };
        let (module, rows) = barcode.geometry(code_width)?;
        let code_height = f64::from(rows) * module;
        if code_height > text_height + 1e-9 {
            return Err("The barcode does not fit. Increase the label height.".into());
        }
        let (x, top) = if !has_text {
            ((width - code_width) / 2.0, (height + code_height) / 2.0)
        } else if format == Format::Code128 {
            text_height -= code_height + PAD;
            (PAD, code_height + PAD)
        } else {
            text_width -= code_width + PAD;
            (width - PAD - code_width, (height + code_height) / 2.0)
        };
        barcode.draw_pdf(
            &mut out,
            x * PT_PER_MM,
            top * PT_PER_MM,
            module * PT_PER_MM,
            rows,
        );
    }
    if !has_text {
        return if label.code.is_some() {
            Ok(out)
        } else {
            Err("The label is empty.".into())
        };
    }
    let font = font.ok_or("Missing label font.")?;
    let max_width = lines
        .iter()
        .map(|line| {
            line.chars()
                .map(|c| f64::from(font.chars[&c].2) / 1000.0)
                .sum::<f64>()
        })
        .fold(0.0_f64, f64::max);
    let size = 11.0_f64
        .min(text_width * PT_PER_MM / max_width.max(1.0))
        .min(text_height * PT_PER_MM / (1.35 * lines.len() as f64));
    if size < 5.5 {
        return Err("The text does not fit. Choose fewer fields or a larger label.".into());
    }
    let left = PAD * PT_PER_MM;
    let top = (height - PAD) * PT_PER_MM;
    for (line, text) in lines.into_iter().enumerate() {
        let baseline = top - size * (1.0 + 1.35 * line as f64);
        let encoded = font.encode(text)?;
        writeln!(
            out,
            "BT /Label {size:.4} Tf {left:.4} {baseline:.4} Td <{encoded}> Tj ET"
        )
        .unwrap();
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn exported_codes_decode_with_the_original_values() {
        for (format, payload) in [
            (Format::Qr, "Узел 007^"),
            (Format::DataMatrix, "Узел 007^"),
            (Format::Aztec, "Узел 007^"),
            (Format::Code128, "PCB-0007"),
        ] {
            let bytes = barcode_png(format, payload, 55.0).unwrap();
            let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
            let pixels_per_metre = reader.info().pixel_dims.unwrap().xppu;
            let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
            let info = reader.next_frame(&mut pixels).unwrap();
            assert!(
                (f64::from(info.width) / f64::from(pixels_per_metre) * 1000.0 - 55.0).abs() < 0.01
            );
            if format == Format::DataMatrix {
                let matrix = Barcode::new(format, payload).unwrap().matrix;
                let scale = info.width / (matrix.getWidth() + 2);
                let bits = (0..matrix.getHeight())
                    .flat_map(|y| (0..matrix.getWidth()).map(move |x| (x, y)))
                    .map(|(x, y)| {
                        let col = (x + 1) * scale + scale / 2;
                        let row = (y + 1) * scale + scale / 2;
                        pixels[(row * info.width + col) as usize] == 0
                    })
                    .collect::<Vec<_>>();
                let (map, size) = datamatrix::placement::MatrixMap::try_from_bits(
                    &bits,
                    matrix.getWidth() as usize,
                )
                .unwrap();
                let mut words = map.codewords();
                datamatrix::errorcode::decode_error(&mut words, size).unwrap();
                let code =
                    datamatrix::DataMatrix::encode_str(payload, datamatrix::SymbolList::default())
                        .unwrap();
                assert_eq!(
                    datamatrix::data::decode_str(&words[..code.data_codewords().len()]).unwrap(),
                    payload
                );
                continue;
            }
            let result = rxing::helpers::detect_in_luma_slice(
                &pixels,
                info.width,
                info.height,
                Some(format.engine()),
            )
            .unwrap_or_else(|error| panic!("{format:?}: {error}"));
            assert_eq!(result.getText(), payload, "{format:?}");
        }
    }

    #[test]
    fn sheet_reuses_forms_for_copies_across_pages_and_skips_used_slots() {
        let layout = Layout {
            width: 100.0,
            height: 140.0,
            horizontal_gap: 2.0,
            vertical_gap: 2.0,
            first_slot: 4,
        };
        let labels = [Label {
            text: "Плата 007".into(),
            code: Some("PCB-0007".into()),
            copies: 2,
        }];
        let pdf = sheet_pdf(layout, Some(Format::Code128), &labels).unwrap();
        let doc = Document::load_mem(&pdf).unwrap();
        let pages = doc.get_pages();
        assert_eq!(pages.len(), 2);
        let first = String::from_utf8(doc.get_page_content(pages[&1])).unwrap();
        let second = String::from_utf8(doc.get_page_content(pages[&2])).unwrap();
        let first_coords = first
            .split_whitespace()
            .skip(5)
            .take(2)
            .map(|v| v.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        let second_coords = second
            .split_whitespace()
            .skip(5)
            .take(2)
            .map(|v| v.parse::<f64>().unwrap())
            .collect::<Vec<_>>();
        assert!((first_coords[0] - 106.0 * PT_PER_MM).abs() < 0.01);
        assert!((first_coords[1] - 7.5 * PT_PER_MM).abs() < 0.01);
        assert!((second_coords[0] - 4.0 * PT_PER_MM).abs() < 0.01);
        assert!((second_coords[1] - 149.5 * PT_PER_MM).abs() < 0.01);
        assert_eq!(first.matches("/L0 Do").count(), 1);
        assert_eq!(second.matches("/L0 Do").count(), 1);
        let form = doc
            .objects
            .values()
            .filter_map(|obj| obj.as_stream().ok())
            .find(|stream| {
                stream
                    .dict
                    .get(b"Subtype")
                    .and_then(Object::as_name)
                    .is_ok_and(|value| value == b"Form")
            })
            .unwrap();
        let contents = String::from_utf8(form.get_plain_content().unwrap()).unwrap();
        assert!(contents.contains(" re f"));
        assert!(contents.contains(" Tj ET"));
        assert!(form.dict.get(b"Resources").is_ok());
    }

    #[test]
    fn refuses_barcodes_that_cannot_fit_or_have_the_wrong_alphabet() {
        assert!(barcode_png(Format::Qr, "1234567890", 3.0).is_err());
        assert!(barcode_png(Format::Code128, "Привет", 100.0).is_err());
        assert!(barcode_png(Format::Aztec, "\n", 35.0).is_err());
    }
}
