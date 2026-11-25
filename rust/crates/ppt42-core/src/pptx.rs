//! PPTX package overlay. Unowned package parts and XML elements remain intact.
use super::*;
use std::{
    collections::{HashMap, HashSet},
    io::{Cursor, Read, Write},
    ops::Range,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

#[derive(Clone)]
struct ObjectSource {
    object_id: u64,
    xml_id: String,
    xml_type: String,
    original: SlideObject,
    text_editable: bool,
    writable_cells: Vec<Vec<bool>>,
    transform: [f64; 4],
    inherited_geometry: bool,
    rich_safe: bool,
}
#[derive(Clone)]
struct SlideSource {
    slide_id: u64,
    path: String,
    presentation_entry: String,
    rel_id: String,
    layout_target: Option<String>,
    has_notes: bool,
    notes_path: Option<String>,
    notes_cloneable: bool,
    background: [u8; 3],
    decorations: Vec<SlideObject>,
    objects: Vec<ObjectSource>,
    timing: SlideTiming,
    timing_editable: bool,
}
pub struct Source {
    bytes: Vec<u8>,
    slides: Vec<SlideSource>,
    width: f64,
    height: f64,
    can_delete: bool,
}
impl Source {
    pub fn can_edit_timing(&self, slide: u64) -> bool {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .is_none_or(|s| s.timing_editable)
    }
    pub fn can_rotate(&self, slide: u64, object: u64) -> bool {
        let transform = self
            .slides
            .iter()
            .find(|s| s.slide_id == slide)
            .and_then(|s| s.objects.iter().find(|o| o.object_id == object))
            .map(|o| o.transform)
            .unwrap_or([1.0, 1.0, 0.0, 0.0]);
        ((transform[0] * 720.0 / self.width) - (transform[1] * 405.0 / self.height)).abs() <= 1e-9
    }
    pub fn can_edit_rich(&self, slide: u64, object: u64) -> bool {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .and_then(|s| s.objects.iter().find(|o| o.object_id == object))
            .map(|o| o.rich_safe)
            .unwrap_or(true)
    }
    pub fn background(&self, slide: u64) -> Option<[u8; 3]> {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .map(|s| s.background)
    }
    pub fn decorations(&self, slide: u64) -> &[SlideObject] {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .map(|s| s.decorations.as_slice())
            .unwrap_or(&[])
    }
    pub fn size_points(&self) -> (f64, f64) {
        (self.width / 12700.0, self.height / 12700.0)
    }
    pub fn can_delete(&self) -> bool {
        self.can_delete
    }
    pub fn can_add(&self) -> bool {
        self.slides
            .iter()
            .any(|s| s.layout_target.is_some() && s.path.starts_with("ppt/slides/"))
    }
    pub fn can_duplicate(&self, slide: u64) -> bool {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .map(|s| !s.has_notes || s.notes_cloneable)
            .unwrap_or(true)
    }
    pub fn can_edit_text(&self, slide: u64, object: u64) -> bool {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .and_then(|s| s.objects.iter().find(|o| o.object_id == object))
            .map(|o| o.text_editable)
            .unwrap_or(true)
    }
    pub fn can_edit_cell(&self, slide: u64, object: u64, row: usize, column: usize) -> bool {
        let entry = self
            .slides
            .iter()
            .find(|s| s.slide_id == slide)
            .and_then(|s| s.objects.iter().find(|o| o.object_id == object));
        let Some(entry) = entry else { return true };
        entry
            .writable_cells
            .get(row)
            .and_then(|r| r.get(column))
            .copied()
            .unwrap_or(false)
    }
    pub fn is_original_object(&self, slide: u64, object: u64) -> bool {
        self.slides
            .iter()
            .find(|s| s.slide_id == slide)
            .is_some_and(|s| s.objects.iter().any(|o| o.object_id == object))
    }
}
fn read(z: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = z.by_name(name).map_err(|e| e.to_string())?;
    if f.size() > 32 * 1024 * 1024 {
        return Err("PPTX part too large".into());
    }
    let mut v = Vec::new();
    f.read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(v)
}
fn xml<'a>(bytes: &'a [u8]) -> Result<roxmltree::Document<'a>, String> {
    roxmltree::Document::parse(std::str::from_utf8(bytes).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn local<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
    node.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == name)
}
fn attr<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}
fn num(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    attr(node, name)?.parse().ok()
}
fn parse_timing(root: roxmltree::Node<'_, '_>) -> Option<SlideTiming> {
    let Some(node) = root
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "transition")
    else {
        return Some(SlideTiming::default());
    };
    if node
        .attributes()
        .any(|a| !matches!(a.name(), "advTm" | "spd" | "dur" | "advClick"))
        || matches!(attr(node, "advClick"), Some("0" | "false"))
    {
        return None;
    }
    let children = node
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    let transition = match children.as_slice() {
        [] => Transition::Cut,
        [n] if n.tag_name().name() == "cut" && n.attributes().len() == 0 => Transition::Cut,
        [n] if n.tag_name().name() == "fade" && n.attributes().len() == 0 => Transition::Fade,
        [n] if n.tag_name().name() == "push"
            && matches!(attr(*n, "dir"), Some("l"))
            && n.attributes().len() == 1 =>
        {
            Transition::PushLeft
        }
        _ => return None,
    };
    let advance_ms = attr(node, "advTm")
        .map(str::parse::<u32>)
        .transpose()
        .ok()?;
    let duration_ms = if let Some(d) = attr(node, "dur") {
        d.parse().ok()?
    } else {
        match attr(node, "spd").unwrap_or("med") {
            "slow" => 1000,
            "med" => 500,
            "fast" => 250,
            _ => return None,
        }
    };
    if advance_ms.is_some_and(|v| !(100..=86_400_000).contains(&v))
        || !(100..=10_000).contains(&duration_ms)
    {
        return None;
    }
    Some(SlideTiming {
        advance_ms,
        transition,
        duration_ms,
    })
}
fn timing_xml(t: SlideTiming) -> String {
    let advance = t
        .advance_ms
        .map(|v| format!(" advTm=\"{v}\""))
        .unwrap_or_default();
    let effect = match t.transition {
        Transition::Cut => "<p:cut/>",
        Transition::PushLeft => "<p:push dir=\"l\"/>",
        Transition::Fade => "<p:fade/>",
    };
    format!("<p:transition xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:p14=\"http://schemas.microsoft.com/office/powerpoint/2010/main\" p14:dur=\"{}\"{advance}>{effect}</p:transition>", t.duration_ms)
}
fn group_transform(node: roxmltree::Node<'_, '_>) -> Option<[f64; 4]> {
    let (mut sx, mut sy, mut tx, mut ty) = (1.0, 1.0, 0.0, 0.0);
    for group in node
        .ancestors()
        .filter(|n| n.is_element() && n.tag_name().name() == "grpSp")
    {
        let props = group
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "grpSpPr")?;
        let transform = props
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "xfrm")?;
        if attr(transform, "rot").is_some_and(|v| v != "0")
            || matches!(attr(transform, "flipH"), Some("1" | "true"))
            || matches!(attr(transform, "flipV"), Some("1" | "true"))
        {
            return None;
        }
        let (off, ext, child_off, child_ext) = (
            local(transform, "off")?,
            local(transform, "ext")?,
            local(transform, "chOff")?,
            local(transform, "chExt")?,
        );
        let (ox, oy, w, h, cx, cy, cw, ch) = (
            num(off, "x")?,
            num(off, "y")?,
            num(ext, "cx")?,
            num(ext, "cy")?,
            num(child_off, "x")?,
            num(child_off, "y")?,
            num(child_ext, "cx")?,
            num(child_ext, "cy")?,
        );
        if w <= 0.0 || h <= 0.0 || cw <= 0.0 || ch <= 0.0 {
            return None;
        }
        let gx = w / cw;
        let gy = h / ch;
        tx = (tx - cx) * gx + ox;
        ty = (ty - cy) * gy + oy;
        sx *= gx;
        sy *= gy;
    }
    Some([sx, sy, tx, ty])
}
fn placeholder<'a>(
    doc: &'a roxmltree::Document<'a>,
    idx: Option<&str>,
    kind: Option<&str>,
) -> Option<roxmltree::Node<'a, 'a>> {
    doc.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "sp")
        .find(|n| {
            local(*n, "ph").is_some_and(|ph| {
                if let Some(index) = idx {
                    attr(ph, "idx") == Some(index)
                } else {
                    attr(ph, "type").unwrap_or("body") == kind.unwrap_or("body")
                }
            })
        })
}
fn content(node: roxmltree::Node<'_, '_>) -> String {
    node.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| {
            p.descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "t")
                .filter_map(|n| n.text())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn rich_paragraphs(node: roxmltree::Node<'_, '_>, base: &TextStyle) -> Vec<RichParagraph> {
    let Some(body) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "txBody")
    else {
        return Vec::new();
    };
    let mut paragraphs = Vec::new();
    for p in body
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
    {
        let properties = p
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == "pPr");
        let align = match properties.and_then(|n| attr(n, "algn")) {
            Some("ctr") => TextAlign::Center,
            Some("r") => TextAlign::Right,
            _ => TextAlign::Left,
        };
        let bullet = properties.is_some_and(|n| {
            n.children()
                .any(|v| v.is_element() && matches!(v.tag_name().name(), "buChar" | "buAutoNum"))
        });
        let mut runs = Vec::new();
        for run in p
            .children()
            .filter(|n| n.is_element() && n.tag_name().name() == "r")
        {
            let Some(text) = run
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "t")
            else {
                continue;
            };
            let mut style = base.clone();
            style.align = align;
            if let Some(pr) = run
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "rPr")
            {
                if let Some(v) = attr(pr, "sz").and_then(|v| v.parse::<f64>().ok()) {
                    let v = v / 100.0;
                    if (1.0..=512.0).contains(&v) {
                        style.points = v;
                    }
                }
                if let Some(v) = attr(pr, "b") {
                    style.bold = matches!(v, "1" | "true");
                }
                if let Some(v) = attr(pr, "i") {
                    style.italic = matches!(v, "1" | "true");
                }
                if let Some(v) = attr(pr, "u") {
                    style.underline = v != "none";
                }
                if let Some(f) = local(pr, "latin").and_then(|n| attr(n, "typeface")) {
                    if !f.is_empty() && f.len() <= 256 {
                        style.family = f.into();
                    }
                }
                if let Some(c) = color(pr, "solidFill") {
                    style.color = c;
                }
            }
            runs.push(RichRun {
                text: text.text().unwrap_or("").into(),
                style,
            });
        }
        if runs.is_empty() {
            runs.push(RichRun {
                text: String::new(),
                style: base.clone(),
            });
        }
        paragraphs.push(RichParagraph {
            runs,
            align,
            bullet,
        });
    }
    if paragraphs.len() == 1 && paragraphs[0].runs.len() == 1 && !paragraphs[0].bullet {
        Vec::new()
    } else {
        paragraphs
    }
}
fn rich_edit_safe(node: roxmltree::Node<'_, '_>) -> bool {
    let Some(body) = node
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "txBody")
    else {
        return false;
    };
    let paragraphs = body
        .children()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .collect::<Vec<_>>();
    if paragraphs.is_empty() {
        return false;
    }
    paragraphs.iter().all(|p| {
        p.children()
            .filter(|n| n.is_element())
            .all(|child| match child.tag_name().name() {
                "pPr" => {
                    child.attributes().all(|a| a.name() == "algn")
                        && child
                            .children()
                            .filter(|n| n.is_element())
                            .all(|n| matches!(n.tag_name().name(), "buChar" | "buNone"))
                }
                "r" => {
                    child
                        .children()
                        .filter(|n| n.is_element())
                        .all(|n| match n.tag_name().name() {
                            "t" => true,
                            "rPr" => {
                                n.attributes()
                                    .all(|a| matches!(a.name(), "sz" | "b" | "i" | "u"))
                                    && n.children().filter(|c| c.is_element()).all(|c| {
                                        match c.tag_name().name() {
                                            "latin" => {
                                                c.attributes().all(|a| a.name() == "typeface")
                                            }
                                            "solidFill" => c
                                                .children()
                                                .filter(|v| v.is_element())
                                                .all(|v| v.tag_name().name() == "srgbClr"),
                                            _ => false,
                                        }
                                    })
                            }
                            _ => false,
                        })
                }
                _ => false,
            })
    })
}
fn target(base: &str, value: &str) -> Option<String> {
    if value.contains(':') || value.starts_with('/') {
        return None;
    }
    let mut parts = base.split('/').collect::<Vec<_>>();
    parts.pop();
    for segment in value.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            x => parts.push(x),
        }
    }
    Some(parts.join("/"))
}
fn color(node: roxmltree::Node<'_, '_>, name: &str) -> Option<[u8; 3]> {
    let shape = local(node, name)?;
    let val = attr(local(shape, "srgbClr")?, "val")?;
    if val.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&val[0..2], 16).ok()?,
        u8::from_str_radix(&val[2..4], 16).ok()?,
        u8::from_str_radix(&val[4..6], 16).ok()?,
    ])
}
fn resolved_color(
    node: roxmltree::Node<'_, '_>,
    palette: &HashMap<String, [u8; 3]>,
    aliases: &HashMap<String, String>,
) -> Option<[u8; 3]> {
    if let Some(c) = local(node, "srgbClr").and_then(|n| attr(n, "val")) {
        if c.len() == 6 {
            return Some([
                u8::from_str_radix(&c[0..2], 16).ok()?,
                u8::from_str_radix(&c[2..4], 16).ok()?,
                u8::from_str_radix(&c[4..6], 16).ok()?,
            ]);
        }
    }
    if let Some(value) = local(node, "schemeClr").and_then(|n| attr(n, "val")) {
        return palette
            .get(aliases.get(value).map(String::as_str).unwrap_or(value))
            .copied();
    }
    None
}
fn decoration(
    node: roxmltree::Node<'_, '_>,
    width: f64,
    height: f64,
    palette: &HashMap<String, [u8; 3]>,
    aliases: &HashMap<String, String>,
) -> Option<SlideObject> {
    if local(node, "ph").is_some() {
        return None;
    }
    let xfrm = local(node, "xfrm")?;
    if attr(xfrm, "rot").is_some_and(|v| v != "0") {
        return None;
    }
    let (off, ext) = (local(xfrm, "off")?, local(xfrm, "ext")?);
    let t = group_transform(node)?;
    let (x, y, w, h) = (
        num(off, "x")?,
        num(off, "y")?,
        num(ext, "cx")?,
        num(ext, "cy")?,
    );
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let preset = local(node, "prstGeom")
        .and_then(|n| attr(n, "prst"))
        .unwrap_or("rect");
    let kind = match preset {
        "rect" => ObjectKind::Rectangle,
        "ellipse" => ObjectKind::Ellipse,
        "line" => ObjectKind::Line,
        _ => return None,
    };
    let fill = local(node, "solidFill").and_then(|n| resolved_color(n, palette, aliases))?;
    let stroke = local(node, "ln")
        .and_then(|n| resolved_color(n, palette, aliases))
        .unwrap_or(fill);
    Some(SlideObject {
        id: 0,
        group_id: None,
        x: (x * t[0] + t[2]) / width * 720.0,
        y: (y * t[1] + t[3]) / height * 405.0,
        width: w * t[0] / width * 720.0,
        height: h * t[1] / height * 405.0,
        rotation_degrees: 0.0,
        kind,
        fill,
        stroke,
        fill_visible: true,
        stroke_visible: true,
        text: content(node),
        text_style: TextStyle::default(),
        paragraphs: Vec::new(),
        media: Vec::new(),
        cells: Vec::new(),
    })
}
/// Opens ordinary PPTX packages, retaining all original parts for lossless overlay.
/// Objects outside the editable subset remain in the package, but may not appear on the canvas.
pub fn import(bytes: &[u8]) -> Result<(Presentation, Source), String> {
    if bytes.len() as u64 > MAX_MEDIA_FILE {
        return Err("PPTX exceeds limit".into());
    }
    let mut z = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let presentation_bytes = read(&mut z, "ppt/presentation.xml")?;
    let rel = read(&mut z, "ppt/_rels/presentation.xml.rels")?;
    let p = xml(&presentation_bytes)?;
    let rel = xml(&rel)?;
    let mut targets = HashMap::new();
    for r in rel
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
    {
        if let (Some(id), Some(t)) = (attr(r, "Id"), attr(r, "Target")) {
            if let Some(path) = target("ppt/presentation.xml", t) {
                targets.insert(id.to_owned(), path);
            }
        }
    }
    let size = local(p.root_element(), "sldSz");
    let width = size.and_then(|n| num(n, "cx")).unwrap_or(9144000.0);
    let height = size.and_then(|n| num(n, "cy")).unwrap_or(5143500.0);
    if width <= 0.0 || height <= 0.0 {
        return Err("Invalid slide size".into());
    }
    let presentation_text = std::str::from_utf8(&presentation_bytes).map_err(|e| e.to_string())?;
    let paths=p.descendants().filter(|n|n.is_element()&&n.tag_name().name()=="sldId").filter_map(|n|n.attributes().find(|a|a.name()=="id"&&a.namespace()==Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships")).and_then(|a|targets.get(a.value()).map(|path|(path.clone(),a.value().to_owned()))).map(|(path,id)|(path,id,presentation_text[n.range()].to_owned()))).collect::<Vec<_>>();
    if paths.is_empty() || paths.len() > MAX_SLIDES {
        return Err("Missing or invalid PPTX slide list".into());
    }
    let mut deck = Presentation::new();
    let mut slides = Vec::new();
    for (index, (path, rel_id, presentation_entry)) in paths.into_iter().enumerate() {
        if index > 0 {
            deck.add()?;
        }
        let slide_bytes = read(&mut z, &path)?;
        let tree = xml(&slide_bytes)?;
        let parsed_timing = parse_timing(tree.root_element());
        let timing = parsed_timing.unwrap_or_default();
        deck.set_timing(timing)?;
        let mut objects = Vec::new();
        let mut group_ids: HashMap<usize, u64> = HashMap::new();
        let (directory, filename) = path.rsplit_once('/').ok_or("Invalid slide path")?;
        let relpath = format!("{directory}/_rels/{filename}.rels");
        let relbytes = read(&mut z, &relpath).ok();
        let mut slide_targets = HashMap::new();
        let mut layout_target = None;
        let mut has_notes = false;
        let mut notes_path = None;
        if let Some(ref relbytes) = relbytes {
            let relationships = xml(relbytes)?;
            for r in relationships
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
            {
                let kind = attr(r, "Type").unwrap_or("");
                if kind.ends_with("/slideLayout") {
                    layout_target = attr(r, "Target").map(str::to_owned);
                }
                if kind.ends_with("/notesSlide") {
                    has_notes = true;
                    notes_path = attr(r, "Target").and_then(|v| target(&path, v));
                }
                if let (Some(id), Some(value)) = (attr(r, "Id"), attr(r, "Target")) {
                    if let Some(target) = target(&path, value) {
                        slide_targets.insert(id.to_owned(), target);
                    }
                }
            }
        }
        let layout_path = layout_target.as_deref().and_then(|v| target(&path, v));
        let layout_bytes = layout_path.as_deref().and_then(|p| read(&mut z, p).ok());
        let layout_doc = layout_bytes.as_ref().and_then(|b| xml(b).ok());
        let master_path = layout_path.as_ref().and_then(|p| {
            let (dir, file) = p.rsplit_once('/')?;
            let bytes = read(&mut z, &format!("{dir}/_rels/{file}.rels")).ok()?;
            let doc = xml(&bytes).ok()?;
            let v = doc
                .descendants()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "Relationship"
                        && attr(*n, "Type").is_some_and(|v| v.ends_with("/slideMaster"))
                })
                .and_then(|n| attr(n, "Target"))?;
            target(p, v)
        });
        let master_bytes = master_path.as_deref().and_then(|p| read(&mut z, p).ok());
        let master_doc = master_bytes.as_ref().and_then(|b| xml(b).ok());
        let theme_path = master_path.as_ref().and_then(|p| {
            let (dir, file) = p.rsplit_once('/')?;
            let bytes = read(&mut z, &format!("{dir}/_rels/{file}.rels")).ok()?;
            let doc = xml(&bytes).ok()?;
            let value = doc
                .descendants()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "Relationship"
                        && attr(*n, "Type").is_some_and(|v| v.ends_with("/theme"))
                })
                .and_then(|n| attr(n, "Target"))?;
            target(p, value)
        });
        let theme_bytes = theme_path.as_deref().and_then(|p| read(&mut z, p).ok());
        let theme_doc = theme_bytes.as_ref().and_then(|b| xml(b).ok());
        let mut palette = HashMap::new();
        if let Some(theme) = theme_doc.as_ref() {
            if let Some(scheme) = local(theme.root_element(), "clrScheme") {
                for entry in scheme.children().filter(|n| n.is_element()) {
                    let value = local(entry, "srgbClr")
                        .and_then(|n| attr(n, "val"))
                        .or_else(|| local(entry, "sysClr").and_then(|n| attr(n, "lastClr")));
                    if let Some(v) = value {
                        if v.len() == 6 {
                            if let (Ok(r), Ok(g), Ok(b)) = (
                                u8::from_str_radix(&v[0..2], 16),
                                u8::from_str_radix(&v[2..4], 16),
                                u8::from_str_radix(&v[4..6], 16),
                            ) {
                                palette.insert(entry.tag_name().name().to_owned(), [r, g, b]);
                            }
                        }
                    }
                }
            }
        }
        let mut aliases = HashMap::new();
        if let Some(master) = master_doc.as_ref() {
            if let Some(map) = local(master.root_element(), "clrMap") {
                for a in map.attributes() {
                    aliases.insert(a.name().to_owned(), a.value().to_owned());
                }
            }
        }
        let background = [Some(&tree), layout_doc.as_ref(), master_doc.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|doc| {
                local(doc.root_element(), "bg")
                    .and_then(|bg| resolved_color(bg, &palette, &aliases))
            })
            .unwrap_or([255, 255, 255]);
        let mut decorations = Vec::new();
        for doc in [master_doc.as_ref(), layout_doc.as_ref()]
            .into_iter()
            .flatten()
        {
            for shape in doc.descendants().filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "sp"
                    && n.parent()
                        .is_some_and(|p| matches!(p.tag_name().name(), "spTree" | "grpSp"))
            }) {
                if let Some(o) = decoration(shape, width, height, &palette, &aliases) {
                    decorations.push(o);
                }
            }
        }
        for node in tree.descendants().filter(|n| {
            n.is_element()
                && matches!(n.tag_name().name(), "sp" | "pic" | "graphicFrame")
                && n.parent()
                    .is_some_and(|p| matches!(p.tag_name().name(), "spTree" | "grpSp"))
        }) {
            let Some(id) = local(node, "cNvPr").and_then(|n| attr(n, "id")) else {
                continue;
            };
            let ph = local(node, "ph");
            let idx = ph.and_then(|n| attr(n, "idx"));
            let ph_type = ph.and_then(|n| attr(n, "type"));
            let inherited = if ph.is_some() {
                layout_doc
                    .as_ref()
                    .and_then(|doc| placeholder(doc, idx, ph_type))
                    .or_else(|| {
                        master_doc
                            .as_ref()
                            .and_then(|doc| placeholder(doc, idx, ph_type))
                    })
            } else {
                None
            };
            let explicit = local(node, "xfrm");
            let Some(xfrm) = explicit
                .or_else(|| inherited.and_then(|n| local(n, "xfrm")))
                .or_else(|| {
                    if ph.is_some() {
                        master_doc
                            .as_ref()
                            .and_then(|doc| placeholder(doc, idx, ph_type))
                            .and_then(|n| local(n, "xfrm"))
                    } else {
                        None
                    }
                })
            else {
                continue;
            };
            if matches!(attr(xfrm, "flipH"), Some("1" | "true"))
                || matches!(attr(xfrm, "flipV"), Some("1" | "true"))
            {
                continue;
            }
            let Some(transform) = group_transform(node) else {
                continue;
            };
            let rotation = attr(xfrm, "rot")
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or(0.0)
                / 60000.0;
            if rotation.rem_euclid(360.0) != 0.0
                && ((transform[0] * 720.0 / width) - (transform[1] * 405.0 / height)).abs() > 1e-9
            {
                continue;
            }
            let (Some(off), Some(ext)) = (local(xfrm, "off"), local(xfrm, "ext")) else {
                continue;
            };
            let (Some(x), Some(y), Some(w), Some(h)) =
                (num(off, "x"), num(off, "y"), num(ext, "cx"), num(ext, "cy"))
            else {
                continue;
            };
            if w <= 0.0 || h <= 0.0 {
                continue;
            }
            let preset = local(node, "prstGeom")
                .and_then(|n| attr(n, "prst"))
                .unwrap_or("rect");
            let kind = if node.tag_name().name() == "pic" {
                ObjectKind::Picture
            } else if node.tag_name().name() == "graphicFrame" && local(node, "tbl").is_some() {
                ObjectKind::Table
            } else if node.tag_name().name() == "graphicFrame" {
                continue;
            } else {
                match preset {
                    "rect" => {
                        if local(node, "txBody").is_some() {
                            ObjectKind::Text
                        } else {
                            ObjectKind::Rectangle
                        }
                    }
                    "ellipse" => ObjectKind::Ellipse,
                    "line" => ObjectKind::Line,
                    _ => continue,
                }
            };
            let media = if matches!(kind, ObjectKind::Picture) {
                let Some(embed) = local(node, "blip").and_then(|n| attr(n, "embed")) else {
                    continue;
                };
                let Some(target) = slide_targets.get(embed) else {
                    continue;
                };
                read(&mut z, target)?
            } else {
                Vec::new()
            };
            let cells = if matches!(kind, ObjectKind::Table) {
                let table = local(node, "tbl").unwrap();
                let rows = table
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "tr")
                    .map(|r| {
                        r.children()
                            .filter(|n| n.is_element() && n.tag_name().name() == "tc")
                            .map(content)
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                if rows.is_empty()
                    || rows[0].is_empty()
                    || rows.iter().any(|r| r.len() != rows[0].len())
                {
                    continue;
                }
                rows
            } else {
                Vec::new()
            };
            let text = if matches!(kind, ObjectKind::Picture | ObjectKind::Table) {
                String::new()
            } else {
                content(node)
            };
            let object_id = deck.insert_content(kind, text, media, cells)?;
            let mut object = deck.slides()[index]
                .objects
                .iter()
                .find(|o| o.id == object_id)
                .unwrap()
                .clone();
            if let Some(group) = node
                .ancestors()
                .filter(|n| n.is_element() && n.tag_name().name() == "grpSp")
                .last()
            {
                let key = group.range().start;
                let id = *group_ids.entry(key).or_insert(object.id);
                object.group_id = Some(id);
            }
            object.x = (x * transform[0] + transform[2]) / width * 720.0;
            object.y = (y * transform[1] + transform[3]) / height * 405.0;
            object.width = w * transform[0] / width * 720.0;
            object.height = h * transform[1] / height * 405.0;
            object.rotation_degrees = rotation.rem_euclid(360.0);
            let props = node
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "spPr");
            let inherited_props = inherited.and_then(|n| {
                n.children()
                    .find(|c| c.is_element() && c.tag_name().name() == "spPr")
            });
            let fill_node = props
                .and_then(|p| {
                    p.children().find(|n| {
                        n.is_element() && matches!(n.tag_name().name(), "solidFill" | "noFill")
                    })
                })
                .or_else(|| {
                    inherited_props.and_then(|p| {
                        p.children().find(|n| {
                            n.is_element() && matches!(n.tag_name().name(), "solidFill" | "noFill")
                        })
                    })
                });
            object.fill_visible = fill_node.is_some_and(|n| n.tag_name().name() == "solidFill");
            if let Some(c) = fill_node.and_then(|n| resolved_color(n, &palette, &aliases)) {
                object.fill = c;
            }
            let line_node = props
                .and_then(|p| {
                    p.children()
                        .find(|n| n.is_element() && n.tag_name().name() == "ln")
                })
                .or_else(|| {
                    inherited_props.and_then(|p| {
                        p.children()
                            .find(|n| n.is_element() && n.tag_name().name() == "ln")
                    })
                });
            object.stroke_visible = line_node.is_some_and(|n| {
                !n.children()
                    .any(|c| c.is_element() && c.tag_name().name() == "noFill")
            });
            if let Some(c) = line_node.and_then(|n| resolved_color(n, &palette, &aliases)) {
                object.stroke = c;
            }
            if let Some(properties) = local(node, "rPr")
                .or_else(|| inherited.and_then(|n| local(n, "rPr").or_else(|| local(n, "defRPr"))))
            {
                let style = &mut object.text_style;
                if let Some(v) = attr(properties, "sz").and_then(|v| v.parse::<f64>().ok()) {
                    let points = v / 100.0;
                    if (1.0..=512.0).contains(&points) {
                        style.points = points;
                    }
                }
                style.bold = matches!(attr(properties, "b"), Some("1" | "true"));
                style.italic = matches!(attr(properties, "i"), Some("1" | "true"));
                style.underline = attr(properties, "u").is_some_and(|v| v != "none");
                if let Some(family) = local(properties, "latin").and_then(|n| attr(n, "typeface")) {
                    if !family.is_empty() && family.len() <= 256 {
                        style.family = family.to_owned();
                    }
                }
                if let Some(c) = color(properties, "solidFill") {
                    style.color = c;
                }
            }
            if let Some(properties) =
                local(node, "pPr").or_else(|| inherited.and_then(|n| local(n, "pPr")))
            {
                object.text_style.align = match attr(properties, "algn") {
                    Some("ctr") => TextAlign::Center,
                    Some("r") => TextAlign::Right,
                    _ => TextAlign::Left,
                };
            }
            object.paragraphs = rich_paragraphs(node, &object.text_style);
            if !object.paragraphs.is_empty()
                && object
                    .paragraphs
                    .iter()
                    .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                    .collect::<Vec<_>>()
                    .join("\n")
                    != object.text
            {
                object.paragraphs.clear();
            }
            deck.update_object(object.clone())?;
            objects.push(ObjectSource {
                object_id,
                xml_id: id.into(),
                xml_type: node.tag_name().name().into(),
                original: object,
                text_editable: node.tag_name().name() == "sp"
                    && node
                        .descendants()
                        .any(|n| n.is_element() && n.tag_name().name() == "t"),
                writable_cells: local(node, "tbl")
                    .map(|table| {
                        table
                            .children()
                            .filter(|n| n.is_element() && n.tag_name().name() == "tr")
                            .map(|r| {
                                r.children()
                                    .filter(|n| n.is_element() && n.tag_name().name() == "tc")
                                    .map(|c| {
                                        c.descendants()
                                            .any(|n| n.is_element() && n.tag_name().name() == "t")
                                    })
                                    .collect()
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                transform,
                inherited_geometry: explicit.is_none(),
                rich_safe: node.tag_name().name() == "sp" && rich_edit_safe(node),
            });
        }
        let notes_cloneable = if let Some(ref note) = notes_path {
            if note.starts_with("ppt/notesSlides/") {
                if let Some((dir, file)) = note.rsplit_once('/') {
                    let relpath = format!("{dir}/_rels/{file}.rels");
                    read(&mut z, note).is_ok()
                        && read(&mut z, &relpath)
                            .ok()
                            .and_then(|bytes| {
                                xml(&bytes).ok().map(|doc| {
                                    doc.descendants().any(|n| {
                                        n.is_element()
                                            && n.tag_name().name() == "Relationship"
                                            && attr(n, "Type")
                                                .is_some_and(|v| v.ends_with("/slide"))
                                    })
                                })
                            })
                            .unwrap_or(false)
                } else {
                    false
                }
            } else {
                false
            }
        } else {
            !has_notes
        };
        slides.push(SlideSource {
            slide_id: deck.slides()[index].id,
            path,
            presentation_entry,
            rel_id,
            layout_target,
            has_notes,
            notes_path,
            notes_cloneable,
            background,
            decorations,
            objects,
            timing,
            timing_editable: parsed_timing.is_some(),
        });
    }
    deck.select(0)?;
    let state = deck.history.snapshot();
    deck.history = SnapshotHistory::new(state, HISTORY_LIMIT);
    let can_delete = read(&mut z, "_rels/.rels")
        .ok()
        .and_then(|bytes| {
            xml(&bytes).ok().map(|doc| {
                doc.descendants().any(|n| {
                    n.is_element()
                        && n.tag_name().name() == "Relationship"
                        && attr(n, "Target")
                            .and_then(|s| target("root.xml", s))
                            .as_deref()
                            == Some("ppt/presentation.xml")
                })
            })
        })
        .unwrap_or(false);
    Ok((
        deck,
        Source {
            bytes: bytes.to_vec(),
            slides,
            width,
            height,
            can_delete,
        },
    ))
}
fn replace_attr(
    node: roxmltree::Node<'_, '_>,
    name: &str,
    value: String,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let a = node
        .attributes()
        .find(|a| a.name() == name)
        .ok_or("Missing PPTX geometry")?;
    let range = a.range();
    let original = &source[range.clone()];
    let q = original.find('=').ok_or("Invalid attribute")?;
    patches.push((range, format!("{}=\"{}\"", &original[..q], value)));
    Ok(())
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\r', "&#13;")
}
fn patch_text(
    node: roxmltree::Node<'_, '_>,
    value: &str,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let texts = node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "t")
        .collect::<Vec<_>>();
    if texts.is_empty() || value.contains('\n') {
        return Err("Saving this PPTX text edit is not supported yet".into());
    }
    for (i, n) in texts.iter().enumerate() {
        let range = n.range();
        let fragment = &source[range.clone()];
        let begin = fragment.find('>').ok_or("Invalid text element")? + 1;
        let end = fragment.rfind('<').ok_or("Invalid text element")?;
        patches.push((
            range.start + begin..range.start + end,
            if i == 0 { escape(value) } else { String::new() },
        ));
    }
    Ok(())
}
fn style_attrs(old: &TextStyle, new: &TextStyle) -> Vec<(&'static str, String)> {
    let mut attrs = Vec::new();
    if old.points != new.points {
        attrs.push(("sz", format!("{:.0}", new.points * 100.0)));
    }
    if old.bold != new.bold {
        attrs.push(("b", if new.bold { "1" } else { "0" }.into()));
    }
    if old.italic != new.italic {
        attrs.push(("i", if new.italic { "1" } else { "0" }.into()));
    }
    if old.underline != new.underline {
        attrs.push(("u", if new.underline { "sng" } else { "none" }.into()));
    }
    attrs
}
fn update_element(
    node: roxmltree::Node<'_, '_>,
    source: &str,
    attrs: &[(&str, String)],
    children: &[(&str, String)],
) -> Result<String, String> {
    let range = node.range();
    let mut value = source[range.clone()].to_owned();
    let mut edits: Vec<(Range<usize>, String)> = Vec::new();
    let mut missing = String::new();
    for (name, v) in attrs {
        if let Some(a) = node.attributes().find(|a| a.name() == *name) {
            let r = a.range();
            let original = &source[r.clone()];
            let eq = original.find('=').ok_or("Invalid style attribute")?;
            edits.push((
                r.start - range.start..r.end - range.start,
                format!("{}=\"{}\"", &original[..eq], escape(v)),
            ));
        } else {
            missing.push_str(&format!(" {name}=\"{}\"", escape(v)));
        }
    }
    let mut appended = String::new();
    for (name, xml) in children {
        if let Some(child) = node
            .children()
            .find(|n| n.is_element() && n.tag_name().name() == *name)
        {
            let r = child.range();
            edits.push((r.start - range.start..r.end - range.start, xml.clone()));
        } else {
            appended.push_str(xml);
        }
    }
    let end = value.find('>').ok_or("Invalid style element")?;
    let insert = if value.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
        end - 1
    } else {
        end
    };
    if !missing.is_empty() {
        edits.push((insert..insert, missing));
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    for (r, v) in edits {
        value.replace_range(r, &v)
    }
    if !appended.is_empty() {
        append_element_content(&mut value, &appended)?;
    }
    Ok(value)
}
fn patch_text_style(
    node: roxmltree::Node<'_, '_>,
    old: &TextStyle,
    new: &TextStyle,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let attrs = style_attrs(old, new);
    let mut children = Vec::new();
    if old.family != new.family {
        children.push(("latin",format!("<a:latin xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" typeface=\"{}\"/>",escape(&new.family))));
    }
    if old.color != new.color {
        children.push(("solidFill",format!("<a:solidFill xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:srgbClr val=\"{:02X}{:02X}{:02X}\"/></a:solidFill>",new.color[0],new.color[1],new.color[2])));
    }
    if !attrs.is_empty() || !children.is_empty() {
        let runs = node
            .descendants()
            .filter(|n| {
                n.is_element()
                    && matches!(n.tag_name().name(), "r" | "fld")
                    && n.children()
                        .any(|c| c.is_element() && c.tag_name().name() == "t")
            })
            .collect::<Vec<_>>();
        if runs.is_empty() {
            return Err("This PPTX text structure cannot be formatted".into());
        }
        for run in runs {
            if let Some(properties) = run
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "rPr")
            {
                patches.push((
                    properties.range(),
                    update_element(properties, source, &attrs, &children)?,
                ));
            } else {
                let start = run.range().start;
                let end = source[start..run.range().end]
                    .find('>')
                    .ok_or("Invalid PPTX text run")?
                    + start
                    + 1;
                let attributes = attrs
                    .iter()
                    .map(|(n, v)| format!(" {n}=\"{}\"", escape(v)))
                    .collect::<String>();
                let child = children.iter().map(|(_, v)| v.as_str()).collect::<String>();
                patches.push((end..end,format!("<a:rPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"{attributes}>{child}</a:rPr>")));
            }
        }
    }
    if old.align != new.align {
        let value = match new.align {
            TextAlign::Left => "l",
            TextAlign::Center => "ctr",
            TextAlign::Right => "r",
        };
        for paragraph in node
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "p")
        {
            if let Some(properties) = paragraph
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "pPr")
            {
                patches.push((
                    properties.range(),
                    update_element(properties, source, &[("algn", value.into())], &[])?,
                ));
            } else {
                let start = paragraph.range().start;
                let end = source[start..paragraph.range().end]
                    .find('>')
                    .ok_or("Invalid paragraph")?
                    + start
                    + 1;
                patches.push((end..end,format!("<a:pPr xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" algn=\"{value}\"/>")));
            }
        }
    }
    Ok(())
}
fn rich_xml(paragraphs: &[RichParagraph]) -> String {
    paragraphs.iter().map(|p|{let align=match p.align{TextAlign::Left=>"l",TextAlign::Center=>"ctr",TextAlign::Right=>"r"};let bullet=if p.bullet{"<a:buChar char=\"•\"/>"}else{"<a:buNone/>"};let runs=p.runs.iter().map(|r|{let s=&r.style;let underline=if s.underline{"sng"}else{"none"};format!("<a:r><a:rPr sz=\"{:.0}\" b=\"{}\" i=\"{}\" u=\"{underline}\"><a:solidFill><a:srgbClr val=\"{:02X}{:02X}{:02X}\"/></a:solidFill><a:latin typeface=\"{}\"/></a:rPr><a:t xml:space=\"preserve\">{}</a:t></a:r>",s.points*100.0,s.bold as u8,s.italic as u8,s.color[0],s.color[1],s.color[2],escape(&s.family),escape(&r.text))}).collect::<String>();format!("<a:p xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:pPr algn=\"{align}\">{bullet}</a:pPr>{runs}</a:p>")}).collect()
}
fn relation_owner(path: &str) -> Option<String> {
    if path == "_rels/.rels" {
        return Some(String::new());
    }
    let (dir, file) = path.rsplit_once("/_rels/")?;
    Some(format!("{dir}/{}", file.strip_suffix(".rels")?))
}
fn prune_deleted(
    input: &mut ZipArchive<Cursor<&[u8]>>,
    source: &Source,
    deck: &Presentation,
    replacements: &mut HashMap<String, Vec<u8>>,
) -> Result<HashSet<String>, String> {
    let deleted = source
        .slides
        .iter()
        .filter(|r| !deck.slides().iter().any(|s| s.id == r.slide_id))
        .collect::<Vec<_>>();
    if deleted.is_empty() {
        return Ok(HashSet::new());
    }
    let deleted_ids = deleted
        .iter()
        .map(|s| s.rel_id.as_str())
        .collect::<HashSet<_>>();
    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let mut rel_files = HashMap::new();
    let names = (0..input.len())
        .map(|i| {
            input
                .by_index(i)
                .map(|f| f.name().to_owned())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
    for name in names.iter().filter(|n| n.ends_with(".rels")) {
        let Some(owner) = relation_owner(name) else {
            continue;
        };
        let bytes = read(input, name)?;
        let document = xml(&bytes)?;
        let mut edges = Vec::new();
        let mut removed = Vec::new();
        for rel in document
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            if owner == "ppt/presentation.xml"
                && attr(rel, "Id").is_some_and(|id| deleted_ids.contains(id))
            {
                removed.push(rel.range());
                continue;
            }
            if attr(rel, "TargetMode") == Some("External") {
                continue;
            }
            if let Some(value) = attr(rel, "Target") {
                let base = if owner.is_empty() { "root.xml" } else { &owner };
                if let Some(path) = target(base, value) {
                    edges.push(path);
                }
            }
        }
        if !removed.is_empty() {
            let mut updated = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            for range in removed.into_iter().rev() {
                updated.replace_range(range, "");
            }
            replacements.insert(name.clone(), updated.into_bytes());
        }
        rel_files.insert(owner.clone(), name.clone());
        graph.insert(owner, edges);
    }
    if !graph
        .get("")
        .is_some_and(|v| v.iter().any(|p| p == "ppt/presentation.xml"))
    {
        return Err(
            "Cannot safely remove slides: package root relationships are unavailable".into(),
        );
    }
    fn closure(
        graph: &HashMap<String, Vec<String>>,
        starts: impl IntoIterator<Item = String>,
    ) -> HashSet<String> {
        let mut visited = HashSet::new();
        let mut pending = starts.into_iter().collect::<Vec<_>>();
        while let Some(node) = pending.pop() {
            if visited.insert(node.clone()) {
                if let Some(next) = graph.get(&node) {
                    pending.extend(next.iter().cloned());
                }
            }
        }
        visited
    }
    let reachable = closure(&graph, [String::new()]);
    let removed = closure(&graph, deleted.iter().map(|s| s.path.clone()));
    let mut prune = HashSet::new();
    for part in removed.difference(&reachable) {
        prune.insert(part.clone());
        if let Some(rels) = rel_files.get(part) {
            prune.insert(rels.clone());
        }
    }
    if let Ok(bytes) = read(input, "[Content_Types].xml") {
        let doc = xml(&bytes)?;
        let mut ranges = doc
            .descendants()
            .filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "Override"
                    && attr(*n, "PartName")
                        .is_some_and(|s| prune.contains(s.trim_start_matches('/')))
            })
            .map(|n| n.range())
            .collect::<Vec<_>>();
        if !ranges.is_empty() {
            ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
            let mut content = String::from_utf8(bytes).map_err(|e| e.to_string())?;
            for r in ranges {
                content.replace_range(r, "");
            }
            replacements.insert("[Content_Types].xml".into(), content.into_bytes());
        }
    }
    Ok(prune)
}
/// Patches only geometry and text of imported supported shapes; all other ZIP
/// entries and slide XML remain unchanged. Unsupported edits return an error.
pub fn export(deck: &Presentation, source: &Source) -> Result<Vec<u8>, String> {
    if source.slides.iter().any(|r| {
        !deck.slides().iter().any(|s| s.id == r.slide_id)
            && deck
                .slides()
                .iter()
                .any(|s| s.source_identity() == r.slide_id)
    }) {
        return Err("Remove duplicates before deleting their original PPTX slide".into());
    }
    let mut input =
        ZipArchive::new(Cursor::new(source.bytes.as_slice())).map_err(|e| e.to_string())?;
    let mut replacements = HashMap::new();
    let mut added_files: Vec<(String, Vec<u8>)> = Vec::new();
    let mut image_extensions = HashSet::new();
    let prune = prune_deleted(&mut input, source, deck, &mut replacements)?;
    let mut new_slide_entries = HashMap::new();
    let mut slide_overrides = String::new();
    let mut presentation_relationships = String::new();
    let new_slides = deck
        .slides()
        .iter()
        .filter(|s| !source.slides.iter().any(|r| r.slide_id == s.id))
        .collect::<Vec<_>>();
    if !new_slides.is_empty() {
        if !source.can_add() {
            return Err("Cannot add slides without a compatible layout".into());
        }
        let bytes = read(&mut input, "ppt/presentation.xml")?;
        let doc = xml(&bytes)?;
        let mut next = doc
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "sldId")
            .filter_map(|n| attr(n, "id").and_then(|v| v.parse::<u32>().ok()))
            .max()
            .unwrap_or(255);
        let relbytes = if let Some(v) = replacements.get("ppt/_rels/presentation.xml.rels") {
            v.clone()
        } else {
            read(&mut input, "ppt/_rels/presentation.xml.rels")?
        };
        let reltext = std::str::from_utf8(&relbytes).map_err(|e| e.to_string())?;
        for slide in &new_slides {
            if let Some(origin) = source
                .slides
                .iter()
                .find(|r| r.slide_id == slide.source_identity())
            {
                if origin.has_notes && !origin.notes_cloneable {
                    return Err("Duplicating PPTX slides with notes is not supported yet".into());
                }
            }
            next = next.checked_add(1).ok_or("PPTX slide ID exhausted")?;
            let id = format!("suite42Slide{}", slide.id);
            let path = format!("ppt/slides/suite42_{}.xml", slide.id);
            if reltext.contains(&format!("\"{id}\"")) || input.by_name(&path).is_ok() {
                return Err("PPTX slide identifier collision".into());
            }
            new_slide_entries.insert(slide.id,(path.clone(),format!("<p:sldId xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" id=\"{next}\" r:id=\"{id}\"/>")));
            presentation_relationships.push_str(&format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide\" Target=\"slides/suite42_{}.xml\"/>",slide.id));
            slide_overrides.push_str(&format!("<Override PartName=\"/{path}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"));
        }
        let mut rels = String::from_utf8(relbytes).map_err(|e| e.to_string())?;
        append_element_content(&mut rels, &presentation_relationships)?;
        replacements.insert("ppt/_rels/presentation.xml.rels".into(), rels.into_bytes());
    }
    if deck.slides().len() != source.slides.len()
        || deck
            .slides()
            .iter()
            .zip(&source.slides)
            .any(|(s, r)| s.id != r.slide_id)
    {
        let bytes = read(&mut input, "ppt/presentation.xml")?;
        let original = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let tree = xml(&bytes)?;
        let entries = tree
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "sldId")
            .collect::<Vec<_>>();
        if entries.len() != source.slides.len() {
            return Err("PPTX slide list changed unexpectedly".into());
        }
        let replacement = deck
            .slides()
            .iter()
            .map(|s| {
                source
                    .slides
                    .iter()
                    .find(|r| r.slide_id == s.id)
                    .map(|r| r.presentation_entry.as_str())
                    .unwrap_or_else(|| new_slide_entries.get(&s.id).unwrap().1.as_str())
            })
            .collect::<String>();
        let mut modified = original.to_owned();
        for (index, node) in entries.iter().enumerate().rev() {
            modified.replace_range(node.range(), if index == 0 { &replacement } else { "" });
        }
        replacements.insert("ppt/presentation.xml".to_owned(), modified.into_bytes());
    }
    for slide in deck.slides() {
        let record = source
            .slides
            .iter()
            .find(|r| r.slide_id == slide.source_identity());
        let Some(record) = record else {
            let path = new_slide_entries
                .get(&slide.id)
                .ok_or("New slide path missing")?
                .0
                .clone();
            let layout = source
                .slides
                .iter()
                .filter(|r| r.path.starts_with("ppt/slides/"))
                .find_map(|r| r.layout_target.as_deref())
                .ok_or("No PPTX slide layout")?;
            let (fragments, relationships, media) =
                appended_objects(slide, &slide.objects, 1, source.width, source.height)?;
            let group="<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>";
            let transition = timing_xml(slide.timing);
            let xml=format!("<p:sld xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><p:cSld><p:spTree>{group}{fragments}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>{transition}</p:sld>");
            let rels=format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"layout\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout\" Target=\"{}\"/>{relationships}</Relationships>",escape(layout));
            let (dir, file) = path.rsplit_once('/').ok_or("Invalid slide path")?;
            added_files.push((format!("{dir}/_rels/{file}.rels"), rels.into_bytes()));
            added_files.push((path, xml.into_bytes()));
            for (path, bytes) in media {
                image_extensions.insert(path.rsplit('.').next().unwrap_or("").to_owned());
                added_files.push((path, bytes));
            }
            continue;
        };
        let cloned = record.slide_id != slide.id;
        let output_path = if cloned {
            new_slide_entries
                .get(&slide.id)
                .ok_or("Duplicate slide path missing")?
                .0
                .clone()
        } else {
            record.path.clone()
        };
        let extras = slide
            .objects
            .iter()
            .filter(|o| !record.objects.iter().any(|r| r.object_id == o.id))
            .cloned()
            .collect::<Vec<_>>();
        let bytes = read(&mut input, &record.path)?;
        let content = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
        let tree = xml(&bytes)?;
        let mut patches = Vec::new();
        if slide.timing != record.timing {
            if !record.timing_editable {
                return Err("This PPTX transition cannot be safely edited".into());
            }
            let transition = timing_xml(slide.timing);
            if let Some(node) = tree
                .root_element()
                .children()
                .find(|n| n.is_element() && n.tag_name().name() == "transition")
            {
                patches.push((node.range(), transition));
            } else {
                let root = tree.root_element();
                let insertion = root
                    .children()
                    .find(|n| n.is_element() && matches!(n.tag_name().name(), "timing" | "extLst"))
                    .map(|n| n.range().start)
                    .unwrap_or_else(|| content.rfind("</").unwrap_or(root.range().end));
                patches.push((insertion..insertion, transition));
            }
        }
        let mut added_fragment = String::new();
        if !extras.is_empty() {
            let max_id = tree
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "cNvPr")
                .filter_map(|n| attr(n, "id").and_then(|v| v.parse::<u64>().ok()))
                .max()
                .unwrap_or(1);
            let (fragments, relationships, media) =
                appended_objects(slide, &extras, max_id, source.width, source.height)?;
            added_fragment = fragments;
            if !relationships.is_empty() {
                let (dir, file) = record.path.rsplit_once('/').ok_or("Invalid slide path")?;
                let relpath = format!("{dir}/_rels/{file}.rels");
                let (out_dir, out_file) = output_path
                    .rsplit_once('/')
                    .ok_or("Invalid slide output path")?;
                let output_relpath = format!("{out_dir}/_rels/{out_file}.rels");
                let mut rels=match read(&mut input,&relpath){Ok(bytes)=>String::from_utf8(bytes).map_err(|e|e.to_string())?,Err(_)=>"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".into()};
                for object in extras
                    .iter()
                    .filter(|o| matches!(o.kind, ObjectKind::Picture))
                {
                    if rels.contains(&format!("suite42_{}_{}", slide.id, object.id)) {
                        return Err("PPTX relationship ID collision".into());
                    }
                }
                append_element_content(&mut rels, &relationships)?;
                if !cloned && input.by_name(&output_relpath).is_ok() {
                    replacements.insert(output_relpath, rels.into_bytes());
                } else {
                    added_files.push((output_relpath, rels.into_bytes()));
                }
            }
            for (path, bytes) in media {
                let ext = path.rsplit('.').next().unwrap_or("").to_owned();
                image_extensions.insert(ext);
                added_files.push((path, bytes));
            }
        }
        for item in &record.objects {
            let node = tree
                .descendants()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == item.xml_type
                        && local(*n, "cNvPr").and_then(|v| attr(v, "id"))
                            == Some(item.xml_id.as_str())
                })
                .ok_or("PPTX shape missing")?;
            let Some(object) = slide.objects.iter().find(|o| o.id == item.object_id) else {
                patches.push((node.range(), String::new()));
                continue;
            };
            if object.kind != item.original.kind
                || object.group_id != item.original.group_id
                || object.fill != item.original.fill
                || object.stroke != item.original.stroke
                || object.fill_visible != item.original.fill_visible
                || object.stroke_visible != item.original.stroke_visible
                || object.media != item.original.media
            {
                return Err("Saving this PPTX object formatting is not supported yet".into());
            }
            if object.cells != item.original.cells {
                let table = local(node, "tbl").ok_or("PPTX table missing")?;
                let rows = table
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "tr")
                    .collect::<Vec<_>>();
                if object.cells.len() != rows.len()
                    || object.cells.len() != item.original.cells.len()
                {
                    return Err("Changing PPTX table size is not supported yet".into());
                }
                for (r, row) in rows.iter().enumerate() {
                    let cells = row
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "tc")
                        .collect::<Vec<_>>();
                    if cells.len() != object.cells[r].len()
                        || cells.len() != item.original.cells[r].len()
                    {
                        return Err("Changing PPTX table size is not supported yet".into());
                    }
                    for (c, cell) in cells.iter().enumerate() {
                        if object.cells[r][c] != item.original.cells[r][c] {
                            patch_text(*cell, &object.cells[r][c], content, &mut patches)?;
                        }
                    }
                }
            }
            let changed_geometry = object.x != item.original.x
                || object.y != item.original.y
                || object.width != item.original.width
                || object.height != item.original.height
                || object.rotation_degrees != item.original.rotation_degrees;
            if object.rotation_degrees.rem_euclid(360.0) != 0.0
                && ((item.transform[0] * 720.0 / source.width)
                    - (item.transform[1] * 405.0 / source.height))
                    .abs()
                    > 1e-9
            {
                return Err(
                    "Rotating objects under a nonuniform PPTX transform is not supported".into(),
                );
            }
            if item.inherited_geometry && changed_geometry {
                let props = node
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "spPr")
                    .ok_or("PPTX shape properties missing")?;
                let x = (object.x / 720.0 * source.width - item.transform[2]) / item.transform[0];
                let y = (object.y / 405.0 * source.height - item.transform[3]) / item.transform[1];
                let w = object.width / 720.0 * source.width / item.transform[0];
                let h = object.height / 405.0 * source.height / item.transform[1];
                let rot = object.rotation_degrees.rem_euclid(360.0) * 60000.0;
                let addition=format!("<a:xfrm xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" rot=\"{rot:.0}\"><a:off x=\"{x:.0}\" y=\"{y:.0}\"/><a:ext cx=\"{w:.0}\" cy=\"{h:.0}\"/></a:xfrm>");
                let mut element = content[props.range()].to_owned();
                prepend_element_content(&mut element, &addition)?;
                patches.push((props.range(), element));
            } else if !item.inherited_geometry {
                let transform = local(node, "xfrm").ok_or("PPTX transform missing")?;
                if object.rotation_degrees != item.original.rotation_degrees {
                    let value =
                        format!("{:.0}", object.rotation_degrees.rem_euclid(360.0) * 60000.0);
                    if attr(transform, "rot").is_some() {
                        replace_attr(transform, "rot", value, content, &mut patches)?;
                    } else {
                        let start = transform.range().start;
                        let end =
                            content[start..].find('>').ok_or("Invalid PPTX transform")? + start;
                        let at = if content.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
                            end - 1
                        } else {
                            end
                        };
                        patches.push((at..at, format!(" rot=\"{value}\"")));
                    }
                }
                let off = local(transform, "off").ok_or("PPTX offset missing")?;
                let ext = local(transform, "ext").ok_or("PPTX extent missing")?;
                if object.x != item.original.x {
                    replace_attr(
                        off,
                        "x",
                        format!(
                            "{:.0}",
                            (object.x / 720.0 * source.width - item.transform[2])
                                / item.transform[0]
                        ),
                        content,
                        &mut patches,
                    )?
                }
                if object.y != item.original.y {
                    replace_attr(
                        off,
                        "y",
                        format!(
                            "{:.0}",
                            (object.y / 405.0 * source.height - item.transform[3])
                                / item.transform[1]
                        ),
                        content,
                        &mut patches,
                    )?
                }
                if object.width != item.original.width {
                    replace_attr(
                        ext,
                        "cx",
                        format!(
                            "{:.0}",
                            object.width / 720.0 * source.width / item.transform[0]
                        ),
                        content,
                        &mut patches,
                    )?
                }
                if object.height != item.original.height {
                    replace_attr(
                        ext,
                        "cy",
                        format!(
                            "{:.0}",
                            object.height / 405.0 * source.height / item.transform[1]
                        ),
                        content,
                        &mut patches,
                    )?
                }
            }
            let rich_changed = object.paragraphs != item.original.paragraphs;
            if rich_changed {
                if !item.rich_safe || object.paragraphs.is_empty() {
                    return Err(
                        "This PPTX text structure cannot be safely edited as rich text".into(),
                    );
                }
                let body = node
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "txBody")
                    .ok_or("PPTX text body missing")?;
                let paragraphs = body
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "p")
                    .collect::<Vec<_>>();
                if paragraphs.is_empty() {
                    return Err("PPTX paragraph missing".into());
                }
                let replacement = rich_xml(&object.paragraphs);
                for (index, p) in paragraphs.iter().enumerate() {
                    patches.push((
                        p.range(),
                        if index == 0 {
                            replacement.clone()
                        } else {
                            String::new()
                        },
                    ));
                }
            }
            if !rich_changed && object.text != item.original.text {
                patch_text(node, &object.text, content, &mut patches)?;
            }
            if !rich_changed && object.text_style != item.original.text_style {
                patch_text_style(
                    node,
                    &item.original.text_style,
                    &object.text_style,
                    content,
                    &mut patches,
                )?;
            }
        }
        // Insertions wrap the entire shape tree, so apply existing-object patches
        // first and append the generated elements to the resulting tree below.
        patches.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        let mut updated = content.to_owned();
        for (range, value) in patches {
            updated.replace_range(range, &value)
        }
        if !added_fragment.is_empty() {
            let parsed = roxmltree::Document::parse(&updated).map_err(|e| e.to_string())?;
            let range = local(parsed.root_element(), "spTree")
                .ok_or("PPTX shape tree missing")?
                .range();
            drop(parsed);
            let mut current = updated[range.clone()].to_owned();
            append_element_content(&mut current, &added_fragment)?;
            updated.replace_range(range, &current);
        }
        if cloned {
            let (dir, file) = record.path.rsplit_once('/').ok_or("Invalid slide path")?;
            let source_relpath = format!("{dir}/_rels/{file}.rels");
            let (out_dir, out_file) = output_path
                .rsplit_once('/')
                .ok_or("Invalid output slide path")?;
            let output_relpath = format!("{out_dir}/_rels/{out_file}.rels");
            let index = added_files.iter().position(|(n, _)| n == &output_relpath);
            let mut rels = if let Some(index) = index {
                added_files.remove(index).1
            } else {
                read(&mut input, &source_relpath)?
            };
            if let Some(note) = record.notes_path.as_deref() {
                if !record.notes_cloneable {
                    return Err("Cannot safely duplicate this slide's notes".into());
                }
                let new_note = format!("ppt/notesSlides/suite42_{}.xml", slide.id);
                let new_note_rels = format!("ppt/notesSlides/_rels/suite42_{}.xml.rels", slide.id);
                let (note_dir, note_file) = note.rsplit_once('/').ok_or("Invalid notes path")?;
                let note_rels = format!("{note_dir}/_rels/{note_file}.rels");
                let note_xml = read(&mut input, note)?;
                let note_rel_xml = read(&mut input, &note_rels)?;
                rels = retarget_relationship(
                    &rels,
                    "/notesSlide",
                    &format!("../notesSlides/suite42_{}.xml", slide.id),
                )?;
                let cloned_note_rels = retarget_relationship(
                    &note_rel_xml,
                    "/slide",
                    &format!("../slides/suite42_{}.xml", slide.id),
                )?;
                slide_overrides.push_str(&format!("<Override PartName=\"/{new_note}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.notesSlide+xml\"/>"));
                added_files.push((new_note, note_xml));
                added_files.push((new_note_rels, cloned_note_rels));
            }
            added_files.push((output_relpath, rels));
            added_files.push((output_path, updated.into_bytes()));
        } else {
            replacements.insert(record.path.clone(), updated.into_bytes());
        }
    }
    if !image_extensions.is_empty() || !slide_overrides.is_empty() {
        let bytes = if let Some(v) = replacements.remove("[Content_Types].xml") {
            v
        } else {
            read(&mut input, "[Content_Types].xml")?
        };
        let mut types = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        let tree = roxmltree::Document::parse(&types).map_err(|e| e.to_string())?;
        let mut additions = slide_overrides;
        for ext in image_extensions {
            if !tree.descendants().any(|n| {
                n.is_element()
                    && n.tag_name().name() == "Default"
                    && attr(n, "Extension").is_some_and(|v| v.eq_ignore_ascii_case(&ext))
            }) {
                let mime = if ext == "png" {
                    "image/png"
                } else {
                    "image/jpeg"
                };
                additions.push_str(&format!(
                    "<Default Extension=\"{ext}\" ContentType=\"{mime}\"/>"
                ));
            }
        }
        drop(tree);
        append_element_content(&mut types, &additions)?;
        replacements.insert("[Content_Types].xml".into(), types.into_bytes());
    }
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let file = input.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_owned();
        if prune.contains(&name) {
            continue;
        }
        if let Some(bytes) = replacements.remove(&name) {
            output
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .map_err(|e| e.to_string())?;
            output.write_all(&bytes).map_err(|e| e.to_string())?;
        } else {
            output.raw_copy_file(file).map_err(|e| e.to_string())?;
        }
    }
    for (name, bytes) in added_files {
        if input.by_name(&name).is_ok() {
            return Err("PPTX generated package path collision".into());
        }
        output
            .start_file(
                name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|e| e.to_string())?;
        output.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(output.finish().map_err(|e| e.to_string())?.into_inner())
}
fn retarget_relationship(bytes: &[u8], suffix: &str, value: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let doc = xml(bytes)?;
    let nodes = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && attr(*n, "Type").is_some_and(|t| t.ends_with(suffix))
        })
        .collect::<Vec<_>>();
    if nodes.len() != 1 {
        return Err("Unsupported notes relationship structure".into());
    }
    let mut patches = Vec::new();
    replace_attr(nodes[0], "Target", value.into(), text, &mut patches)?;
    let mut out = text.to_owned();
    for (r, v) in patches {
        out.replace_range(r, &v);
    }
    Ok(out.into_bytes())
}

#[cfg(test)]
mod tests {
    #[test]
    fn timing_roundtrip_and_source_overlay() {
        use super::*;
        let mut deck = Presentation::new();
        let timing = SlideTiming {
            advance_ms: Some(2500),
            transition: Transition::PushLeft,
            duration_ms: 700,
        };
        deck.set_timing(timing).unwrap();
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].timing, SlideTiming::default());
        assert!(deck.redo());
        let (mut opened, source) = import(&export_new(&deck).unwrap()).unwrap();
        assert_eq!(opened.slides()[0].timing, timing);
        let modified = SlideTiming {
            advance_ms: None,
            transition: Transition::Fade,
            duration_ms: 300,
        };
        opened.set_timing(modified).unwrap();
        let (again, _) = import(&export(&opened, &source).unwrap()).unwrap();
        assert_eq!(again.slides()[0].timing, modified);
    }

    #[test]
    fn unsupported_transition_is_preserved_and_locked() {
        use super::*;
        let original = export_new(&Presentation::new()).unwrap();
        let mut input = ZipArchive::new(Cursor::new(original)).unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..input.len() {
            let mut part = input.by_index(i).unwrap();
            let name = part.name().to_owned();
            let mut bytes = Vec::new();
            part.read_to_end(&mut bytes).unwrap();
            if name == "ppt/slides/slide1.xml" {
                let xml = String::from_utf8(bytes).unwrap();
                let tree = roxmltree::Document::parse(&xml).unwrap();
                let range = tree
                    .root_element()
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "transition")
                    .unwrap()
                    .range();
                let mut changed = xml.clone();
                changed.replace_range(range, "<p:transition><p:dissolve/></p:transition>");
                bytes = changed.into_bytes();
            }
            output
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            output.write_all(&bytes).unwrap();
        }
        let bytes = output.finish().unwrap().into_inner();
        let (mut deck, source) = import(&bytes).unwrap();
        assert!(!source.can_edit_timing(deck.slides()[0].source_identity()));
        let saved = export(&deck, &source).unwrap();
        let mut zip = ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let xml = String::from_utf8(read(&mut zip, "ppt/slides/slide1.xml").unwrap()).unwrap();
        assert!(xml.contains("<p:dissolve/>"));
        deck.set_timing(SlideTiming {
            advance_ms: Some(1000),
            ..Default::default()
        })
        .unwrap();
        assert!(export(&deck, &source).is_err());
    }
    #[test]
    fn rotation_roundtrips_new_and_source_overlay_with_history() {
        use super::*;
        let mut deck = Presentation::new();
        let id = deck
            .insert_object(ObjectKind::Rectangle, String::new())
            .unwrap();
        let mut object = deck.slides()[0]
            .objects
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .clone();
        object.rotation_degrees = 37.0;
        deck.update_object(object).unwrap();
        assert!(deck.undo());
        assert_eq!(deck.slides()[0].objects[0].rotation_degrees, 0.0);
        assert!(deck.redo());
        let bytes = export_new(&deck).unwrap();
        let (mut reopened, source) = import(&bytes).unwrap();
        assert_eq!(reopened.slides()[0].objects[0].rotation_degrees, 37.0);
        let mut object = reopened.slides()[0].objects[0].clone();
        object.rotation_degrees = 82.0;
        reopened.update_object(object).unwrap();
        let bytes = export(&reopened, &source).unwrap();
        let (reopened, _) = import(&bytes).unwrap();
        assert_eq!(reopened.slides()[0].objects[0].rotation_degrees, 82.0);
    }
    use super::*;
    #[test]
    fn duplicate_slide_clones_notes_and_backlink() {
        let deck = Presentation::new();
        let base = export_new(&deck).unwrap();
        let mut input = ZipArchive::new(Cursor::new(base.as_slice())).unwrap();
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for i in 0..input.len() {
            let mut f = input.by_index(i).unwrap();
            let name = f.name().to_owned();
            if name == "ppt/slides/_rels/slide1.xml.rels" {
                let mut xml = String::new();
                f.read_to_string(&mut xml).unwrap();
                append_element_content(&mut xml,"<Relationship Id=\"notes\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/notesSlide\" Target=\"../notesSlides/notesSlide1.xml\"/>").unwrap();
                out.start_file(name, opt).unwrap();
                out.write_all(xml.as_bytes()).unwrap();
            } else {
                out.raw_copy_file(f).unwrap();
            }
        }
        out.start_file("ppt/notesSlides/notesSlide1.xml", opt)
            .unwrap();
        out.write_all(b"<notes>important speaker notes</notes>")
            .unwrap();
        out.start_file("ppt/notesSlides/_rels/notesSlide1.xml.rels", opt)
            .unwrap();
        out.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="back" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="../slides/slide1.xml"/></Relationships>"#).unwrap();
        let input = out.finish().unwrap().into_inner();
        let (mut deck, source) = import(&input).unwrap();
        assert!(source.can_duplicate(deck.slides()[0].id));
        deck.duplicate().unwrap();
        let copied_id = deck.slides()[1].id;
        let bytes = export(&deck, &source).unwrap();
        let mut z = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let note = format!("ppt/notesSlides/suite42_{copied_id}.xml");
        assert_eq!(
            read(&mut z, &note).unwrap(),
            b"<notes>important speaker notes</notes>"
        );
        let rels = String::from_utf8(
            read(
                &mut z,
                &format!("ppt/notesSlides/_rels/suite42_{copied_id}.xml.rels"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(rels.contains(&format!("../slides/suite42_{copied_id}.xml")));
        let slide_rels = String::from_utf8(
            read(
                &mut z,
                &format!("ppt/slides/_rels/suite42_{copied_id}.xml.rels"),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(slide_rels.contains(&format!("../notesSlides/suite42_{copied_id}.xml")));
    }
    #[test]
    fn new_pptx_roundtrips_native_shapes_image_table_and_slides() {
        let mut deck = Presentation::new();
        deck.insert_object(ObjectKind::Rectangle, "hello".into())
            .unwrap();
        deck.insert_object(ObjectKind::Line, String::new()).unwrap();
        deck.insert_picture(b"\x89PNG\r\n\x1a\nfixture".to_vec())
            .unwrap();
        let id = deck.insert_table(1, 1).unwrap();
        let mut t = deck.slides()[0]
            .objects
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .clone();
        t.cells[0][0] = "cell".into();
        deck.update_object(t).unwrap();
        deck.add().unwrap();
        deck.insert_object(ObjectKind::Ellipse, "second".into())
            .unwrap();
        let bytes = export_new(&deck).unwrap();
        let (reopened, _) = import(&bytes).unwrap();
        assert_eq!(reopened.slides().len(), 2);
        assert_eq!(reopened.slides()[0].objects.len(), 4);
        assert_eq!(reopened.slides()[0].objects[0].text, "hello");
        assert_eq!(reopened.slides()[0].objects[1].kind, ObjectKind::Line);
        assert_eq!(
            reopened.slides()[0].objects[2].media,
            b"\x89PNG\r\n\x1a\nfixture"
        );
        assert_eq!(reopened.slides()[0].objects[3].cells[0][0], "cell");
        assert_eq!(reopened.slides()[1].objects[0].kind, ObjectKind::Ellipse);
        let (mut external, source) = import(&bytes).unwrap();
        external
            .insert_object(ObjectKind::Text, "inserted".into())
            .unwrap();
        external
            .insert_picture(b"\xff\xd8\xfffixture".to_vec())
            .unwrap();
        external.insert_table(1, 1).unwrap();
        let expanded = export(&external, &source).unwrap();
        let (loaded, _) = import(&expanded).unwrap();
        assert_eq!(loaded.slides()[0].objects.len(), 7);
        assert_eq!(loaded.slides()[0].objects[4].text, "inserted");
        assert_eq!(loaded.slides()[0].objects[5].media, b"\xff\xd8\xfffixture");
        let (mut extended, original) = import(&bytes).unwrap();
        extended.duplicate().unwrap();
        let mut copied = extended.slides()[1].objects[0].clone();
        copied.text = "copy".into();
        extended.update_object(copied).unwrap();
        extended.add().unwrap();
        extended
            .insert_object(ObjectKind::Text, "new slide".into())
            .unwrap();
        let result = export(&extended, &original).unwrap();
        let (loaded, _) = import(&result).unwrap();
        assert_eq!(loaded.slides().len(), 4);
        assert_eq!(loaded.slides()[1].objects[0].text, "copy");
        assert_eq!(loaded.slides()[2].objects[0].text, "new slide");
        external.move_slide(1, 0).unwrap();
        let reordered = export(&external, &source).unwrap();
        let (again, _) = import(&reordered).unwrap();
        assert_eq!(again.slides()[0].objects[0].kind, ObjectKind::Ellipse);
        external.delete().unwrap();
        let reduced = export(&external, &source).unwrap();
        let mut package = ZipArchive::new(Cursor::new(reduced.as_slice())).unwrap();
        assert!(package.by_name("ppt/slides/slide2.xml").is_err());
        assert!(package.by_name("ppt/slides/slide1.xml").is_ok());
        let (again, _) = import(&reduced).unwrap();
        assert_eq!(again.slides().len(), 1);
    }
    #[test]
    fn external_package_overlay_preserves_unknown_parts_and_updates_geometry() {
        let mut z = ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        let files = [
            (
                "ppt/presentation.xml",
                r#"<p:presentation xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><p:sldIdLst><p:sldId id="256" r:id="rId1"/></p:sldIdLst><p:sldSz cx="9144000" cy="5143500"/></p:presentation>"#,
            ),
            (
                "ppt/_rels/presentation.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Target="slides/slide1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide"/></Relationships>"#,
            ),
            (
                "ppt/slides/slide1.xml",
                r#"<p:sld xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:spTree><p:sp><p:nvSpPr><p:cNvPr id="2" name="Text"/></p:nvSpPr><p:spPr><a:xfrm><a:off x="1270000" y="635000"/><a:ext cx="2540000" cy="1270000"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr><p:txBody><a:p><a:r><a:rPr b="1"/><a:t>Source</a:t></a:r></a:p></p:txBody></p:sp><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="3" name="opaque"/></p:nvGraphicFramePr></p:graphicFrame></p:spTree></p:cSld></p:sld>"#,
            ),
            (
                "ppt/notesSlides/notesSlide1.xml",
                "<opaque>important notes</opaque>",
            ),
        ];
        for (name, body) in files {
            z.start_file(name, opt).unwrap();
            if name == "ppt/slides/slide1.xml" {
                let additions = r#"<p:pic><p:nvPicPr><p:cNvPr id="4" name="Picture"/></p:nvPicPr><p:blipFill><a:blip r:embed="img"/></p:blipFill><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="1270000"/></a:xfrm></p:spPr></p:pic><p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id="5" name="Table"/></p:nvGraphicFramePr><p:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="1270000"/></p:xfrm><a:graphic><a:graphicData><a:tbl><a:tr><a:tc><a:txBody><a:p><a:r><a:t>Cell</a:t></a:r></a:p></a:txBody></a:tc></a:tr></a:tbl></a:graphicData></a:graphic></p:graphicFrame>"#;
                let group = r#"<p:grpSp><p:nvGrpSpPr><p:cNvPr id="6" name="Group"/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="1000000" y="500000"/><a:ext cx="2000000" cy="2000000"/><a:chOff x="0" y="0"/><a:chExt cx="1000000" cy="1000000"/></a:xfrm></p:grpSpPr><p:sp><p:nvSpPr><p:cNvPr id="7" name="Child"/></p:nvSpPr><p:spPr><a:xfrm><a:off x="100000" y="100000"/><a:ext cx="300000" cy="300000"/></a:xfrm><a:prstGeom prst="rect"/></p:spPr></p:sp></p:grpSp>"#;
                let placeholder = r#"<p:sp><p:nvSpPr><p:cNvPr id="8" name="Placeholder"/><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr><p:spPr/><p:txBody><a:p><a:r><a:t>Inherited</a:t></a:r></a:p></p:txBody></p:sp>"#;
                let extended=body.replace("<p:sld ","<p:sld xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" ").replace("</p:spTree>",&format!("{additions}{group}{placeholder}</p:spTree>"));
                z.write_all(extended.as_bytes()).unwrap();
            } else {
                z.write_all(body.as_bytes()).unwrap();
            }
        }
        z.start_file("ppt/slides/_rels/slide1.xml.rels", opt)
            .unwrap();
        z.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="img" Target="../media/image1.png" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image"/><Relationship Id="layout" Target="../slideLayouts/slideLayout1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout"/></Relationships>"#).unwrap();
        z.start_file("ppt/slideLayouts/slideLayout1.xml", opt)
            .unwrap();
        z.write_all(br#"<p:sldLayout xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:sp><p:nvSpPr><p:nvPr><p:ph idx="2"/></p:nvPr></p:nvSpPr><p:spPr><a:xfrm><a:off x="2540000" y="1270000"/><a:ext cx="2540000" cy="1270000"/></a:xfrm></p:spPr></p:sp></p:sldLayout>"#).unwrap();
        z.start_file("ppt/slideLayouts/_rels/slideLayout1.xml.rels", opt)
            .unwrap();
        z.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="master" Target="../slideMasters/slideMaster1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster"/></Relationships>"#).unwrap();
        z.start_file("ppt/slideMasters/slideMaster1.xml", opt)
            .unwrap();
        z.write_all(br#"<p:sldMaster xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><p:cSld><p:bg><p:bgPr><a:solidFill><a:schemeClr val="bg1"/></a:solidFill></p:bgPr></p:bg><p:spTree><p:sp><p:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1270000" cy="1270000"/></a:xfrm><a:prstGeom prst="rect"/><a:solidFill><a:srgbClr val="AABBCC"/></a:solidFill></p:spPr></p:sp></p:spTree></p:cSld><p:clrMap bg1="lt1"/></p:sldMaster>"#).unwrap();
        z.start_file("ppt/slideMasters/_rels/slideMaster1.xml.rels", opt)
            .unwrap();
        z.write_all(br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="theme" Target="../theme/theme1.xml" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme"/></Relationships>"#).unwrap();
        z.start_file("ppt/theme/theme1.xml", opt).unwrap();
        z.write_all(br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:clrScheme><a:lt1><a:srgbClr val="123456"/></a:lt1></a:clrScheme></a:themeElements></a:theme>"#).unwrap();
        z.start_file("ppt/media/image1.png", opt).unwrap();
        z.write_all(b"image bytes").unwrap();
        let input = z.finish().unwrap().into_inner();
        let (mut deck, source) = import(&input).unwrap();
        assert_eq!(deck.slides()[0].objects.len(), 5);
        assert_eq!(
            source.background(deck.slides()[0].id),
            Some([0x12, 0x34, 0x56])
        );
        assert_eq!(source.decorations(deck.slides()[0].id).len(), 1);
        let mut placeholder = deck.slides()[0].objects[4].clone();
        let original_x = placeholder.x;
        placeholder.x += 30.0;
        deck.update_object(placeholder).unwrap();
        let mut child = deck.slides()[0].objects[3].clone();
        let x = child.x;
        child.x += 25.0;
        deck.update_object(child).unwrap();
        assert_eq!(deck.slides()[0].objects[1].media, b"image bytes");
        let mut table = deck.slides()[0].objects[2].clone();
        table.cells[0][0] = "Edited cell".into();
        deck.update_object(table).unwrap();
        let mut object = deck.slides()[0].objects[0].clone();
        object.x += 72.0;
        object.text = "Changed & safe".into();
        object.text_style.family = "Calibri".into();
        object.text_style.points = 24.0;
        object.text_style.color = [12, 34, 56];
        object.text_style.align = TextAlign::Center;
        deck.update_object(object).unwrap();
        let out = export(&deck, &source).unwrap();
        let (reopened, _) = import(&out).unwrap();
        assert_eq!(reopened.slides()[0].objects[0].text, "Changed & safe");
        let style = &reopened.slides()[0].objects[0].text_style;
        assert_eq!(style.family, "Calibri");
        assert_eq!(style.points, 24.0);
        assert_eq!(style.color, [12, 34, 56]);
        assert_eq!(style.align, TextAlign::Center);
        assert!(style.bold);
        assert_eq!(reopened.slides()[0].objects[2].cells[0][0], "Edited cell");
        assert_eq!(reopened.slides()[0].objects[1].media, b"image bytes");
        assert!((reopened.slides()[0].objects[3].x - (x + 25.0)).abs() < 0.001);
        assert!((reopened.slides()[0].objects[4].x - (original_x + 30.0)).abs() < 0.001);
        let mut archive = ZipArchive::new(Cursor::new(out.as_slice())).unwrap();
        assert_eq!(
            read(&mut archive, "ppt/notesSlides/notesSlide1.xml").unwrap(),
            b"<opaque>important notes</opaque>"
        );
        let slide =
            String::from_utf8(read(&mut archive, "ppt/slides/slide1.xml").unwrap()).unwrap();
        assert!(slide.contains("graphicFrame"));
        assert!(slide.contains("b=\"1\""));
    }
}

/// Builds a standards-based PPTX package for a native deck; media remain embedded.
pub fn export_new(deck: &Presentation) -> Result<Vec<u8>, String> {
    fn emu(v: f64, extent: f64, canvas: f64) -> i64 {
        (v / canvas * extent).round() as i64
    }
    fn run_piece(s: &str, style: &TextStyle) -> String {
        let underline = if style.underline { "sng" } else { "none" };
        format!("<a:r><a:rPr sz=\"{:.0}\" b=\"{}\" i=\"{}\" u=\"{underline}\"><a:solidFill><a:srgbClr val=\"{:02X}{:02X}{:02X}\"/></a:solidFill><a:latin typeface=\"{}\"/></a:rPr><a:t xml:space=\"preserve\">{}</a:t></a:r>",style.points*100.0,style.bold as u8,style.italic as u8,style.color[0],style.color[1],style.color[2],escape(&style.family),escape(s))
    }
    fn paragraph(runs: &[RichRun], align: TextAlign, bullet: bool) -> String {
        let align = match align {
            TextAlign::Left => "l",
            TextAlign::Center => "ctr",
            TextAlign::Right => "r",
        };
        let bullet = if bullet {
            "<a:buChar char=\"•\"/>"
        } else {
            "<a:buNone/>"
        };
        let runs = runs
            .iter()
            .map(|r| run_piece(&r.text, &r.style))
            .collect::<String>();
        format!("<a:p><a:pPr algn=\"{align}\">{bullet}</a:pPr>{runs}</a:p>")
    }
    fn run(s: &str, style: &TextStyle) -> String {
        let _align = match style.align {
            TextAlign::Left => "l",
            TextAlign::Center => "ctr",
            TextAlign::Right => "r",
        };
        s.split('\n')
            .map(|p| {
                paragraph(
                    &[RichRun {
                        text: p.into(),
                        style: style.clone(),
                    }],
                    style.align,
                    false,
                )
            })
            .collect()
    }
    let pns = "http://schemas.openxmlformats.org/presentationml/2006/main";
    let ans = "http://schemas.openxmlformats.org/drawingml/2006/main";
    let rns = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let pkg = "http://schemas.openxmlformats.org/package/2006/relationships";
    let width = 9144000.0;
    let height = 5143500.0;
    let mut parts: Vec<(String, Vec<u8>)> = Vec::new();
    let mut overrides = String::new();
    let mut slide_ids = String::new();
    let mut pres_rels=format!("<Relationship Id=\"master\" Type=\"{rns}/slideMaster\" Target=\"slideMasters/slideMaster1.xml\"/>");
    let group="<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>";
    for (i, slide) in deck.slides().iter().enumerate() {
        let n = i + 1;
        slide_ids.push_str(&format!("<p:sldId id=\"{}\" r:id=\"slide{n}\"/>", 255 + n));
        pres_rels.push_str(&format!(
            "<Relationship Id=\"slide{n}\" Type=\"{rns}/slide\" Target=\"slides/slide{n}.xml\"/>"
        ));
        overrides.push_str(&format!("<Override PartName=\"/ppt/slides/slide{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slide+xml\"/>"));
        let mut objects = slide.objects.clone();
        let mut next = objects
            .iter()
            .map(|o| o.id)
            .max()
            .unwrap_or(0)
            .saturating_add(2);
        for (row, value) in [&slide.title, &slide.body].iter().enumerate() {
            if !value.is_empty() {
                objects.push(SlideObject {
                    id: next,
                    group_id: None,
                    x: 35.0,
                    y: 30.0 + row as f64 * 55.0,
                    width: 650.0,
                    height: 50.0,
                    rotation_degrees: 0.0,
                    kind: ObjectKind::Text,
                    fill: [255, 255, 255],
                    stroke: [255, 255, 255],
                    fill_visible: false,
                    stroke_visible: false,
                    text: (*value).clone(),
                    text_style: TextStyle::default(),
                    paragraphs: Vec::new(),
                    media: Vec::new(),
                    cells: Vec::new(),
                });
                next = next.saturating_add(1);
            }
        }
        let mut shapes = String::new();
        let mut group_shapes: HashMap<u64, String> = HashMap::new();
        let mut group_order = Vec::new();
        let mut rels=format!("<Relationship Id=\"layout\" Type=\"{rns}/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/>");
        for (j, o) in objects.iter().enumerate() {
            if let Some(group_id) = o.group_id {
                if !group_shapes.contains_key(&group_id) {
                    shapes.push_str(&format!("<!--suite42-group-{group_id}-->"));
                    group_shapes.insert(group_id, String::new());
                    group_order.push(group_id);
                }
            }
            let target = if let Some(group_id) = o.group_id {
                group_shapes.get_mut(&group_id).unwrap()
            } else {
                &mut shapes
            };
            let id = j + 2;
            let x = emu(o.x, width, 720.0);
            let y = emu(o.y, height, 405.0);
            let cx = emu(o.width, width, 720.0).max(1);
            let cy = emu(o.height, height, 405.0).max(1);
            let xfrm = format!(
                "<a:xfrm rot=\"{:.0}\"><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm>", o.rotation_degrees.rem_euclid(360.0) * 60000.0
            );
            let fill = format!("{:02X}{:02X}{:02X}", o.fill[0], o.fill[1], o.fill[2]);
            let line = format!("{:02X}{:02X}{:02X}", o.stroke[0], o.stroke[1], o.stroke[2]);
            let rot = (o.rotation_degrees.rem_euclid(360.0) * 60000.0).round() as i64;
            match o.kind {
                ObjectKind::Rectangle
                | ObjectKind::Ellipse
                | ObjectKind::Line
                | ObjectKind::Text => {
                    let geom = match o.kind {
                        ObjectKind::Ellipse => "ellipse",
                        ObjectKind::Line => "line",
                        _ => "rect",
                    };
                    let body = if o.text.is_empty() && !matches!(o.kind, ObjectKind::Text) {
                        String::new()
                    } else {
                        format!(
                            "<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody>",
                            if o.paragraphs.is_empty() {
                                run(&o.text, &o.text_style)
                            } else {
                                o.paragraphs
                                    .iter()
                                    .map(|p| paragraph(&p.runs, p.align, p.bullet))
                                    .collect()
                            }
                        )
                    };
                    let fill_xml = if o.fill_visible {
                        format!("<a:solidFill><a:srgbClr val=\"{fill}\"/></a:solidFill>")
                    } else {
                        "<a:noFill/>".into()
                    };
                    let line_xml = if o.stroke_visible {
                        format!(
                            "<a:ln><a:solidFill><a:srgbClr val=\"{line}\"/></a:solidFill></a:ln>"
                        )
                    } else {
                        "<a:ln><a:noFill/></a:ln>".into()
                    };
                    target.push_str(&format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Object {id}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{xfrm}<a:prstGeom prst=\"{geom}\"><a:avLst/></a:prstGeom>{fill_xml}{line_xml}</p:spPr>{body}</p:sp>"));
                }
                ObjectKind::Picture => {
                    let (ext, _mime) = if o.media.starts_with(b"\x89PNG\r\n\x1a\n") {
                        ("png", "image/png")
                    } else if o.media.starts_with(b"\xff\xd8\xff") {
                        ("jpg", "image/jpeg")
                    } else {
                        return Err("Unsupported picture encoding".into());
                    };
                    let rid = format!("image{id}");
                    let name = format!("image{n}_{id}.{ext}");
                    rels.push_str(&format!("<Relationship Id=\"{rid}\" Type=\"{rns}/image\" Target=\"../media/{name}\"/>"));
                    parts.push((format!("ppt/media/{name}"), o.media.clone()));
                    target.push_str(&format!("<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Picture {id}\"/><p:cNvPicPr/><p:nvPr/></p:nvPicPr><p:blipFill><a:blip r:embed=\"{rid}\"/><a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{xfrm}<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></p:spPr></p:pic>"));
                }
                ObjectKind::Table => {
                    let cols = o.cells.first().map_or(0, Vec::len);
                    if cols == 0 || o.cells.is_empty() || o.cells.iter().any(|r| r.len() != cols) {
                        return Err("Invalid table".into());
                    }
                    let mut grid = String::new();
                    for _ in 0..cols {
                        grid.push_str(&format!("<a:gridCol w=\"{}\"/>", cx / cols as i64));
                    }
                    let mut rows = String::new();
                    for row in &o.cells {
                        rows.push_str(&format!("<a:tr h=\"{}\">", cy / o.cells.len() as i64));
                        for cell in row {
                            rows.push_str(&format!("<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr/></a:tc>",run(cell,&TextStyle::default())));
                        }
                        rows.push_str("</a:tr>");
                    }
                    target.push_str(&format!("<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr/><p:nvPr/></p:nvGraphicFramePr><p:xfrm rot=\"{rot}\"><a:off x=\"{x}\" y=\"{y}\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></p:xfrm><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr/><a:tblGrid>{grid}</a:tblGrid>{rows}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>"));
                }
            }
        }
        for (index, group_id) in group_order.iter().enumerate() {
            let id = objects.len() + 2 + index;
            let children = group_shapes.remove(group_id).unwrap();
            let xml=format!("<p:grpSp><p:nvGrpSpPr><p:cNvPr id=\"{id}\" name=\"Group {id}\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"9144000\" cy=\"5143500\"/><a:chOff x=\"0\" y=\"0\"/><a:chExt cx=\"9144000\" cy=\"5143500\"/></a:xfrm></p:grpSpPr>{children}</p:grpSp>");
            shapes = shapes.replace(&format!("<!--suite42-group-{group_id}-->"), &xml);
        }
        let transition = timing_xml(slide.timing);
        parts.push((format!("ppt/slides/slide{n}.xml"),format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><p:sld xmlns:p=\"{pns}\" xmlns:a=\"{ans}\" xmlns:r=\"{rns}\"><p:cSld><p:spTree>{group}{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr>{transition}</p:sld>").into_bytes()));
        parts.push((
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            format!("<Relationships xmlns=\"{pkg}\">{rels}</Relationships>").into_bytes(),
        ));
    }
    parts.push(("ppt/presentation.xml".into(),format!("<p:presentation xmlns:p=\"{pns}\" xmlns:r=\"{rns}\"><p:sldMasterIdLst><p:sldMasterId id=\"2147483648\" r:id=\"master\"/></p:sldMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx=\"9144000\" cy=\"5143500\"/><p:notesSz cx=\"6858000\" cy=\"9144000\"/></p:presentation>").into_bytes()));
    parts.push((
        "ppt/_rels/presentation.xml.rels".into(),
        format!("<Relationships xmlns=\"{pkg}\">{pres_rels}</Relationships>").into_bytes(),
    ));
    parts.push(("_rels/.rels".into(),format!("<Relationships xmlns=\"{pkg}\"><Relationship Id=\"main\" Type=\"{rns}/officeDocument\" Target=\"ppt/presentation.xml\"/></Relationships>").into_bytes()));
    let master=format!("<p:sldMaster xmlns:p=\"{pns}\" xmlns:a=\"{ans}\" xmlns:r=\"{rns}\"><p:cSld><p:spTree>{group}</p:spTree></p:cSld><p:clrMap bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"/><p:sldLayoutIdLst><p:sldLayoutId id=\"2147483649\" r:id=\"layout\"/></p:sldLayoutIdLst><p:txStyles/></p:sldMaster>");
    parts.push((
        "ppt/slideMasters/slideMaster1.xml".into(),
        master.into_bytes(),
    ));
    parts.push(("ppt/slideMasters/_rels/slideMaster1.xml.rels".into(),format!("<Relationships xmlns=\"{pkg}\"><Relationship Id=\"layout\" Type=\"{rns}/slideLayout\" Target=\"../slideLayouts/slideLayout1.xml\"/><Relationship Id=\"theme\" Type=\"{rns}/theme\" Target=\"../theme/theme1.xml\"/></Relationships>").into_bytes()));
    parts.push(("ppt/slideLayouts/slideLayout1.xml".into(),format!("<p:sldLayout xmlns:p=\"{pns}\" xmlns:a=\"{ans}\" xmlns:r=\"{rns}\" type=\"blank\" preserve=\"1\"><p:cSld name=\"Blank\"><p:spTree>{group}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>").into_bytes()));
    parts.push(("ppt/slideLayouts/_rels/slideLayout1.xml.rels".into(),format!("<Relationships xmlns=\"{pkg}\"><Relationship Id=\"master\" Type=\"{rns}/slideMaster\" Target=\"../slideMasters/slideMaster1.xml\"/></Relationships>").into_bytes()));
    let mut colors = String::new();
    for (key, value) in [
        ("dk1", "000000"),
        ("lt1", "FFFFFF"),
        ("dk2", "44546A"),
        ("lt2", "E7E6E6"),
        ("accent1", "4472C4"),
        ("accent2", "ED7D31"),
        ("accent3", "A5A5A5"),
        ("accent4", "FFC000"),
        ("accent5", "5B9BD5"),
        ("accent6", "70AD47"),
        ("hlink", "0563C1"),
        ("folHlink", "954F72"),
    ] {
        colors.push_str(&format!("<a:{key}><a:srgbClr val=\"{value}\"/></a:{key}>"));
    }
    let solid = "<a:solidFill><a:schemeClr val=\"phClr\"/></a:solidFill>";
    let theme=format!("<a:theme xmlns:a=\"{ans}\" name=\"Suite42\"><a:themeElements><a:clrScheme name=\"Suite42\">{colors}</a:clrScheme><a:fontScheme name=\"Suite42\"><a:majorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:majorFont><a:minorFont><a:latin typeface=\"Arial\"/><a:ea typeface=\"\"/><a:cs typeface=\"\"/></a:minorFont></a:fontScheme><a:fmtScheme name=\"Suite42\"><a:fillStyleLst>{}</a:fillStyleLst><a:lnStyleLst>{}</a:lnStyleLst><a:effectStyleLst>{}</a:effectStyleLst><a:bgFillStyleLst>{}</a:bgFillStyleLst></a:fmtScheme></a:themeElements></a:theme>",solid.repeat(3),format!("<a:ln>{solid}</a:ln>").repeat(3),"<a:effectStyle><a:effectLst/></a:effectStyle>".repeat(3),solid.repeat(3));
    parts.push(("ppt/theme/theme1.xml".into(), theme.into_bytes()));
    let types=format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"xml\" ContentType=\"application/xml\"/><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"png\" ContentType=\"image/png\"/><Default Extension=\"jpg\" ContentType=\"image/jpeg\"/><Override PartName=\"/ppt/presentation.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml\"/><Override PartName=\"/ppt/slideMasters/slideMaster1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml\"/><Override PartName=\"/ppt/slideLayouts/slideLayout1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml\"/><Override PartName=\"/ppt/theme/theme1.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.theme+xml\"/>{overrides}</Types>");
    parts.push(("[Content_Types].xml".into(), types.into_bytes()));
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opt = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, data) in parts {
        zip.start_file(name, opt).map_err(|e| e.to_string())?;
        zip.write_all(&data).map_err(|e| e.to_string())?;
    }
    Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
}

fn append_element_content(element: &mut String, addition: &str) -> Result<(), String> {
    if let Some(end) = element.rfind("</") {
        element.insert_str(end, addition);
        return Ok(());
    }
    let close = element.rfind("/>").ok_or("Invalid XML element")?;
    let start = element[..close].rfind('<').ok_or("Invalid XML element")? + 1;
    let name = element[start..close]
        .split_whitespace()
        .next()
        .ok_or("Invalid XML element")?
        .to_owned();
    element.replace_range(close..close + 2, &format!(">{addition}</{name}>"));
    Ok(())
}
fn prepend_element_content(element: &mut String, addition: &str) -> Result<(), String> {
    let end = element.find('>').ok_or("Invalid XML element")?;
    if element.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
        return append_element_content(element, addition);
    }
    element.insert_str(end + 1, addition);
    Ok(())
}
fn appended_objects(
    slide: &Slide,
    extras: &[SlideObject],
    max_id: u64,
    width: f64,
    height: f64,
) -> Result<(String, String, Vec<(String, Vec<u8>)>), String> {
    let mut temp_slide = slide.clone();
    temp_slide.objects = extras
        .iter()
        .cloned()
        .map(|mut o| {
            o.x *= width / 9144000.0;
            o.width *= width / 9144000.0;
            o.y *= height / 5143500.0;
            o.height *= height / 5143500.0;
            o
        })
        .collect();
    temp_slide.title.clear();
    temp_slide.body.clear();
    temp_slide.title_text = TextValue::from_raw(String::new());
    temp_slide.body_text = TextValue::from_raw(String::new());
    let next_id = slide.id.checked_add(1).ok_or("slide identity exhausted")?;
    let state = State {
        deck: Deck {
            version: 2,
            next_id,
            slides: vec![temp_slide],
        },
        selected: slide.id,
    };
    let temp = Presentation {
        history: SnapshotHistory::new(state, HISTORY_LIMIT),
    };
    let bytes = export_new(&temp)?;
    let mut z = ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(|e| e.to_string())?;
    let xmlbytes = read(&mut z, "ppt/slides/slide1.xml")?;
    let xmltext = std::str::from_utf8(&xmlbytes).map_err(|e| e.to_string())?;
    let doc = xml(&xmlbytes)?;
    let tree = local(doc.root_element(), "spTree").ok_or("Generated shape tree missing")?;
    let nodes = tree
        .children()
        .filter(|n| n.is_element() && matches!(n.tag_name().name(), "sp" | "pic" | "graphicFrame"))
        .collect::<Vec<_>>();
    if nodes.len() != extras.len() {
        return Err("Generated shape mismatch".into());
    }
    let mut fragments = String::new();
    let mut relationships = String::new();
    let mut media = Vec::new();
    for (index, (node, object)) in nodes.iter().zip(extras).enumerate() {
        let old = index + 2;
        let new = max_id
            .checked_add(index as u64 + 1)
            .ok_or("PPTX object ID exhausted")?;
        let mut fragment = xmltext[node.range()].to_owned();
        fragment = fragment.replacen(
            &format!("<p:cNvPr id=\"{old}\""),
            &format!("<p:cNvPr id=\"{new}\""),
            1,
        );
        let insert = fragment
            .find(|c: char| c == '>' || c.is_whitespace())
            .ok_or("Invalid generated shape")?;
        fragment.insert_str(insert," xmlns:p=\"http://schemas.openxmlformats.org/presentationml/2006/main\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"");
        if matches!(object.kind, ObjectKind::Picture) {
            let ext = if object.media.starts_with(b"\x89PNG\r\n\x1a\n") {
                "png"
            } else if object.media.starts_with(b"\xff\xd8\xff") {
                "jpg"
            } else {
                return Err("Unsupported image encoding".into());
            };
            let rid = format!("suite42_{}_{}", slide.id, object.id);
            let file = format!("suite42_{}_{}.{ext}", slide.id, object.id);
            fragment = fragment.replace(
                &format!("r:embed=\"image{old}\""),
                &format!("r:embed=\"{rid}\""),
            );
            relationships.push_str(&format!("<Relationship Id=\"{rid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/image\" Target=\"../media/{file}\"/>"));
            media.push((format!("ppt/media/{file}"), object.media.clone()));
        }
        fragments.push_str(&fragment);
    }
    Ok((fragments, relationships, media))
}
