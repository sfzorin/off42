//! Immutable OCR capture and one-transaction invisible text, retaining page content and geometry.
use super::*;
use lopdf::{dictionary, Stream};
use std::sync::Arc;
#[derive(Clone, Debug, PartialEq)]
pub struct Text {
    pub text: String,
    /// Unrotated PDF points in the page's media-box coordinate system.
    pub rect: [f64; 4],
}
#[derive(Clone, Debug, PartialEq)]
pub struct Page {
    pub index: usize,
    pub language: String,
    pub words: Vec<Text>,
}
#[derive(Clone, PartialEq)]
pub(super) struct Layer {
    language: String,
    words: Arc<[Text]>,
}
pub struct Capture {
    pub source: SaveState,
    /// Rendering copy uses unrotated media boxes and excludes editable annotations.
    pub bytes: Vec<u8>,
    pub pages: Vec<(usize, [f64; 4])>,
}
fn media_box(doc: &Document, page: ObjectId) -> Result<[f64; 4], String> {
    let mut id = page;
    let mut seen = HashSet::new();
    for _ in 0..128 {
        if !seen.insert(id) {
            return Err("Cyclic page geometry".into());
        }
        let node = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = node.get(b"MediaBox") {
            let array = doc
                .dereference(value)
                .map_err(|e| e.to_string())?
                .1
                .as_array()
                .map_err(|e| e.to_string())?;
            if array.len() != 4 {
                return Err("Invalid OCR media box".into());
            }
            let mut bounds = [0.0; 4];
            for (i, value) in array.iter().enumerate() {
                bounds[i] = match doc.dereference(value).map_err(|e| e.to_string())?.1 {
                    Object::Integer(n) => *n as f64,
                    Object::Real(n) => f64::from(*n),
                    _ => return Err("Invalid OCR media box".into()),
                };
            }
            if bounds.iter().any(|n| !n.is_finite())
                || bounds[2] <= bounds[0]
                || bounds[3] <= bounds[1]
                || !(bounds[2] - bounds[0]).is_finite()
                || !(bounds[3] - bounds[1]).is_finite()
            {
                return Err("Invalid OCR media box".into());
            }
            return Ok(bounds);
        }
        id = node
            .get(b"Parent")
            .map_err(|_| "Page has no media box")?
            .as_reference()
            .map_err(|e| e.to_string())?;
    }
    Err("Page geometry exceeds OCR limits".into())
}
impl PdfDocument {
    pub fn capture_ocr(&self, indices: &[usize]) -> Result<Capture, String> {
        if !self.allows_changes() {
            return Err("PDF permissions do not allow text recognition".into());
        }
        let selected: std::collections::BTreeSet<_> = indices.iter().copied().collect();
        if selected.is_empty() || selected.len() > MAX_PAGES {
            return Err("Choose at least one PDF page".into());
        }
        let (bytes, source) = self.save_snapshot()?;
        let mut rendering = Document::load_mem(&bytes).map_err(|e| e.to_string())?;
        let ids: Vec<_> = rendering.get_pages().into_values().collect();
        let mut pages = Vec::new();
        for index in selected {
            let id = *ids.get(index).ok_or("OCR page is unavailable")?;
            let bounds = media_box(&rendering, id)?;
            let page = rendering
                .get_dictionary_mut(id)
                .map_err(|e| e.to_string())?;
            page.set("Rotate", 0);
            page.set(
                "CropBox",
                bounds
                    .iter()
                    .map(|v| Object::Real(*v as f32))
                    .collect::<Vec<_>>(),
            );
            page.remove(b"Annots");
            pages.push((index, bounds));
        }
        let mut bytes = Vec::new();
        rendering.save_to(&mut bytes).map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 * 1024 {
            return Err("OCR rendering snapshot exceeds 64 MB; choose a smaller PDF".into());
        }
        Ok(Capture {
            source,
            bytes,
            pages,
        })
    }
    pub fn apply_ocr(&mut self, source: &SaveState, pages: Vec<Page>) -> Result<usize, String> {
        if !self.matches_snapshot(source) {
            return Err("The PDF changed during recognition; no pages changed".into());
        }
        if !self.allows_changes() {
            return Err("PDF permissions do not allow text recognition".into());
        }
        if pages.is_empty() || pages.len() > MAX_PAGES {
            return Err("No OCR pages returned".into());
        }
        let mut state = self.history.current().clone();
        let mut seen = HashSet::new();
        let mut changed = 0;
        for page in pages {
            if !seen.insert(page.index) {
                return Err("Duplicate OCR page".into());
            }
            let id = *state
                .pages
                .get(page.index)
                .ok_or("OCR page is unavailable")?;
            let bounds = if let Some(r) = state.raster_pages.get(&id) {
                [0., 0., r.page_width, r.page_height]
            } else {
                media_box(&self.doc, id)?
            };
            if page.language.is_empty()
                || page.language.len() > 256
                || !page
                    .language
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+'))
            {
                return Err("Invalid OCR language".into());
            }
            for word in &page.words {
                let b = word.rect;
                if word.text.is_empty()
                    || word.text.len() > 4096
                    || word.text.chars().any(char::is_control)
                    || b.iter().any(|n| !n.is_finite())
                    || b[2] <= b[0]
                    || b[3] <= b[1]
                    || b[0] < bounds[0] - 0.01
                    || b[1] < bounds[1] - 0.01
                    || b[2] > bounds[2] + 0.01
                    || b[3] > bounds[3] + 0.01
                {
                    return Err("Invalid OCR text or geometry".into());
                }
            }
            if !page.words.is_empty() {
                state.ocr_layers.insert(
                    id,
                    Layer {
                        language: page.language,
                        words: page.words.into(),
                    },
                );
                changed += 1;
            }
        }
        let (mut count, mut text) = (0usize, 0usize);
        for layer in state.ocr_layers.values() {
            count = count
                .checked_add(layer.words.len())
                .ok_or("OCR output exceeds limits")?;
            for word in layer.words.iter() {
                text = text
                    .checked_add(word.text.len())
                    .ok_or("OCR output exceeds limits")?;
            }
        }
        if count > 100_000 || text > 16 * 1024 * 1024 {
            return Err("OCR output exceeds limits; recognize a smaller selection".into());
        }
        if changed == 0 {
            return Ok(0);
        }
        let candidate = Self {
            source_identity: self.source_identity,
            doc: self.doc.clone(),
            root: self.root,
            original_pages: self.original_pages.clone(),
            history: SnapshotHistory::new(state.clone(), 0),
        };
        candidate.save_snapshot()?;
        self.history.apply(state);
        Ok(changed)
    }
    pub(super) fn bake_ocr(&self, doc: &mut Document, state: &State) -> Result<(), String> {
        let all = state
            .pages
            .iter()
            .filter_map(|p| state.ocr_layers.get(p))
            .flat_map(|l| l.words.iter().map(|w| w.text.as_str()))
            .collect::<String>();
        if all.is_empty() {
            return Ok(());
        }
        if matches!(
            doc.version.as_str(),
            "1.0" | "1.1" | "1.2" | "1.3" | "1.4" | "1.5" | "1.6"
        ) {
            doc.version = "1.7".into();
        }
        let font = suite42_pdf_text::PdfFont::embed_invisible_subset(doc, &all)?;
        for &id in &state.pages {
            let Some(layer) = state.ocr_layers.get(&id) else {
                continue;
            };
            let bounds = media_box(doc, id)?;
            let mut content = String::from("q\n");
            for word in layer.words.iter() {
                let [left, bottom, right, top] = word.rect;
                let size = ((top - bottom) * 0.8).max(0.1);
                let advance = word
                    .text
                    .chars()
                    .map(|c| u64::from(font.chars[&c].2))
                    .sum::<u64>() as f64
                    * size
                    / 1000.;
                if advance <= 0.0 {
                    return Err("Invalid OCR text advance".into());
                }
                let scale = (right - left) / advance * 100.;
                let hex = font.encode(&word.text)?;
                content.push_str(&format!(
                    "BT /Text {size} Tf {scale} Tz 3 Tr 1 0 0 1 {left} {} Tm <{hex}> Tj ET\n",
                    bottom + (top - bottom) * 0.18
                ));
            }
            content.push_str("Q\n");
            let form=doc.add_object(Stream::new(dictionary!{"Type"=>"XObject","Subtype"=>"Form","Off42OCR"=>true,"Off42OCRVersion"=>1,"Lang"=>Object::string_literal(layer.language.as_bytes()),"BBox"=>bounds.iter().map(|v|Object::Real(*v as f32)).collect::<Vec<_>>(),"Resources"=>dictionary!{"Font"=>dictionary!{"Text"=>font.id}}},content.into_bytes()));
            let mut resources = decorations::inherited_resources(doc, id)?;
            let mut objects = resources
                .get(b"XObject")
                .ok()
                .map(|v| {
                    doc.dereference(v)
                        .map_err(|e| e.to_string())
                        .and_then(|(_, v)| v.as_dict().cloned().map_err(|e| e.to_string()))
                })
                .transpose()?
                .unwrap_or_default();
            let mut name = format!("Off42OCR{}_{}", form.0, form.1);
            while objects.get(name.as_bytes()).is_ok() {
                name.push('_');
            }
            objects.set(name.as_bytes(), form);
            let before = doc.add_object(Stream::new(
                dictionary! {"Off42OCRVersion"=>1,"Off42OCRPart"=>"Before"},
                b"q\n".to_vec(),
            ));
            let after=doc.add_object(Stream::new(dictionary!{"Off42OCRVersion"=>1,"Off42OCRPart"=>"After","Off42OCRName"=>Object::Name(name.as_bytes().to_vec())},format!("Q\nq /{name} Do Q\n").into_bytes()));
            let old = doc
                .get_dictionary(id)
                .map_err(|e| e.to_string())?
                .get(b"Contents")
                .ok()
                .cloned();
            let mut contents = vec![Object::Reference(before)];
            if let Some(old) = old {
                match doc.dereference(&old).map_err(|e| e.to_string())?.1 {
                    Object::Array(items) => {
                        contents.extend(strip_owned(doc, &mut objects, items.clone())?)
                    }
                    Object::Stream(_) => {
                        contents.extend(strip_owned(doc, &mut objects, vec![old])?)
                    }
                    _ => return Err("Invalid OCR page content".into()),
                }
            }
            contents.push(Object::Reference(after));
            let page = doc.get_dictionary_mut(id).map_err(|e| e.to_string())?;
            resources.set("XObject", objects);
            page.set("Resources", resources);
            page.set("Contents", contents);
        }
        Ok(())
    }
}

/// Remove only complete, byte-validated wrappers produced by this implementation; foreign text stays intact.
fn strip_owned(
    doc: &Document,
    objects: &mut lopdf::Dictionary,
    items: Vec<Object>,
) -> Result<Vec<Object>, String> {
    let (mut before, mut after) = (0usize, 0usize);
    let mut keep = Vec::new();
    let mut remove = Vec::new();
    for (item_index, item) in items.iter().enumerate() {
        let Ok((_, Object::Stream(stream))) = doc.dereference(item) else {
            continue;
        };
        if stream
            .dict
            .get(b"Off42OCRVersion")
            .ok()
            .and_then(|v| v.as_i64().ok())
            != Some(1)
        {
            continue;
        }
        let part = stream
            .dict
            .get(b"Off42OCRPart")
            .ok()
            .and_then(|v| v.as_name().ok());
        let data = if stream.dict.get(b"Filter").is_err() {
            (stream.content.len() <= 4096).then(|| stream.content.clone())
        } else {
            stream.decompressed_content_with_limit(4096).ok()
        };
        if part == Some(b"Before") && data.as_deref() == Some(b"q\n" as &[u8]) {
            before += 1;
            keep.push(item_index);
        } else if part == Some(b"After") {
            let Some(name) = stream
                .dict
                .get(b"Off42OCRName")
                .ok()
                .and_then(|v| v.as_name().ok())
            else {
                return Ok(items);
            };
            if name.len() > 512 || !name.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_') {
                return Ok(items);
            }
            let expected = format!("Q\nq /{} Do Q\n", String::from_utf8_lossy(name));
            if data.as_deref() != Some(expected.as_bytes()) {
                return Ok(items);
            }
            let Some(form) = objects
                .get(name)
                .ok()
                .and_then(|v| doc.dereference(v).ok())
                .and_then(|(_, v)| v.as_stream().ok())
            else {
                return Ok(items);
            };
            if form
                .dict
                .get(b"Off42OCR")
                .ok()
                .and_then(|v| v.as_bool().ok())
                != Some(true)
                || form
                    .dict
                    .get(b"Off42OCRVersion")
                    .ok()
                    .and_then(|v| v.as_i64().ok())
                    != Some(1)
            {
                return Ok(items);
            }
            after += 1;
            keep.push(item_index);
            remove.push(name.to_vec());
        }
    }
    if before == 0 || before != after {
        return Ok(items);
    }
    for name in remove {
        objects.remove(&name);
    }
    let discard: HashSet<_> = keep.into_iter().collect();
    Ok(items
        .into_iter()
        .enumerate()
        .filter_map(|(i, v)| (!discard.contains(&i)).then_some(v))
        .collect())
}
#[cfg(test)]
mod tests;
