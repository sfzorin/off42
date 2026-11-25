//! A single reusable vector PDF page, fitted into stock cells without rasterization.
use super::{pdf_num, Stock, A4, MAX_LABELS, PT_PER_MM};
use lopdf::{dictionary, Dictionary, Document, LoadOptions, Object, ObjectId, Stream};
use std::fmt::Write;
const MAX_BYTES: usize = 64 * 1024 * 1024;
fn inherited(doc: &Document, mut id: ObjectId, key: &[u8]) -> Result<Option<Object>, String> {
    let mut seen = std::collections::HashSet::new();
    for _ in 0..128 {
        if !seen.insert(id) {
            return Err("Cyclic PDF page inheritance".into());
        }
        let node = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = node.get(key) {
            return Ok(Some(
                doc.dereference(value).map_err(|e| e.to_string())?.1.clone(),
            ));
        }
        let Ok(parent) = node.get(b"Parent") else {
            return Ok(None);
        };
        id = parent.as_reference().map_err(|e| e.to_string())?;
    }
    Err("PDF page inheritance exceeds its limit".into())
}
fn bounds(doc: &Document, value: &Object) -> Result<[f64; 4], String> {
    let array = value.as_array().map_err(|e| e.to_string())?;
    if array.len() != 4 {
        return Err("Invalid PDF artwork bounds".into());
    }
    let mut out = [0.; 4];
    for (i, item) in array.iter().enumerate() {
        out[i] = match doc.dereference(item).map_err(|e| e.to_string())?.1 {
            Object::Integer(n) => *n as f64,
            Object::Real(n) => f64::from(*n),
            _ => return Err("Invalid PDF artwork bounds".into()),
        };
    }
    if out.iter().any(|n| !n.is_finite() || n.abs() > 10_000_000.)
        || out[2] - out[0] < 0.001
        || out[3] - out[1] < 0.001
    {
        return Err("Invalid PDF artwork bounds".into());
    }
    Ok(out)
}
fn contents(doc: &Document, value: &Object, out: &mut Vec<u8>, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("PDF artwork contents exceed nesting limits".into());
    }
    match doc.dereference(value).map_err(|e| e.to_string())?.1 {
        Object::Array(items) => {
            for value in items {
                contents(doc, value, out, depth + 1)?;
            }
        }
        Object::Stream(stream) => {
            let remaining = MAX_BYTES.saturating_sub(out.len() + 1);
            let bytes = if stream.dict.get(b"Filter").is_ok() {
                stream
                    .decompressed_content_with_limit(remaining)
                    .map_err(|e| e.to_string())?
            } else {
                if stream.content.len() > remaining {
                    return Err("PDF artwork contents exceed 64 MB".into());
                }
                stream.content.clone()
            };
            out.extend_from_slice(&bytes);
            out.push(b'\n');
        }
        Object::Null => {}
        _ => return Err("Invalid PDF artwork contents".into()),
    }
    Ok(())
}
fn reusable_annotations(doc: &Document, page: ObjectId) -> Result<(), String> {
    let node = doc.get_dictionary(page).map_err(|e| e.to_string())?;
    let Ok(value) = node.get(b"Annots") else {
        return Ok(());
    };
    let items = doc
        .dereference(value)
        .map_err(|e| e.to_string())?
        .1
        .as_array()
        .map_err(|e| e.to_string())?;
    for value in items {
        let node = doc
            .dereference(value)
            .map_err(|e| e.to_string())?
            .1
            .as_dict()
            .map_err(|e| e.to_string())?;
        let kind = node.get(b"Subtype").ok().and_then(|v| v.as_name().ok());
        let flags = node
            .get(b"F")
            .ok()
            .and_then(|v| v.as_i64().ok())
            .unwrap_or(0);
        if matches!(kind, Some(b"Link" | b"Popup")) || flags & (1 | 2 | 32) != 0 {
            continue;
        }
        return Err("Flatten visible PDF annotations before creating vector label artwork".into());
    }
    Ok(())
}
/// `region` is an optional left/bottom/right/top rectangle in unrotated PDF points.
/// The page's rotation and visible crop are retained in the fitted appearance.
pub fn sheet_pdf_artwork_stock(
    stock: &Stock,
    first_slot: usize,
    copies: usize,
    bytes: &[u8],
    region: Option<[f64; 4]>,
) -> Result<Vec<u8>, String> {
    stock.validate()?;
    let capacity = stock.columns * stock.rows;
    if !(1..=capacity).contains(&first_slot) || !(1..=MAX_LABELS).contains(&copies) {
        return Err("Invalid label copy count or first slot".into());
    }
    let count = (copies + first_slot - 1).div_ceil(capacity);
    if count > 500 {
        return Err("Label output exceeds 500 pages".into());
    }
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("PDF artwork exceeds 64 MB".into());
    }
    let mut doc = Document::load_mem_with_options(
        bytes,
        LoadOptions {
            max_decompressed_size: Some(MAX_BYTES),
            ..Default::default()
        },
    )
    .map_err(|e| e.to_string())?;
    if doc.is_encrypted() || doc.objects.len() > 100_000 {
        return Err("PDF artwork is encrypted or exceeds its object limit".into());
    }
    let pages = doc.get_pages();
    if pages.len() != 1 {
        return Err("Choose a single PDF page for label artwork".into());
    }
    let page = pages[&1];
    reusable_annotations(&doc, page)?;
    let media = bounds(
        &doc,
        &inherited(&doc, page, b"MediaBox")?.ok_or("PDF artwork has no media box")?,
    )?;
    let crop = inherited(&doc, page, b"CropBox")?
        .map(|v| bounds(&doc, &v))
        .transpose()?
        .unwrap_or(media);
    let mut area = [
        media[0].max(crop[0]),
        media[1].max(crop[1]),
        media[2].min(crop[2]),
        media[3].min(crop[3]),
    ];
    if let Some(region) = region {
        if region.iter().any(|n| !n.is_finite()) || region[2] <= region[0] || region[3] <= region[1]
        {
            return Err("Choose a nonempty PDF artwork region".into());
        }
        area = [
            area[0].max(region[0]),
            area[1].max(region[1]),
            area[2].min(region[2]),
            area[3].min(region[3]),
        ];
    }
    let [left, bottom, right, top] = area;
    let (w, h) = (right - left, top - bottom);
    if w < 0.001 || h < 0.001 {
        return Err("Choose a region inside the visible PDF page".into());
    }
    let rotation = inherited(&doc, page, b"Rotate")?
        .map(|v| v.as_i64().map_err(|e| e.to_string()))
        .transpose()?
        .unwrap_or(0);
    if rotation % 90 != 0 {
        return Err("Unsupported PDF artwork rotation".into());
    }
    let (matrix, display) = match rotation.rem_euclid(360) {
        0 => ([1., 0., 0., 1., -left, -bottom], (w, h)),
        90 => ([0., -1., 1., 0., -bottom, right], (h, w)),
        180 => ([-1., 0., 0., -1., right, top], (w, h)),
        _ => ([0., 1., -1., 0., top, -left], (h, w)),
    };
    let mut body = b"q\n".to_vec();
    if let Ok(value) = doc
        .get_dictionary(page)
        .map_err(|e| e.to_string())?
        .get(b"Contents")
    {
        contents(&doc, value, &mut body, 0)?;
    }
    body.extend_from_slice(b"Q\n");
    let resources = inherited(&doc, page, b"Resources")?
        .unwrap_or_else(|| Object::Dictionary(Dictionary::new()));
    resources.as_dict().map_err(|e| e.to_string())?;
    let mut form = dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>area.into_iter().map(pdf_num).collect::<Vec<_>>(),"Matrix"=>matrix.into_iter().map(pdf_num).collect::<Vec<_>>(),"Resources"=>resources};
    if let Ok(group) = doc
        .get_dictionary(page)
        .map_err(|e| e.to_string())?
        .get(b"Group")
    {
        form.set("Group", group.clone());
    }
    let artwork = doc.add_object(Stream::new(form, body));
    let width = stock.width * PT_PER_MM;
    let height = stock.height * PT_PER_MM;
    let scale = (width / display.0).min(height / display.1);
    let (dx, dy) = (
        (width - display.0 * scale) / 2.,
        (height - display.1 * scale) / 2.,
    );
    let cell=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![pdf_num(0.),pdf_num(0.),pdf_num(width),pdf_num(height)],"Resources"=>dictionary!{"XObject"=>dictionary!{"Artwork"=>artwork}}},format!("q 0 0 {width} {height} re W n {scale} 0 0 {scale} {dx} {dy} cm /Artwork Do Q\n").into_bytes()));
    let root = doc.new_object_id();
    let mut kids = Vec::new();
    let mut slot = first_slot - 1;
    for _ in 0..count {
        let mut commands = String::new();
        let page_index = kids.len();
        while slot < first_slot - 1 + copies && slot / capacity == page_index {
            let local = slot % capacity;
            let x = (stock.margin_left
                + (local % stock.columns) as f64 * (stock.width + stock.gap_x))
                * PT_PER_MM;
            let y = (A4[1]
                - stock.margin_top
                - (local / stock.columns) as f64 * (stock.height + stock.gap_y)
                - stock.height)
                * PT_PER_MM;
            writeln!(commands, "q 1 0 0 1 {x} {y} cm /Label Do Q").unwrap();
            slot += 1;
        }
        let content = doc.add_object(Stream::new(dictionary! {}, commands.into_bytes()));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"Resources"=>dictionary!{"XObject"=>dictionary!{"Label"=>cell}},"Contents"=>content});
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>count as i64,"Kids"=>kids,"MediaBox"=>vec![pdf_num(0.),pdf_num(0.),pdf_num(A4[0]*PT_PER_MM),pdf_num(A4[1]*PT_PER_MM)]}));
    let mut catalog = dictionary! {"Type"=>"Catalog","Pages"=>root,"ViewerPreferences"=>dictionary!{"PrintScaling"=>"None"}};
    for key in [b"OCProperties" as &[u8], b"Extensions"] {
        if let Ok(value) = doc.catalog().map_err(|e| e.to_string())?.get(key) {
            catalog.set(key, value.clone());
        }
    }
    let catalog = doc.add_object(catalog);
    doc.trailer = Dictionary::new();
    doc.trailer.set("Root", catalog);
    if doc.version.starts_with("1.") {
        doc.version = "1.7".into();
    }
    doc.prune_objects();
    doc.compress();
    let mut output = Vec::new();
    doc.save_to(&mut output).map_err(|e| e.to_string())?;
    if output.len() > 128 * 1024 * 1024 {
        return Err("Label artwork output exceeds 128 MB".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests;

/// Wrap an existing bitmap in a one-page PDF with the document's displayed dimensions.
/// Pixel alpha is composited on white, matching the label sheet's paper background.
pub fn bitmap_artwork_pdf(
    width: u32,
    height: u32,
    rgba: &[u8],
    points: (f64, f64),
) -> Result<Vec<u8>, String> {
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || pixels > 16_000_000
        || rgba.len() as u64 != pixels * 4
        || ![points.0, points.1]
            .iter()
            .all(|v| v.is_finite() && *v >= 0.001 && *v <= 14_400.)
    {
        return Err("Invalid or oversized picture artwork".into());
    }
    let mut rgb = Vec::with_capacity(pixels as usize * 3);
    for p in rgba.chunks_exact(4) {
        let alpha = u32::from(p[3]);
        for &value in &p[..3] {
            rgb.push(((u32::from(value) * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let image=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Image","Width"=>width as i64,"Height"=>height as i64,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8,"Interpolate"=>false},rgb));
    let content = doc.add_object(Stream::new(
        dictionary! {},
        format!("q {} 0 0 {} 0 0 cm /Picture Do Q\n", points.0, points.1).into_bytes(),
    ));
    let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![pdf_num(0.),pdf_num(0.),pdf_num(points.0),pdf_num(points.1)],"Resources"=>dictionary!{"XObject"=>dictionary!{"Picture"=>image}},"Contents"=>content});
    doc.objects.insert(
        root,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages","Kids"=>vec![Object::Reference(page)],"Count"=>1},
        ),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    doc.trailer.set("Root", catalog);
    doc.compress();
    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}
