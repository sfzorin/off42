//! Live page decorations, baked as isolated vector/text page content in a save snapshot.
use super::*;
use lopdf::{dictionary, Dictionary, Stream};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecorationKind {
    Watermark,
    PageNumber,
    HeaderFooter,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Watermark {
    pub text: String,
    pub image: Option<ImageStamp>,
    pub size: f64,
    pub color: [u8; 3],
    pub opacity: f64,
    pub degrees: f64,
}
impl Default for Watermark {
    fn default() -> Self {
        Self {
            text: "CONFIDENTIAL".into(),
            image: None,
            size: 72.0,
            color: [210, 25, 35],
            opacity: 0.25,
            degrees: 45.0,
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct HeaderFooter {
    /// Top left/center/right, bottom left/center/right. Tokens match the macOS workflow.
    pub texts: [String; 6],
    pub size: f64,
    pub font:Option<suite42_pdf_text::FontProgram>,
    pub color: [u8; 3],
    pub horizontal: f64,
    pub vertical: f64,
    pub bates_prefix: String,
    pub bates_suffix: String,
    pub bates_digits: usize,
    pub bates_start: u64,
    pub date: String,
    pub file_name: String,
}
impl Default for HeaderFooter {
    fn default() -> Self {
        Self {
            texts: [
                String::new(),
                String::new(),
                String::new(),
                String::new(),
                "<<page>> / <<pages>>".into(),
                String::new(),
            ],
            size: 10.0,
            font:None,
            color: [0, 0, 0],
            horizontal: 36.0,
            vertical: 28.0,
            bates_prefix: String::new(),
            bates_suffix: String::new(),
            bates_digits: 6,
            bates_start: 1,
            date: String::new(),
            file_name: String::new(),
        }
    }
}
#[derive(Clone, Debug, PartialEq)]
pub struct PageNumbers {
    pub slot: usize,
    pub start: u64,
    pub skip_first: bool,
    pub format: NumberFormat,
    pub size: f64,
    pub margin: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NumberFormat {
    Plain,
    PageOfTotal,
    Dashes,
}
#[derive(Clone, PartialEq)]
pub(super) enum Content {
    Watermark(Watermark),
    Text {
        kind: DecorationKind,
        lines: [String; 6],
        size: f64,
        font:Option<suite42_pdf_text::FontProgram>,
        color: [u8; 3],
        horizontal: f64,
        vertical: f64,
    },
}
#[derive(Clone, PartialEq)]
pub(super) struct Decoration {
    pub page: ObjectId,
    content: Content,
}
impl Decoration {
    fn kind(&self) -> DecorationKind {
        match &self.content {
            Content::Watermark(_) => DecorationKind::Watermark,
            Content::Text { kind, .. } => *kind,
        }
    }
}
fn size(value: f64) -> bool {
    value.is_finite() && (1.0..=512.0).contains(&value)
}
fn margin(value: f64) -> bool {
    value.is_finite() && (0.0..=1440.0).contains(&value)
}
fn check_line(text: &str) -> Result<(), String> {check_line_font(text,None)}
fn check_line_font(text:&str,font:Option<&suite42_pdf_text::FontProgram>)->Result<(),String>{
    if text.len() > 4096 || text.chars().any(|c| c.is_control()) {
        return Err("Use a single line up to 4096 bytes".into());
    }
    if let Some(font)=font{font.validate(text)}else{suite42_pdf_text::PdfFont::validate(text)}
}
impl PdfDocument {
    fn decoration_pages(&self, indices: &[usize]) -> Result<Vec<(usize, ObjectId)>, String> {
        if !self.allows_changes() {
            return Err("PDF permissions do not allow page changes".into());
        }
        if indices.is_empty() || indices.len() > MAX_PAGES {
            return Err("Choose at least one page".into());
        }
        let mut indices = indices.to_vec();
        indices.sort_unstable();
        if indices.windows(2).any(|w| w[0] == w[1]) {
            return Err("A page may be selected only once".into());
        }
        indices
            .into_iter()
            .map(|i| {
                let id = *self
                    .history
                    .current()
                    .pages
                    .get(i)
                    .ok_or("Page index out of range")?;
                self.page_box(i)?;
                self.rotation(i)?;
                inherited_resources(&self.doc, id)?;
                Ok((i, id))
            })
            .collect()
    }

    fn append_decorations(&mut self, added: Vec<Decoration>) -> Result<(), String> {
        let mut state = self.history.current().clone();
        if state.decorations.len() + added.len() > 6000 {
            return Err("Too many page decorations".into());
        }
        let bytes = state
            .decorations
            .iter()
            .chain(&added)
            .map(|d| match &d.content {
                Content::Watermark(w) => w.text.len(),
                Content::Text { lines, .. } => lines.iter().map(String::len).sum(),
            })
            .sum::<usize>();
        if bytes > 8 * 1024 * 1024 {
            return Err("Page decoration text exceeds limits".into());
        }
        let mut fonts=std::collections::BTreeSet::new();let mut font_bytes=0usize;
        for decoration in state.decorations.iter().chain(&added){
            if let Content::Text{font:Some(font),..}=&decoration.content{if fonts.insert(font.cache_id()){font_bytes=font_bytes.checked_add(font.byte_len()).ok_or("Font data exceeds limits")?;}}
        }
        if font_bytes>32*1024*1024{return Err("Page decoration font data exceeds limits".into())}
        state.decorations.extend(added);
        self.history.apply(state);
        Ok(())
    }
    pub fn add_watermark(&mut self, indices: &[usize], spec: Watermark) -> Result<(), String> {
        if !size(spec.size)
            || !spec.opacity.is_finite()
            || !(0.0..=1.0).contains(&spec.opacity)
            || !spec.degrees.is_finite()
        {
            return Err("Invalid watermark size, opacity or rotation".into());
        }
        if let Some(image) = &spec.image {
            let pixels = u64::from(image.width) * u64::from(image.height);
            if image.width == 0
                || image.height == 0
                || pixels > 4_000_000
                || image.rgba.len() as u64 != pixels * 4
            {
                return Err("Invalid watermark image".into());
            }
        } else {
            if spec.text.trim().is_empty() {
                return Err("Enter watermark text".into());
            }
            check_line(&spec.text)?;
        }
        let added = self
            .decoration_pages(indices)?
            .into_iter()
            .map(|(_, page)| Decoration {
                page,
                content: Content::Watermark(spec.clone()),
            })
            .collect();
        self.append_decorations(added)
    }
    /// Resolves tokens now, in document order. Reordering retains each page's assigned number.
    pub fn add_header_footer(
        &mut self,
        indices: &[usize],
        spec: &HeaderFooter,
    ) -> Result<u64, String> {
        if !size(spec.size)
            || !margin(spec.horizontal)
            || !margin(spec.vertical)
            || spec.bates_digits > 12
            || spec.texts.iter().all(|s| s.trim().is_empty())
        {
            return Err("Invalid header/footer settings".into());
        }
        let pages = self.decoration_pages(indices)?;
        let next = spec
            .bates_start
            .checked_add(pages.len() as u64)
            .ok_or("Bates number overflow")?;
        let labels = self.page_labels()?;
        let mut added = Vec::new();
        for (offset, (i, page)) in pages.into_iter().enumerate() {
            let number = spec.bates_start + offset as u64;
            let bates = format!(
                "{}{:0width$}{}",
                spec.bates_prefix,
                number,
                spec.bates_suffix,
                width = spec.bates_digits
            );
            let label = labels
                .as_ref()
                .and_then(|l| l.get(i))
                .cloned()
                .unwrap_or_else(|| (i + 1).to_string());
            let lines = spec.texts.clone().map(|text| {
                text.replace("<<page>>", &(i + 1).to_string())
                    .replace("<<pages>>", &self.page_count().to_string())
                    .replace("<<label>>", &label)
                    .replace("<<date>>", &spec.date)
                    .replace("<<file>>", &spec.file_name)
                    .replace("<<bates>>", &bates)
            });
            for line in &lines {
                check_line_font(line,spec.font.as_ref())?;
            }
            added.push(Decoration {
                page,
                content: Content::Text {
                    kind: DecorationKind::HeaderFooter,
                    lines,
                    size: spec.size,
                    font:spec.font.clone(),
                    color: spec.color,
                    horizontal: spec.horizontal,
                    vertical: spec.vertical,
                },
            });
        }
        self.append_decorations(added)?;
        Ok(next)
    }
    pub fn add_page_numbers(
        &mut self,
        indices: &[usize],
        spec: &PageNumbers,
    ) -> Result<(), String> {
        if spec.slot >= 6 || !size(spec.size) || !margin(spec.margin) {
            return Err("Invalid page number settings".into());
        }
        let mut pages = self.decoration_pages(indices)?;
        if spec.skip_first {
            pages.remove(0);
        }
        if pages.is_empty() {
            return Ok(());
        }
        let total = spec
            .start
            .checked_add(pages.len() as u64 - 1)
            .ok_or("Page number overflow")?;
        let added = pages
            .into_iter()
            .enumerate()
            .map(|(offset, (_, page))| {
                let n = spec.start + offset as u64;
                let text = match spec.format {
                    NumberFormat::Plain => n.to_string(),
                    NumberFormat::PageOfTotal => format!("Page {n} of {total}"),
                    NumberFormat::Dashes => format!("- {n} -"),
                };
                let mut lines = std::array::from_fn(|_| String::new());
                lines[spec.slot] = text;
                Decoration {
                    page,
                    content: Content::Text {
                        kind: DecorationKind::PageNumber,
                        lines,
                        size: spec.size,
                        font:None,
                        color: [0, 0, 0],
                        horizontal: spec.margin,
                        vertical: spec.margin,
                    },
                }
            })
            .collect();
        self.append_decorations(added)
    }
    pub fn has_decorations(&self, kind: DecorationKind) -> bool {
        self.history
            .current()
            .decorations
            .iter()
            .any(|d| d.kind() == kind && self.history.current().pages.contains(&d.page))
    }
    pub fn remove_decorations(&mut self, kinds: &[DecorationKind]) {
        let mut state = self.history.current().clone();
        state.decorations.retain(|d| !kinds.contains(&d.kind()));
        self.history.apply(state);
    }
    pub(super) fn bake_decorations(&self, doc: &mut Document, state: &State) -> Result<(), String> {
        let mut watermark_forms: Vec<(Watermark, f64, f64, ObjectId)> = Vec::new();
        let mut generated_bytes = 0usize;
        let mut fonts = HashMap::new();
        let mut images = HashMap::new();
        for &page in &state.pages {
            let overlays = state
                .decorations
                .iter()
                .filter(|d| d.page == page)
                .collect::<Vec<_>>();
            if overlays.is_empty() {
                continue;
            }
            let index = state.pages.iter().position(|p| *p == page).unwrap();
            let b = self.page_box(index)?;
            let rotation = self.rotation(index)?;
            let width = b[2] - b[0];
            let height = b[3] - b[1];
            let (w, h) = if rotation == 90 || rotation == 270 {
                (height, width)
            } else {
                (width, height)
            };
            let matrix = match rotation {
                90 => [0.0, 1.0, -1.0, 0.0, b[2], b[1]],
                180 => [-1.0, 0.0, 0.0, -1.0, b[2], b[3]],
                270 => [0.0, -1.0, 1.0, 0.0, b[0], b[3]],
                _ => [1.0, 0.0, 0.0, 1.0, b[0], b[1]],
            };
            let mut resources = inherited_resources(doc, page)?;
            let mut xobjects = resources
                .get(b"XObject")
                .ok()
                .map(|v| {
                    doc.dereference(v)
                        .map_err(|e| e.to_string())
                        .and_then(|(_, v)| v.as_dict().map(Clone::clone).map_err(|e| e.to_string()))
                })
                .transpose()?
                .unwrap_or_default();
            let mut page_drawing = String::new();
            for overlay in overlays {
                let cached = match &overlay.content {
                    Content::Watermark(spec) => watermark_forms
                        .iter()
                        .find(|(s, fw, fh, _)| s == spec && *fw == w && *fh == h)
                        .map(|(_, _, _, id)| *id),
                    _ => None,
                };
                let form = if let Some(id) = cached {
                    id
                } else {
                    let (content, res) =
                        drawing(doc, &overlay.content, w, h, &mut fonts, &mut images)?;
                    generated_bytes = generated_bytes
                        .checked_add(content.len())
                        .filter(|n| *n <= 64 * 1024 * 1024)
                        .ok_or("Page decoration output exceeds limits")?;
                    let id = doc.add_object(Stream::new(dictionary! {"Type"=>"XObject","Subtype"=>"Form","BBox"=>vec![0.into(),0.into(),Object::Real(w as f32),Object::Real(h as f32)],"Resources"=>res}, content.into_bytes()));
                    if let Content::Watermark(spec) = &overlay.content {
                        watermark_forms.push((spec.clone(), w, h, id));
                    }
                    id
                };
                let mut name = format!("Off42Decoration{}_{}", form.0, form.1);
                while xobjects.get(name.as_bytes()).is_ok() {
                    name.push('_');
                }
                xobjects.set(name.as_bytes(), form);
                page_drawing.push_str(&format!(
                    "q {} {} {} {} {} {} cm /{name} Do Q\n",
                    matrix[0], matrix[1], matrix[2], matrix[3], matrix[4], matrix[5]
                ));
            }
            resources.set("XObject", xobjects);
            let before = doc.add_object(Stream::new(dictionary! {}, b"q\n".to_vec()));
            let after = doc.add_object(Stream::new(
                dictionary! {},
                format!("Q\n{page_drawing}").into_bytes(),
            ));
            let old = doc
                .get_dictionary(page)
                .map_err(|e| e.to_string())?
                .get(b"Contents")
                .ok()
                .cloned();
            let mut contents = vec![Object::Reference(before)];
            if let Some(old) = old {
                match doc.dereference(&old).map_err(|e| e.to_string())?.1 {
                    Object::Array(items) => contents.extend(items.clone()),
                    Object::Stream(_) => contents.push(old),
                    _ => return Err("Invalid page content".into()),
                }
            }
            contents.push(Object::Reference(after));
            let page = doc.get_dictionary_mut(page).map_err(|e| e.to_string())?;
            page.set("Resources", resources);
            page.set("Contents", contents);
        }
        Ok(())
    }
}
fn image_resource(doc: &mut Document, image: &ImageStamp) -> Result<ObjectId, String> {
    let mut rgb = Vec::with_capacity(image.rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(image.rgba.len() / 4);
    for p in image.rgba.chunks_exact(4) {
        rgb.extend_from_slice(&p[..3]);
        alpha.push(p[3]);
    }
    let mut mask = Stream::new(
        dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>image.width as i64,"Height"=>image.height as i64,"ColorSpace"=>"DeviceGray","BitsPerComponent"=>8},
        alpha,
    );
    mask.compress().map_err(|e| e.to_string())?;
    let mask = doc.add_object(mask);
    let mut stream = Stream::new(
        dictionary! {"Type"=>"XObject","Subtype"=>"Image","Width"=>image.width as i64,"Height"=>image.height as i64,"ColorSpace"=>"DeviceRGB","BitsPerComponent"=>8,"SMask"=>mask},
        rgb,
    );
    stream.compress().map_err(|e| e.to_string())?;
    Ok(doc.add_object(stream))
}
fn drawing(
    doc: &mut Document,
    content: &Content,
    w: f64,
    h: f64,
    fonts: &mut HashMap<String, suite42_pdf_text::PdfFont>,
    images: &mut HashMap<(u32, u32, usize), ObjectId>,
) -> Result<(String, Dictionary), String> {
    match content {
        Content::Watermark(spec) => {
            let radians = spec.degrees.rem_euclid(360.0).to_radians();
            let (sin, cos) = radians.sin_cos();
            let mut resources = dictionary! {"ExtGState"=>dictionary!{"Opacity"=>dictionary!{"Type"=>"ExtGState","ca"=>Object::Real(spec.opacity as f32),"CA"=>Object::Real(spec.opacity as f32)}}};
            let mut out = format!(
                "q /Opacity gs {cos} {sin} {} {cos} {} {} cm ",
                -sin,
                w / 2.0,
                h / 2.0
            );
            if let Some(image) = &spec.image {
                let key = (
                    image.width,
                    image.height,
                    std::sync::Arc::as_ptr(&image.rgba) as *const u8 as usize,
                );
                let id = if let Some(id) = images.get(&key) {
                    *id
                } else {
                    let id = image_resource(doc, image)?;
                    images.insert(key, id);
                    id
                };
                resources.set("XObject", dictionary! {"Image"=>id});
                let iw = w * 0.5;
                let ih = iw * f64::from(image.height) / f64::from(image.width);
                out.push_str(&format!(
                    "{iw} 0 0 {ih} {} {} cm /Image Do ",
                    -iw / 2.0,
                    -ih / 2.0
                ));
            } else {
                let (outline, width) = suite42_pdf_text::text_outline(&spec.text, spec.size)?;
                let color = spec.color.map(|v| f64::from(v) / 255.0);
                out.push_str(&format!(
                    "{} {} {} rg 1 0 0 1 {} {} cm {outline}",
                    color[0],
                    color[1],
                    color[2],
                    -width / 2.0,
                    -spec.size * 0.35
                ));
            }
            out.push('Q');
            Ok((out, resources))
        }
        Content::Text {
            lines,
            size,
            font:program,
            color,
            horizontal,
            vertical,
            ..
        } => {
            let all = lines.join("");
            let glyphs = all
                .chars()
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<String>();
            let key=format!("{}:{}",program.as_ref().map_or(0,|f|f.cache_id()),glyphs);
            if !fonts.contains_key(&key) {
                fonts.insert(
                    key.clone(),
                    if let Some(program)=program{program.embed_subset(doc,&all)?}else{suite42_pdf_text::PdfFont::embed_subset(doc, &all)?},
                );
            }
            let font = &fonts[&key];
            let resources = dictionary! {"Font"=>dictionary!{"Text"=>font.id}};
            let color = color.map(|v| f64::from(v) / 255.0);
            let mut out = format!("q {} {} {} rg ", color[0], color[1], color[2]);
            for (slot, line) in lines.iter().enumerate().filter(|(_, s)| !s.is_empty()) {
                let width = line
                    .chars()
                    .map(|c| u64::from(font.chars[&c].2))
                    .sum::<u64>() as f64
                    * size
                    / 1000.0;
                let x = match slot % 3 {
                    0 => *horizontal,
                    1 => (w - width) / 2.0,
                    _ => w - horizontal - width,
                };
                let y = if slot < 3 {
                    h - vertical - size * 0.8
                } else {
                    *vertical
                };
                let encoded = font.encode(line)?;
                out.push_str(&format!(
                    "BT /Text {size} Tf 1 0 0 1 {x} {y} Tm <{encoded}> Tj ET "
                ));
            }
            out.push('Q');
            Ok((out, resources))
        }
    }
}

pub(super) fn inherited_resources(doc: &Document, page: ObjectId) -> Result<Dictionary, String> {
    let mut id = page;
    let mut seen = HashSet::new();
    for _ in 0..128 {
        if !seen.insert(id) {
            return Err("Cyclic page resource inheritance".into());
        }
        let dict = doc.get_dictionary(id).map_err(|e| e.to_string())?;
        if let Ok(value) = dict.get(b"Resources") {
            return doc
                .dereference(value)
                .map_err(|e| e.to_string())?
                .1
                .as_dict()
                .cloned()
                .map_err(|e| e.to_string());
        }
        match dict.get(b"Parent") {
            Ok(value) => id = value.as_reference().map_err(|e| e.to_string())?,
            Err(_) => return Ok(Dictionary::new()),
        }
    }
    Err("Page resource inheritance exceeds limits".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> PdfDocument {
        let mut doc = Document::with_version("1.7");
        let root = doc.new_object_id();
        let mut kids = Vec::new();
        let font =
            doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
        let shared = doc
            .add_object(dictionary! {"Font"=>dictionary!{"F1"=>font},"XObject"=>Dictionary::new()});
        for rotation in [0, 90, 180, 270] {
            let content = doc.add_object(Stream::new(
                dictionary! {},
                b"BT /F1 12 Tf 30 140 Td (ORIGINAL) Tj ET\n2 0 0 2 0 0 cm".to_vec(),
            ));
            let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Rotate"=>rotation,"Contents"=>content});
            kids.push(Object::Reference(page));
        }
        doc.objects.insert(
            root,
            Object::Dictionary(
                dictionary! {"Type"=>"Pages","Count"=>4,"Kids"=>kids,"Resources"=>shared},
            ),
        );
        let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
        doc.trailer.set("Root", catalog);
        PdfDocument::from_loaded_document(doc, false).unwrap()
    }
    fn extracted_text(doc: &Document, number: u32) -> String {
        let page = doc.get_pages()[&number];
        let mut text = doc.extract_text(&[number]).unwrap();
        if let Ok(resources) = doc.get_dictionary(page).unwrap().get(b"Resources") {
            let resources = doc.dereference(resources).unwrap().1.as_dict().unwrap();
            if let Ok(xobjects) = resources.get(b"XObject") {
                let xobjects = doc.dereference(xobjects).unwrap().1.as_dict().unwrap();
                for (_, value) in xobjects.iter() {
                    let id = value.as_reference().unwrap();
                    let form = doc.get_object(id).unwrap().as_stream().unwrap();
                    if form.dict.get(b"Subtype").unwrap().as_name().unwrap() != b"Form" {
                        continue;
                    }
                    let mut projected = doc.clone();
                    let page = projected.get_dictionary_mut(page).unwrap();
                    page.set("Contents", id);
                    page.set("Resources", form.dict.get(b"Resources").unwrap().clone());
                    text.push_str(&projected.extract_text(&[number]).unwrap());
                }
            }
        }
        text
    }
    #[test]
    fn selected_font_program_survives_header_snapshot_history_and_reopen(){
        let program=suite42_pdf_text::FontProgram::new(include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf").to_vec()).unwrap();
        let mut doc=source();let baseline=doc.save_snapshot().unwrap().0;
        let mut spec=HeaderFooter::default();spec.texts=["FONT Привет 42".into(),String::new(),String::new(),String::new(),String::new(),String::new()];spec.font=Some(program.clone());
        doc.add_header_footer(&[0,2],&spec).unwrap();let changed=doc.save_snapshot().unwrap().0;
        let saved=Document::load_mem(&changed).unwrap();assert!(extracted_text(&saved,1).contains("FONT Привет 42"));assert!(!extracted_text(&saved,2).contains("FONT Привет 42"));
        assert!(saved.objects.values().any(|o|o.as_dict().is_ok_and(|d|d.get(b"BaseFont").is_ok_and(|v|v.as_name().is_ok_and(|n|n==program.name().as_bytes()))&&d.get(b"ToUnicode").is_ok())));
        assert!(saved.objects.values().any(|o|o.as_dict().is_ok_and(|d|d.get(b"FontFile2").is_ok())));
        doc.undo();assert_eq!(doc.save_snapshot().unwrap().0,baseline);doc.redo();assert_eq!(doc.save_snapshot().unwrap().0,changed);
        let reopened=PdfDocument::open_bytes(&changed).unwrap();assert_eq!(reopened.page_count(),4);
        let current=doc.save_snapshot().unwrap().0;let mut missing=spec.clone();missing.texts[0]="\u{10FFFF}".into();assert!(doc.add_header_footer(&[0],&missing).is_err());assert_eq!(doc.save_snapshot().unwrap().0,current);
    }
    fn headers() -> HeaderFooter {
        let mut spec = HeaderFooter::default();
        spec.texts = [
            "FILE <<file>>".into(),
            "<<label>> <<page>>/<<pages>>".into(),
            "<<date>>".into(),
            "<<bates>>".into(),
            "Привет".into(),
            "RIGHT".into(),
        ];
        spec.file_name = "input.pdf".into();
        spec.date = "2026-11-07".into();
        spec.bates_start = 41;
        spec.bates_prefix = "CASE-".into();
        spec.bates_suffix = "-A".into();
        spec
    }
    #[test]
    fn atomic_ordered_tokens_bounds_history_and_saved_text() {
        let mut source = source();
        source
            .set_page_labels(0, 3, crate::PageLabelStyle::UpperRoman, "P-", 1)
            .unwrap();
        let original = source.save_snapshot().unwrap().0;
        assert_eq!(source.add_header_footer(&[3, 1], &headers()).unwrap(), 43);
        let saved = source.save_snapshot().unwrap().0;
        let doc = Document::load_mem(&saved).unwrap();
        let pages = doc.get_pages();
        assert_eq!(doc.extract_text(&[1]).unwrap().trim(), "ORIGINAL");
        let second = extracted_text(&doc, 2);
        assert!(
            second.contains("P-II")
                && second.contains("CASE-000041-A")
                && second.contains("input.pdf")
                && second.contains("2/4")
                && second.contains("Привет"),
            "{second}"
        );
        let fourth = extracted_text(&doc, 4);
        assert!(
            fourth.contains("CASE-000042-A") && fourth.contains("4/4"),
            "{fourth}"
        );
        assert!(source.undo());
        assert_eq!(source.save_snapshot().unwrap().0, original);
        assert!(source.redo());
        assert_eq!(source.save_snapshot().unwrap().0, saved);
        let mut invalid = headers();
        invalid.texts[0] = "bad\ntext".into();
        assert!(source.add_header_footer(&[0, 1], &invalid).is_err());
        assert_eq!(source.save_snapshot().unwrap().0, saved);
        assert!(source.add_header_footer(&[1, 1], &headers()).is_err());
        assert!(source.add_header_footer(&[7], &headers()).is_err());
        let mut overflow = headers();
        overflow.bates_start = u64::MAX;
        assert!(source.add_header_footer(&[0], &overflow).is_err());
        source.remove_decorations(&[DecorationKind::HeaderFooter]);
        assert!(!source.has_decorations(DecorationKind::HeaderFooter));
        assert!(source.undo());
        assert_eq!(source.save_snapshot().unwrap().0, saved);
        assert!(
            doc.get_dictionary(pages[&1])
                .unwrap()
                .get(b"Resources")
                .is_err(),
            "unselected page must retain inherited resources"
        );
    }
    #[test]
    fn vector_watermarks_do_not_pollute_search_and_follow_crop_rotation() {
        let mut source = source();
        let original = source.doc.clone();
        source
            .add_watermark(&[0, 1, 2, 3], Watermark::default())
            .unwrap();
        let bytes = source.save_snapshot().unwrap().0;
        let doc = Document::load_mem(&bytes).unwrap();
        for (index, (_, id)) in doc.get_pages().iter().enumerate() {
            let text = doc.extract_text(&[(index + 1) as u32]).unwrap();
            assert!(text.contains("ORIGINAL") && !text.contains("CONFIDENTIAL"));
            let page = doc.get_dictionary(*id).unwrap();
            let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
            let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
            assert!(fonts.get(b"F1").is_ok());
            let xobjects = resources.get(b"XObject").unwrap().as_dict().unwrap();
            assert_eq!(xobjects.len(), 1);
            let form = doc
                .get_object(xobjects.iter().next().unwrap().1.as_reference().unwrap())
                .unwrap()
                .as_stream()
                .unwrap();
            let data = form
                .decompressed_content()
                .unwrap_or_else(|_| form.content.clone());
            assert!(lopdf::content::Content::decode(&data)
                .unwrap()
                .operations
                .iter()
                .all(|op| !matches!(op.operator.as_str(), "Tj" | "TJ" | "Tf")));
            let contents = page.get(b"Contents").unwrap().as_array().unwrap();
            let last = doc
                .get_object(contents.last().unwrap().as_reference().unwrap())
                .unwrap()
                .as_stream()
                .unwrap();
            let commands = String::from_utf8(last.content.clone()).unwrap();
            let expected = [
                "1 0 0 1 20 30 cm",
                "0 1 -1 0 200 30 cm",
                "-1 0 0 -1 200 310 cm",
                "0 -1 1 0 20 310 cm",
            ][index];
            assert!(commands.contains(expected), "{commands}");
        }
        assert_eq!(
            source.doc.objects, original.objects,
            "snapshot must not mutate source resources"
        );
        assert!(source.undo());
        assert!(!source.has_decorations(DecorationKind::Watermark));
    }
    #[test]
    fn shared_watermark_forms_reuse_vectors_and_image_alpha() {
        let mut source = source();
        let image = ImageStamp {
            width: 2,
            height: 1,
            rgba: vec![255, 0, 0, 128, 0, 0, 255, 0].into(),
        };
        source
            .add_watermark(
                &[0, 1, 2, 3],
                Watermark {
                    image: Some(image),
                    ..Default::default()
                },
            )
            .unwrap();
        let bytes = source.save_snapshot().unwrap().0;
        let doc = Document::load_mem(&bytes).unwrap();
        let masks = doc
            .objects
            .values()
            .filter_map(|o| o.as_stream().ok())
            .filter(|s| {
                s.dict
                    .get(b"ColorSpace")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    == Some(b"DeviceGray")
            })
            .collect::<Vec<_>>();
        assert_eq!(masks.len(), 1);
        assert_eq!(masks[0].decompressed_content().unwrap(), vec![128, 0]);
        let mut forms = HashSet::new();
        for id in doc.get_pages().values() {
            let res = doc
                .get_dictionary(*id)
                .unwrap()
                .get(b"Resources")
                .unwrap()
                .as_dict()
                .unwrap();
            let dict = res.get(b"XObject").unwrap().as_dict().unwrap();
            forms.insert(dict.iter().next().unwrap().1.as_reference().unwrap());
        }
        assert_eq!(forms.len(), 2);
    }
    #[test]
    fn number_formats_skip_first_selected_and_noop_are_safe() {
        let mut source = source();
        let spec = PageNumbers {
            slot: 4,
            start: 5,
            skip_first: true,
            format: NumberFormat::PageOfTotal,
            size: 10.0,
            margin: 28.0,
        };
        source.add_page_numbers(&[3, 0, 2], &spec).unwrap();
        let bytes = source.save_snapshot().unwrap().0;
        let doc = Document::load_mem(&bytes).unwrap();
        assert!(!doc.extract_text(&[1]).unwrap().contains("Page"));
        assert!(extracted_text(&doc, 3).contains("Page 5 of 6"));
        assert!(extracted_text(&doc, 4).contains("Page 6 of 6"));
        let before = source.save_snapshot().unwrap().0;
        source.add_page_numbers(&[0], &spec).unwrap();
        assert_eq!(source.save_snapshot().unwrap().0, before);
        source.remove_decorations(&[DecorationKind::Watermark]);
        assert_eq!(source.save_snapshot().unwrap().0, before);
    }
}
