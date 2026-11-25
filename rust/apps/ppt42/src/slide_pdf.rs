use crate::{Ppt, SlideLayout, SlideOutput};
use lopdf::{dictionary, Dictionary, Document, Object, ObjectId, Stream};
use std::fmt::Write;
use suite42_pdf_text::PdfFont;
use unicode_segmentation::UnicodeSegmentation;
use win42::{CanvasScene, SceneFont, SceneItem, SceneTextMeasurer, TextAlign};

const DPI: i32 = 144;
const SCALE: f64 = DPI as f64 / 72.0;
const A4: (f64, f64) = (595.0, 842.0);
const MARGIN: f64 = 54.0;
const NOTE_WIDTH: f64 = A4.0 - 2.0 * MARGIN;
const LINE_HEIGHT: f64 = 16.0;
const NOTE_SIZE: f64 = 12.0;
const MAX_PAGES: usize = 1_000;
const MAX_IMAGE_BYTES: usize = 120 * 1024 * 1024;

pub(super) fn build(app: &Ppt, options: SlideOutput) -> Result<Vec<u8>, String> {
    let slide_size = app.slide_size_points().ok_or("Unknown slide dimensions")?;
    if ![slide_size.0, slide_size.1]
        .into_iter()
        .all(|side| (1.0..=14_400.0).contains(&side))
    {
        return Err("Unsupported slide dimensions for PDF export".into());
    }
    let slides: Vec<_> = app
        .deck
        .slides()
        .iter()
        .enumerate()
        .filter(|(_, slide)| options.hidden || !slide.hidden)
        .collect();
    if slides.is_empty() {
        return Err("There are no slides to export".into());
    }

    let mut doc = Document::with_version("1.7");
    let parent = doc.new_object_id();
    let mut kids = Vec::new();
    let mut image_bytes = 0;
    if let SlideLayout::Handouts(count) = options.layout {
        let indices = slides.iter().map(|(index, _)| *index).collect::<Vec<_>>();
        add_handouts(
            app, slide_size, &indices, count, &mut doc, parent, &mut kids,
        )?;
        return finish_document(doc, parent, kids);
    }
    let notes = options.layout == SlideLayout::Notes;
    let font = SceneFont {
        family: "Arial".into(),
        points: NOTE_SIZE,
        bold: false,
        italic: false,
        underline: false,
    };
    let measurer = notes
        .then(|| SceneTextMeasurer::new(&font, DPI))
        .transpose()?;
    let text_font = if notes {
        // The raster supplies the Windows glyph shapes, while this font maps
        // the original Unicode even when the bundled font lacks the outlines.
        let chars: String = slides
            .iter()
            .flat_map(|(_, slide)| slide.notes.chars())
            .filter(|ch| !matches!(ch, '\t' | '\n'))
            .chain('0'..='9')
            .chain([' '])
            .collect();
        Some(PdfFont::embed_invisible_subset(&mut doc, &chars)?)
    } else {
        None
    };

    for (index, slide) in slides {
        let scene = app
            .scene_for(index, false)
            .ok_or("Could not render slide")?;
        let size = if notes {
            let fit = (NOTE_WIDTH / slide_size.0).min((A4.1 * 0.42) / slide_size.1);
            (slide_size.0 * fit, slide_size.1 * fit)
        } else {
            slide_size
        };
        let image = slide_image(scene, size)?;
        let image = add_image(&mut doc, &mut image_bytes, image)?;
        if let (Some(measurer), Some(text_font)) = (&measurer, &text_font) {
            let lines = wrap_notes(&slide.notes, measurer)?;
            let number = (index + 1).to_string();
            let mut remaining = lines.as_slice();
            let mut first = true;
            loop {
                let top = if first {
                    MARGIN + size.1 + 28.0
                } else {
                    MARGIN
                };
                let capacity = ((A4.1 - MARGIN - top) / LINE_HEIGHT) as usize;
                let (shown, rest) = remaining.split_at(remaining.len().min(capacity));
                let (raster, text) = note_band(shown, &number, &font, text_font, top)?;
                let raster = add_image(&mut doc, &mut image_bytes, raster)?;
                let mut images = dictionary! {"Notes"=>raster};
                let mut content = String::new();
                if first {
                    images.set("Slide", image);
                    place_image(
                        &mut content,
                        "Slide",
                        (A4.0 - size.0) / 2.0,
                        A4.1 - MARGIN - size.1,
                        size,
                    );
                }
                let band_height = A4.1 - top - 24.0;
                place_image(
                    &mut content,
                    "Notes",
                    MARGIN,
                    24.0,
                    (NOTE_WIDTH, band_height),
                );
                content.push_str(&text);
                add_page(
                    &mut doc,
                    parent,
                    &mut kids,
                    A4,
                    dictionary! {"XObject"=>images, "Font"=>dictionary!{"F1"=>text_font.id}},
                    content,
                )?;
                if rest.is_empty() {
                    break;
                }
                first = false;
                remaining = rest;
            }
        } else {
            let mut content = String::new();
            place_image(&mut content, "Slide", 0.0, 0.0, size);
            add_page(
                &mut doc,
                parent,
                &mut kids,
                size,
                dictionary! {"XObject"=>dictionary!{"Slide"=>image}},
                content,
            )?;
        }
    }

    finish_document(doc, parent, kids)
}

fn finish_document(
    mut doc: Document,
    parent: ObjectId,
    kids: Vec<Object>,
) -> Result<Vec<u8>, String> {
    doc.objects.insert(
        parent,
        Object::Dictionary(dictionary! {"Type"=>"Pages", "Count"=>kids.len() as i64, "Kids"=>kids}),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>parent});
    let info = doc.add_object(dictionary! {"Creator"=>Object::string_literal("PPT42")});
    doc.trailer.set("Root", root);
    doc.trailer.set("Info", info);
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}

fn add_handouts(
    app: &Ppt,
    slide_size: (f64, f64),
    indices: &[usize],
    count: usize,
    doc: &mut Document,
    parent: ObjectId,
    kids: &mut Vec<Object>,
) -> Result<(), String> {
    let columns: usize = match count {
        1 | 2 => 1,
        3 | 4 | 6 => 2,
        9 => 3,
        _ => return Err("Invalid handout count".into()),
    };
    let rows = count.div_ceil(columns);
    let margin = 40.0;
    let gap = 24.0;
    let cell_width = (A4.0 - 2.0 * margin - (columns - 1) as f64 * gap) / columns as f64;
    let cell_height = (A4.1 - 2.0 * margin - (rows - 1) as f64 * gap) / rows as f64;
    let scale = (cell_width / slide_size.0).min(cell_height / slide_size.1);
    let width = slide_size.0 * scale;
    let height = slide_size.1 * scale;
    let mut image_bytes = 0;
    for page in indices.chunks(count) {
        let mut images = Dictionary::new();
        let mut content = String::new();
        for (slot, index) in page.iter().copied().enumerate() {
            let scene = app
                .scene_for(index, false)
                .ok_or("Could not render slide")?;
            let image = add_image(doc, &mut image_bytes, slide_image(scene, (width, height))?)?;
            let name = format!("Slide{slot}");
            let x =
                margin + (slot % columns) as f64 * (cell_width + gap) + (cell_width - width) / 2.0;
            let y = A4.1
                - margin
                - (slot / columns) as f64 * (cell_height + gap)
                - (cell_height + height) / 2.0;
            place_image(&mut content, &name, x, y, (width, height));
            let _ = writeln!(
                content,
                "q 0.6 G 0.5 w {x:.4} {y:.4} {width:.4} {height:.4} re S Q"
            );
            images.set(name, image);
        }
        add_page(
            doc,
            parent,
            kids,
            A4,
            dictionary! {"XObject"=>images},
            content,
        )?;
    }
    Ok(())
}

fn slide_image(scene: CanvasScene, size: (f64, f64)) -> Result<Stream, String> {
    let scale = SCALE.min(4000.0 / size.0.max(size.1));
    let width = (size.0 * scale).round().max(1.0) as u32;
    let height = (size.1 * scale).round().max(1.0) as u32;
    let pixels = win42::render_scene_rgb(scene, (720.0, 405.0), (width, height))?;
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 94)
        .encode(&pixels, width, height, image::ExtendedColorType::Rgb8)
        .map_err(|error| error.to_string())?;
    Ok(Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>i64::from(width),
            "Height"=>i64::from(height), "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8,
            "Filter"=>"DCTDecode",
        },
        bytes,
    ))
}

fn wrap_notes(text: &str, measurer: &SceneTextMeasurer) -> Result<Vec<(String, f64)>, String> {
    let mut lines = Vec::new();
    for paragraph in text.split(['\n', '\u{0085}', '\u{2028}', '\u{2029}']) {
        let paragraph = paragraph.replace('\t', "    ");
        let mut rest = paragraph.as_str();
        let limit = (NOTE_WIDTH * SCALE) as i32;
        loop {
            // Limit each GDI call even for a single long word or run of spaces.
            let mut units = 0;
            let sample_end = rest
                .grapheme_indices(true)
                .find_map(|(start, grapheme)| {
                    units += grapheme.encode_utf16().count();
                    (units > 2048 && start != 0).then_some(start)
                })
                .unwrap_or(rest.len());
            let sample = &rest[..sample_end];
            let (fit, _) = measurer.fit(sample, limit)?;
            let mut units = 0;
            let end = sample
                .grapheme_indices(true)
                .find_map(|(start, grapheme)| {
                    units += grapheme.encode_utf16().count();
                    (units > fit).then_some(start)
                })
                .unwrap_or(sample.len());
            if end == 0 && !rest.is_empty() {
                return Err("A speaker-note character is wider than the page".into());
            }
            let split = if end < rest.len() {
                // A boundary before the limit keeps whole words. Otherwise
                // split an oversized word at a complete grapheme.
                rest[..sample_end]
                    .split_word_bound_indices()
                    .map(|(i, _)| i)
                    .take_while(|&i| i <= end)
                    .filter(|&i| !rest[..i].trim().is_empty())
                    .last()
                    .unwrap_or(end)
            } else {
                end
            };
            let line = rest[..split].trim_end();
            let (_, pixels) = measurer.fit(line, i32::MAX)?;
            lines.push((line.to_owned(), f64::from(pixels) / SCALE));
            rest = rest[split..].trim_start();
            if rest.is_empty() {
                break;
            }
        }
    }
    Ok(lines)
}

fn note_band(
    lines: &[(String, f64)],
    number: &str,
    font: &SceneFont,
    text_font: &PdfFont,
    top: f64,
) -> Result<(Stream, String), String> {
    let height = A4.1 - top - 24.0;
    let mut scene = CanvasScene::default();
    let mut content = String::new();
    for (index, (line, width)) in lines
        .iter()
        .enumerate()
        .filter(|(_, (line, _))| !line.is_empty())
    {
        let y = index as f64 * LINE_HEIGHT;
        scene
            .items
            .push(note_text(line, y, [0; 3], TextAlign::Left, font));
        let mut hex = String::new();
        let mut advance = 0u64;
        for ch in line.chars() {
            let (cid, _, width) = text_font.chars.get(&ch).ok_or("Missing notes character")?;
            let _ = write!(hex, "{cid:04X}");
            advance += u64::from(*width);
        }
        let baseline = A4.1 - top - y - NOTE_SIZE;
        let scale = if advance == 0 {
            100.0
        } else {
            (width * 100_000.0 / (advance as f64 * NOTE_SIZE)).max(1.0)
        };
        let _ = writeln!(content,
            "BT /F1 {NOTE_SIZE} Tf {scale:.3} Tz 3 Tr 1 0 0 1 {MARGIN} {baseline:.3} Tm <{hex}> Tj ET");
    }
    scene.items.push(note_text(
        number,
        height - LINE_HEIGHT,
        [100; 3],
        TextAlign::Right,
        font,
    ));
    let pixels = ((NOTE_WIDTH * SCALE) as u32, (height * SCALE).round() as u32);
    let rgb = win42::render_scene_rgb(scene, (NOTE_WIDTH, height), pixels)?;
    let gray = rgb.chunks_exact(3).map(|pixel| pixel[0]).collect();
    let mut stream = Stream::new(
        dictionary! {
            "Type"=>"XObject", "Subtype"=>"Image", "Width"=>i64::from(pixels.0),
            "Height"=>i64::from(pixels.1), "ColorSpace"=>"DeviceGray", "BitsPerComponent"=>8,
        },
        gray,
    );
    stream.compress().map_err(|error| error.to_string())?;
    Ok((stream, content))
}

fn note_text(text: &str, y: f64, color: [u8; 3], align: TextAlign, font: &SceneFont) -> SceneItem {
    SceneItem::TextBox {
        x: 0.0,
        y,
        width: NOTE_WIDTH,
        height: LINE_HEIGHT,
        text: text.into(),
        color,
        font: font.clone(),
        align,
        wrap: false,
    }
}

fn add_image(doc: &mut Document, total: &mut usize, image: Stream) -> Result<ObjectId, String> {
    *total += image.content.len();
    if *total > MAX_IMAGE_BYTES {
        return Err("PDF export exceeds the file size limit".into());
    }
    Ok(doc.add_object(image))
}

fn place_image(commands: &mut String, name: &str, x: f64, y: f64, size: (f64, f64)) {
    let (width, height) = size;
    let _ = writeln!(
        commands,
        "q {width:.4} 0 0 {height:.4} {x:.4} {y:.4} cm /{name} Do Q"
    );
}

fn add_page(
    doc: &mut Document,
    parent: ObjectId,
    kids: &mut Vec<Object>,
    size: (f64, f64),
    resources: Dictionary,
    commands: String,
) -> Result<(), String> {
    if kids.len() >= MAX_PAGES {
        return Err("PDF export exceeds the page limit".into());
    }
    let content = doc.add_object(Stream::new(dictionary! {}, commands.into_bytes()));
    kids.push(Object::Reference(doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>parent, "Contents"=>content, "Resources"=>resources,
        "MediaBox"=>vec![0.into(), 0.into(), (size.0 as f32).into(), (size.1 as f32).into()],
    })));
    Ok(())
}
