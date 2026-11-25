//! OpenDocument Presentation package adapter for the supported slide object subset.
use super::*;
use std::io::{Cursor, Read, Write};
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};

struct ObjectSource {
    object_id: u64,
    start: usize,
    original: SlideObject,
    writable_cells: Vec<Vec<bool>>,
    text_editable: bool,
    rich_safe: bool,
}
struct PageSource {
    slide_id: u64,
    objects: Vec<ObjectSource>,
    timing: SlideTiming,
    timing_editable: bool,
}
pub struct Source {
    bytes: Vec<u8>,
    pages: Vec<PageSource>,
    size_points: Option<(f64, f64)>,
}
impl Source {
    pub fn can_edit_timing(&self, slide: u64) -> bool {
        self.pages
            .iter()
            .find(|p| p.slide_id == slide)
            .is_none_or(|p| p.timing_editable)
    }
    pub fn can_edit_rich(&self, slide: u64, object: u64) -> bool {
        self.pages
            .iter()
            .find(|p| p.slide_id == slide)
            .and_then(|p| p.objects.iter().find(|o| o.object_id == object))
            .map(|o| o.rich_safe)
            .unwrap_or(true)
    }
    pub fn size_points(&self) -> Option<(f64, f64)> {
        self.size_points
    }
    pub fn can_delete_slide(&self, slide: u64) -> bool {
        let Some(index) = self.pages.iter().position(|p| p.slide_id == slide) else {
            return true;
        };
        let Ok(mut archive) = ZipArchive::new(Cursor::new(self.bytes.as_slice())) else {
            return false;
        };
        let Ok(bytes) = zip_read(&mut archive, "content.xml") else {
            return false;
        };
        let Ok(text) = std::str::from_utf8(&bytes) else {
            return false;
        };
        let Ok(doc) = roxmltree::Document::parse(text) else {
            return false;
        };
        let pages = doc
            .descendants()
            .filter(|n| {
                n.is_element()
                    && n.tag_name().name() == "page"
                    && n.tag_name().namespace()
                        == Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0")
            })
            .collect::<Vec<_>>();
        let Some(page) = pages.get(index) else {
            return false;
        };
        let names = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_owned()))
            .collect::<HashSet<_>>();
        for a in page
            .descendants()
            .filter(|n| n.is_element())
            .flat_map(|n| n.attributes())
        {
            if a.name() != "href" {
                continue;
            }
            let href = a.value().trim_start_matches("./");
            if href.starts_with('#') || href.contains("://") || href.starts_with("mailto:") {
                continue;
            }
            if href.contains('%')
                || href.starts_with('/')
                || href.split('/').any(|s| s == "..")
                || (names.contains(href) && !href.starts_with("Pictures/"))
            {
                return false;
            }
        }
        names.contains("META-INF/manifest.xml")
    }
    pub fn can_edit_text(&self, slide: u64, object: u64) -> bool {
        self.pages
            .iter()
            .find(|p| p.slide_id == slide)
            .and_then(|p| p.objects.iter().find(|o| o.object_id == object))
            .map(|o| o.text_editable)
            .unwrap_or(true)
    }
    pub fn can_edit_cell(&self, slide: u64, object: u64, row: usize, col: usize) -> bool {
        let object = self
            .pages
            .iter()
            .find(|p| p.slide_id == slide)
            .and_then(|p| p.objects.iter().find(|o| o.object_id == object));
        let Some(object) = object else { return true };
        object
            .writable_cells
            .get(row)
            .and_then(|r| r.get(col))
            .copied()
            .unwrap_or(false)
    }
    pub fn is_original_object(&self, slide: u64, object: u64) -> bool {
        self.pages
            .iter()
            .find(|p| p.slide_id == slide)
            .is_some_and(|p| p.objects.iter().any(|o| o.object_id == object))
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn pt(n: f64) -> String {
    format!("{n:.4}pt")
}
fn parse_timing(
    page: roxmltree::Node<'_, '_>,
    tree: &roxmltree::Document<'_>,
) -> Option<SlideTiming> {
    let Some(name) = attr(page, "style-name") else {
        return Some(SlideTiming::default());
    };
    let style = tree.descendants().find(|n| {
        n.is_element()
            && n.tag_name().name() == "style"
            && attr(*n, "name") == Some(name)
            && attr(*n, "family") == Some("drawing-page")
    })?;
    if attr(style, "parent-style-name").is_some() {
        return None;
    }
    let Some(props) = style
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "drawing-page-properties")
    else {
        return Some(SlideTiming::default());
    };
    let transition = match (
        attr(props, "type"),
        attr(props, "subtype"),
        attr(props, "transition-style"),
    ) {
        (None, _, None | Some("none")) => Transition::Cut,
        (Some("pushWipe"), Some("fromRight"), _) => Transition::PushLeft,
        (Some("fade"), None | Some("crossfade"), _) => Transition::Fade,
        _ => return None,
    };
    if attr(props, "direction").is_some_and(|s| s != "forward") {
        return None;
    }
    let advance_ms = match attr(props, "transition-type").unwrap_or("manual") {
        "manual" => None,
        "automatic" | "semi-automatic" => {
            let duration = attr(props, "duration")?
                .strip_prefix("PT")?
                .strip_suffix('S')?
                .parse::<f64>()
                .ok()?;
            if !duration.is_finite() || !(0.1..=86400.0).contains(&duration) {
                return None;
            }
            Some((duration * 1000.0).round() as u32)
        }
        _ => return None,
    };
    let duration_ms = match attr(props, "transition-speed").unwrap_or("medium") {
        "slow" => 1000,
        "medium" => 500,
        "fast" => 250,
        _ => return None,
    };
    Some(SlideTiming {
        advance_ms,
        transition,
        duration_ms,
    })
}
fn timing_style(name: &str, timing: SlideTiming) -> Result<String, String> {
    let speed = match timing.duration_ms {
        250 => "fast",
        500 => "medium",
        1000 => "slow",
        _ => {
            return Err(
                "ODP supports fast (250), medium (500), or slow (1000) transition duration".into(),
            )
        }
    };
    let advance = timing
        .advance_ms
        .map(|ms| {
            format!(
                " presentation:transition-type=\"automatic\" presentation:duration=\"PT{:.3}S\"",
                ms as f64 / 1000.0
            )
        })
        .unwrap_or_else(|| " presentation:transition-type=\"manual\"".into());
    let transition = match timing.transition {
        Transition::Cut => " presentation:transition-style=\"none\"",
        Transition::PushLeft => " smil:type=\"pushWipe\" smil:subtype=\"fromRight\"",
        Transition::Fade => " smil:type=\"fade\" smil:subtype=\"crossfade\"",
    };
    Ok(format!("<style:style xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:presentation=\"urn:oasis:names:tc:opendocument:xmlns:presentation:1.0\" xmlns:smil=\"urn:oasis:names:tc:opendocument:xmlns:smil-compatible:1.0\" style:name=\"{name}\" style:family=\"drawing-page\"><style:drawing-page-properties presentation:transition-speed=\"{speed}\"{advance}{transition}/></style:style>"))
}
fn timing_name(xml: &str, slide_id: u64) -> Result<String, String> {
    (0..10000)
        .map(|n| format!("suite42_timing_{slide_id}_{n}"))
        .find(|name| !xml.contains(&format!("style:name=\"{name}\"")))
        .ok_or_else(|| "ODP timing style identity exhausted".into())
}
fn rotation_transform(o: &SlideObject) -> String {
    let angle = o.rotation_degrees.rem_euclid(360.0).to_radians();
    let (sin, cos) = angle.sin_cos();
    let tx = o.x + o.width / 2.0 - (cos * o.width - sin * o.height) / 2.0;
    let ty = o.y + o.height / 2.0 - (sin * o.width + cos * o.height) / 2.0;
    format!("rotate ({angle:.12}) translate ({} {})", pt(tx), pt(ty))
}
fn geometry_attrs(o: &SlideObject) -> String {
    if o.rotation_degrees.rem_euclid(360.0) == 0.0 {
        format!(
            "svg:x=\"{}\" svg:y=\"{}\" svg:width=\"{}\" svg:height=\"{}\"",
            pt(o.x),
            pt(o.y),
            pt(o.width),
            pt(o.height)
        )
    } else {
        format!(
            "draw:transform=\"{}\" svg:width=\"{}\" svg:height=\"{}\"",
            rotation_transform(o),
            pt(o.width),
            pt(o.height)
        )
    }
}
fn parse_rotation_transform(raw: &str) -> Option<(f64, f64, f64)> {
    let rest = raw
        .trim()
        .strip_prefix("rotate")?
        .trim_start()
        .strip_prefix('(')?;
    let (angle, rest) = rest.split_once(')')?;
    let angle: f64 = angle.trim().parse().ok()?;
    let rest = rest
        .trim_start()
        .strip_prefix("translate")?
        .trim_start()
        .strip_prefix('(')?;
    let (values, tail) = rest.split_once(')')?;
    if !tail.trim().is_empty() {
        return None;
    }
    let values = values
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>();
    if values.len() != 2 || !angle.is_finite() {
        return None;
    }
    Some((
        angle,
        number(Some(values[0])).ok()?,
        number(Some(values[1])).ok()?,
    ))
}
fn text(s: &str) -> String {
    s.split('\n')
        .map(|s| format!("<text:p>{}</text:p>", esc(s)))
        .collect()
}
fn styled_text(s: &str, name: &str, style: &TextStyle, styles: &mut String) -> String {
    let align = match style.align {
        TextAlign::Left => "left",
        TextAlign::Center => "center",
        TextAlign::Right => "right",
    };
    let weight = if style.bold { "bold" } else { "normal" };
    let italic = if style.italic { "italic" } else { "normal" };
    let underline = if style.underline { "solid" } else { "none" };
    styles.push_str(&format!("<style:style xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" style:name=\"{name}_text\" style:family=\"text\"><style:text-properties fo:font-family=\"{}\" fo:font-size=\"{}\" fo:font-weight=\"{weight}\" fo:font-style=\"{italic}\" fo:color=\"#{:02x}{:02x}{:02x}\" style:text-underline-style=\"{underline}\"/></style:style><style:style xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" style:name=\"{name}_para\" style:family=\"paragraph\"><style:paragraph-properties fo:text-align=\"{align}\"/></style:style>",esc(&style.family),pt(style.points),style.color[0],style.color[1],style.color[2]));
    s.split('\n').map(|p|format!("<text:p text:style-name=\"{name}_para\"><text:span text:style-name=\"{name}_text\">{}</text:span></text:p>",esc(p))).collect()
}
fn rich_text(paragraphs: &[RichParagraph], name: &str, styles: &mut String) -> String {
    let mut output = String::new();
    for (pi, p) in paragraphs.iter().enumerate() {
        let para_name = format!("{name}_p{pi}");
        let mut para_style = TextStyle::default();
        para_style.align = p.align;
        styled_text("", &para_name, &para_style, styles);
        let mut spans = String::new();
        for (ri, r) in p.runs.iter().enumerate() {
            let run_name = format!("{name}_p{pi}r{ri}");
            styled_text("", &run_name, &r.style, styles);
            spans.push_str(&format!(
                "<text:span text:style-name=\"{run_name}_text\">{}</text:span>",
                esc(&r.text)
            ));
        }
        let para = format!("<text:p text:style-name=\"{para_name}_para\">{spans}</text:p>");
        if p.bullet {
            let list_name = format!("{name}_list{pi}");
            styles.push_str(&format!("<text:list-style xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" style:name=\"{list_name}\"><text:list-level-style-bullet text:level=\"1\" text:bullet-char=\"•\"/></text:list-style>"));
            output.push_str(&format!("<text:list text:style-name=\"{list_name}\"><text:list-item>{para}</text:list-item></text:list>"));
        } else {
            output.push_str(&para)
        }
    }
    output
}
fn zip_read(z: &mut ZipArchive<Cursor<&[u8]>>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = z.by_name(name).map_err(|e| e.to_string())?;
    if f.size() > 32 * 1024 * 1024 {
        return Err("ODP part exceeds limit".into());
    }
    let mut out = Vec::new();
    f.read_to_end(&mut out).map_err(|e| e.to_string())?;
    Ok(out)
}
fn number(s: Option<&str>) -> Result<f64, String> {
    let s = s.ok_or("Missing geometry")?;
    for (unit, factor) in [
        ("cm", 72.0 / 2.54),
        ("mm", 72.0 / 25.4),
        ("in", 72.0),
        ("pt", 1.0),
        ("px", 0.75),
    ] {
        if let Some(n) = s.strip_suffix(unit) {
            let n: f64 = n.parse().map_err(|_| "Invalid geometry")?;
            return Ok(n * factor);
        }
    }
    Err("Unsupported geometry unit".into())
}
fn attr<'a>(n: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.attributes().find(|a| a.name() == name).map(|a| a.value())
}
fn paragraphs(n: roxmltree::Node<'_, '_>) -> String {
    n.descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .map(|p| {
            p.descendants()
                .filter_map(|v| v.text().filter(|_| v.is_text()))
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn color(value: &str) -> Option<[u8; 3]> {
    let s = value.strip_prefix('#')?;
    if s.len() != 6 {
        return None;
    }
    Some([
        u8::from_str_radix(&s[0..2], 16).ok()?,
        u8::from_str_radix(&s[2..4], 16).ok()?,
        u8::from_str_radix(&s[4..6], 16).ok()?,
    ])
}
fn rich_paragraphs(
    node: roxmltree::Node<'_, '_>,
    tree: &roxmltree::Document<'_>,
    base: &TextStyle,
) -> Vec<RichParagraph> {
    let mut paragraphs = Vec::new();
    for p in node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
    {
        let mut align = base.align;
        if let Some(name) = attr(p, "style-name") {
            if let Some(style) = tree.descendants().find(|n| {
                n.is_element() && n.tag_name().name() == "style" && attr(*n, "name") == Some(name)
            }) {
                if let Some(props) = style
                    .children()
                    .find(|n| n.is_element() && n.tag_name().name() == "paragraph-properties")
                {
                    align = match attr(props, "text-align") {
                        Some("center") => TextAlign::Center,
                        Some("right") => TextAlign::Right,
                        _ => TextAlign::Left,
                    };
                }
            }
        }
        let bullet = p
            .ancestors()
            .any(|n| n.is_element() && n.tag_name().name() == "list-item");
        let mut runs = Vec::new();
        for child in p.children() {
            if child.is_text() {
                runs.push(RichRun {
                    text: child.text().unwrap_or("").into(),
                    style: base.clone(),
                });
            } else if child.is_element() && child.tag_name().name() == "span" {
                let mut style = base.clone();
                if let Some(name) = attr(child, "style-name") {
                    if let Some(s) = tree.descendants().find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "style"
                            && attr(*n, "name") == Some(name)
                    }) {
                        if let Some(props) = s
                            .children()
                            .find(|n| n.is_element() && n.tag_name().name() == "text-properties")
                        {
                            if let Some(f) = attr(props, "font-family") {
                                if !f.is_empty() && f.len() <= 256 {
                                    style.family = f.trim_matches('\'').trim_matches('"').into();
                                }
                            }
                            if let Ok(v) = number(attr(props, "font-size")) {
                                if (1.0..=512.0).contains(&v) {
                                    style.points = v;
                                }
                            }
                            if let Some(v) = attr(props, "font-weight") {
                                style.bold = v == "bold";
                            }
                            if let Some(v) = attr(props, "font-style") {
                                style.italic = v == "italic";
                            }
                            if let Some(v) = attr(props, "text-underline-style") {
                                style.underline = v != "none";
                            }
                            if let Some(c) = attr(props, "color").and_then(color) {
                                style.color = c;
                            }
                        }
                    }
                }
                style.align = align;
                runs.push(RichRun {
                    text: child
                        .descendants()
                        .filter(|n| n.is_text())
                        .filter_map(|n| n.text())
                        .collect::<String>(),
                    style,
                });
            } else if child.is_element() {
                return Vec::new();
            }
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
fn rich_edit_safe(node: roxmltree::Node<'_, '_>, tree: &roxmltree::Document<'_>) -> bool {
    let paras = node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .collect::<Vec<_>>();
    if paras.is_empty() {
        return false;
    }
    for p in paras {
        for n in p.descendants().filter(|n| n.is_element()) {
            if !matches!(n.tag_name().name(), "p" | "span") {
                return false;
            }
            if let Some(name) = attr(n, "style-name") {
                let Some(style) = tree.descendants().find(|s| {
                    s.is_element()
                        && s.tag_name().name() == "style"
                        && attr(*s, "name") == Some(name)
                }) else {
                    return false;
                };
                if style
                    .attributes()
                    .any(|a| !matches!(a.name(), "name" | "family"))
                {
                    return false;
                }
                for props in style.children().filter(|c| c.is_element()) {
                    if !matches!(
                        props.tag_name().name(),
                        "text-properties" | "paragraph-properties"
                    ) || props.attributes().any(|a| {
                        !matches!(
                            a.name(),
                            "font-family"
                                | "font-size"
                                | "font-weight"
                                | "font-style"
                                | "color"
                                | "text-underline-style"
                                | "text-align"
                        )
                    }) {
                        return false;
                    }
                }
            }
        }
    }
    true
}
/// Reads a subset of presentation frames, shapes, pictures and tables.
/// This is not yet a lossless adapter for arbitrary external ODP files.
pub fn import(bytes: &[u8]) -> Result<Presentation, String> {
    import_with_source(bytes).map(|v| v.0)
}
pub fn import_with_source(bytes: &[u8]) -> Result<(Presentation, Source), String> {
    if bytes.len() as u64 > MAX_MEDIA_FILE {
        return Err("ODP exceeds file limit".into());
    }
    let mut z = ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let content = zip_read(&mut z, "content.xml")?;
    let xml = std::str::from_utf8(&content).map_err(|e| e.to_string())?;
    let tree = roxmltree::Document::parse(xml).map_err(|e| e.to_string())?;
    let pages = tree
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "page"
                && n.tag_name().namespace()
                    == Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0")
        })
        .collect::<Vec<_>>();
    if pages.is_empty() || pages.len() > MAX_SLIDES {
        return Err("Invalid slide count".into());
    }
    let mut deck = Presentation::new();
    let mut source_pages = Vec::new();
    for (i, page) in pages.iter().enumerate() {
        if i > 0 {
            deck.add()?;
        }
        let parsed_timing = parse_timing(*page, &tree);
        let timing = parsed_timing.unwrap_or_default();
        deck.set_timing(timing)?;
        let style_safe = attr(*page, "style-name").is_none_or(|name| {
            tree.descendants()
                .find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "style"
                        && attr(*n, "name") == Some(name)
                        && attr(*n, "family") == Some("drawing-page")
                })
                .is_some_and(|style| {
                    style.children().filter(|n| n.is_element()).all(|p| {
                        p.tag_name().name() == "drawing-page-properties"
                            && p.attributes().all(|a| {
                                matches!(
                                    a.name(),
                                    "transition-type"
                                        | "duration"
                                        | "transition-speed"
                                        | "transition-style"
                                        | "type"
                                        | "subtype"
                                        | "direction"
                                )
                            })
                    })
                })
        });
        let mut source_objects = Vec::new();
        let mut group_ids = HashMap::new();
        for node in page.descendants().filter(|n| {
            n.is_element()
                && n.parent()
                    .is_some_and(|p| p == *page || p.tag_name().name() == "g")
        }) {
            // An unrecognized transform cannot be represented by this model yet.
            // Keep the source XML intact instead of displaying incorrect geometry.
            if attr(node, "transform").is_some_and(|s| parse_rotation_transform(s).is_none()) {
                continue;
            }
            if node
                .ancestors()
                .filter(|n| n.is_element() && n.tag_name().name() == "g")
                .any(|g| attr(g, "transform").is_some())
            {
                continue;
            }
            let ns = node.tag_name().namespace().unwrap_or("");
            if ns != "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0" {
                continue;
            }
            let name = node.tag_name().name();
            let (kind, media, cells) = match name {
                "rect" => (ObjectKind::Rectangle, Vec::new(), Vec::new()),
                "ellipse" => (ObjectKind::Ellipse, Vec::new(), Vec::new()),
                "line" => (ObjectKind::Line, Vec::new(), Vec::new()),
                "frame" => {
                    if let Some(image) = node
                        .descendants()
                        .find(|n| n.is_element() && n.tag_name().name() == "image")
                    {
                        let href = attr(image, "href")
                            .ok_or("Image reference missing")?
                            .trim_start_matches("./");
                        if href.starts_with('/')
                            || href.split('/').any(|s| s == "..")
                            || href.contains(':')
                        {
                            return Err("External image reference unsupported".into());
                        }
                        (ObjectKind::Picture, zip_read(&mut z, href)?, Vec::new())
                    } else if let Some(table) = node
                        .descendants()
                        .find(|n| n.is_element() && n.tag_name().name() == "table")
                    {
                        let rows = table
                            .children()
                            .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
                            .map(|r| {
                                r.children()
                                    .filter(|n| {
                                        n.is_element() && n.tag_name().name() == "table-cell"
                                    })
                                    .map(paragraphs)
                                    .collect::<Vec<_>>()
                            })
                            .collect::<Vec<_>>();
                        if rows.is_empty()
                            || rows
                                .iter()
                                .any(|r| r.len() != rows[0].len() || r.is_empty())
                        {
                            return Err("Unsupported table structure".into());
                        }
                        (ObjectKind::Table, Vec::new(), rows)
                    } else if node
                        .descendants()
                        .any(|n| n.is_element() && n.tag_name().name() == "text-box")
                    {
                        (ObjectKind::Text, Vec::new(), Vec::new())
                    } else {
                        continue;
                    }
                }
                "notes" => continue,
                _ => continue,
            };
            let rotation = attr(node, "transform").and_then(parse_rotation_transform);
            if rotation.is_some()
                && ["x", "y"].iter().any(|key| {
                    attr(node, key).is_some_and(|v| number(Some(v)).map_or(true, |n| n != 0.0))
                })
            {
                continue;
            }
            let geometry = if matches!(kind, ObjectKind::Line) {
                match (
                    number(attr(node, "x1")),
                    number(attr(node, "y1")),
                    number(attr(node, "x2")),
                    number(attr(node, "y2")),
                ) {
                    (Ok(x), Ok(y), Ok(x2), Ok(y2)) => Some((x, y, x2 - x, y2 - y)),
                    _ => None,
                }
            } else {
                match (
                    if rotation.is_some() {
                        Ok(0.0)
                    } else {
                        number(attr(node, "x"))
                    },
                    if rotation.is_some() {
                        Ok(0.0)
                    } else {
                        number(attr(node, "y"))
                    },
                    number(attr(node, "width")),
                    number(attr(node, "height")),
                ) {
                    (Ok(x), Ok(y), Ok(w), Ok(h)) => Some((x, y, w, h)),
                    _ => None,
                }
            };
            let Some((x, y, width, height)) = geometry else {
                continue;
            };
            if rotation.is_some() && matches!(kind, ObjectKind::Line) && (x != 0.0 || y != 0.0) {
                continue;
            }
            if width <= 0.0 || height <= 0.0 {
                continue;
            }
            let content = if matches!(kind, ObjectKind::Table | ObjectKind::Picture) {
                String::new()
            } else {
                paragraphs(node)
            };
            let id = deck.insert_content(kind, content, media, cells)?;
            let mut object = deck.slides()[i]
                .objects
                .iter()
                .find(|o| o.id == id)
                .unwrap()
                .clone();
            if let Some(group) = node
                .ancestors()
                .filter(|n| n.is_element() && n.tag_name().name() == "g")
                .last()
            {
                let id = *group_ids.entry(group.range().start).or_insert(object.id);
                object.group_id = Some(id);
            }
            object.x = x;
            object.y = y;
            object.width = width;
            object.height = height;
            if let Some((angle, tx, ty)) = rotation {
                let (sin, cos) = angle.sin_cos();
                object.x = tx + (cos * width - sin * height) / 2.0 - width / 2.0;
                object.y = ty + (sin * width + cos * height) / 2.0 - height / 2.0;
                object.rotation_degrees = angle.to_degrees().rem_euclid(360.0);
            }
            if let Some(span) = node
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "span")
            {
                if let Some(name) = attr(span, "style-name") {
                    if let Some(style) = tree.descendants().find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "style"
                            && attr(*n, "name") == Some(name)
                    }) {
                        if let Some(props) = style
                            .children()
                            .find(|n| n.is_element() && n.tag_name().name() == "text-properties")
                        {
                            let t = &mut object.text_style;
                            if let Some(family) = attr(props, "font-family") {
                                if !family.is_empty() && family.len() <= 256 {
                                    t.family =
                                        family.trim_matches('\'').trim_matches('"').to_owned();
                                }
                            }
                            if let Ok(size) = number(attr(props, "font-size")) {
                                if (1.0..=512.0).contains(&size) {
                                    t.points = size;
                                }
                            }
                            t.bold = attr(props, "font-weight") == Some("bold");
                            t.italic = attr(props, "font-style") == Some("italic");
                            t.underline =
                                attr(props, "text-underline-style").is_some_and(|v| v != "none");
                            if let Some(c) = attr(props, "color").and_then(color) {
                                t.color = c;
                            }
                        }
                    }
                }
            }
            if let Some(paragraph) = node
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "p")
            {
                if let Some(name) = attr(paragraph, "style-name") {
                    if let Some(style) = tree.descendants().find(|n| {
                        n.is_element()
                            && n.tag_name().name() == "style"
                            && attr(*n, "name") == Some(name)
                    }) {
                        if let Some(props) = style.children().find(|n| {
                            n.is_element() && n.tag_name().name() == "paragraph-properties"
                        }) {
                            object.text_style.align = match attr(props, "text-align") {
                                Some("center") => TextAlign::Center,
                                Some("right") => TextAlign::Right,
                                _ => TextAlign::Left,
                            };
                        }
                    }
                }
            }
            if !matches!(object.kind, ObjectKind::Picture | ObjectKind::Table) {
                object.paragraphs = rich_paragraphs(node, &tree, &object.text_style);
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
            }
            if let Some(style_name) = attr(node, "style-name") {
                if let Some(style) = tree.descendants().find(|n| {
                    n.is_element()
                        && n.tag_name().name() == "style"
                        && attr(*n, "name") == Some(style_name)
                }) {
                    if let Some(properties) = style
                        .children()
                        .find(|n| n.is_element() && n.tag_name().name() == "graphic-properties")
                    {
                        if let Some(value) = attr(properties, "fill") {
                            object.fill_visible = value != "none";
                        }
                        if let Some(value) = attr(properties, "stroke") {
                            object.stroke_visible = value != "none";
                        }
                        if let Some(value) = attr(properties, "fill-color").and_then(color) {
                            object.fill = value;
                        }
                        if let Some(value) = attr(properties, "stroke-color").and_then(color) {
                            object.stroke = value;
                        }
                    }
                }
            }
            deck.update_object(object.clone())?;
            let text_editable = node.descendants().any(|n| {
                n.is_element() && n.tag_name().name() == "p" && n.descendants().any(|n| n.is_text())
            });
            let writable_cells = local_table(node)
                .map(|table| {
                    table
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
                        .map(|r| {
                            r.children()
                                .filter(|n| n.is_element() && n.tag_name().name() == "table-cell")
                                .map(|c| {
                                    c.descendants().any(|n| {
                                        n.is_element()
                                            && n.tag_name().name() == "p"
                                            && n.descendants().any(|n| n.is_text())
                                    })
                                })
                                .collect()
                        })
                        .collect()
                })
                .unwrap_or_default();
            source_objects.push(ObjectSource {
                object_id: id,
                start: node.range().start,
                original: object.clone(),
                writable_cells,
                text_editable,
                rich_safe: !matches!(object.kind, ObjectKind::Picture | ObjectKind::Table)
                    && rich_edit_safe(node, &tree),
            });
        }
        source_pages.push(PageSource {
            slide_id: deck.slides()[i].id,
            objects: source_objects,
            timing,
            timing_editable: parsed_timing.is_some() && style_safe,
        });
    }
    deck.select(0)?;
    let state = deck.history.snapshot();
    deck.history = SnapshotHistory::new(state, HISTORY_LIMIT);
    let mut sizes = Vec::new();
    for data in [Some(content.clone()), zip_read(&mut z, "styles.xml").ok()]
        .into_iter()
        .flatten()
    {
        if let Ok(text) = std::str::from_utf8(&data) {
            if let Ok(doc) = roxmltree::Document::parse(text) {
                for n in doc
                    .descendants()
                    .filter(|n| n.is_element() && n.tag_name().name() == "page-layout-properties")
                {
                    if let (Ok(w), Ok(h)) = (
                        number(attr(n, "page-width")),
                        number(attr(n, "page-height")),
                    ) {
                        if w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite() {
                            sizes.push((w, h));
                        }
                    }
                }
            }
        }
    }
    let size_points = sizes.first().copied().filter(|first| {
        sizes
            .iter()
            .all(|size| (size.0 - first.0).abs() < 0.01 && (size.1 - first.1).abs() < 0.01)
    });
    Ok((
        deck,
        Source {
            bytes: bytes.to_vec(),
            pages: source_pages,
            size_points,
        },
    ))
}
fn local_table<'a>(node: roxmltree::Node<'a, 'a>) -> Option<roxmltree::Node<'a, 'a>> {
    node.descendants()
        .find(|n| n.is_element() && n.tag_name().name() == "table")
}
/// Produces an ODF package with standard content, styles, manifest and media.
pub fn export(deck: &Presentation) -> Result<Vec<u8>, String> {
    let mut styles = String::new();
    let mut pages = String::new();
    let mut media: Vec<(String, Vec<u8>, &str)> = Vec::new();
    for (si, slide) in deck.slides().iter().enumerate() {
        let timing_name = format!("suite42_page_{si}");
        styles.push_str(&timing_style(&timing_name, slide.timing)?);
        pages.push_str(&format!(
            "<draw:page draw:name=\"Slide {}\" draw:style-name=\"{}\" draw:master-page-name=\"Default\">",
            si + 1, timing_name
        ));
        let mut group_shapes: HashMap<u64, String> = HashMap::new();
        let mut group_order = Vec::new();
        for (oi, o) in slide.objects.iter().enumerate() {
            if let Some(group) = o.group_id {
                if !group_shapes.contains_key(&group) {
                    pages.push_str(&format!("<!--suite42-odp-group-{si}-{group}-->"));
                    group_shapes.insert(group, String::new());
                    group_order.push(group);
                }
            }
            let target = if let Some(group) = o.group_id {
                group_shapes.get_mut(&group).unwrap()
            } else {
                &mut pages
            };
            let name = format!("g{si}_{oi}");
            styles.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"{}\" draw:fill-color=\"#{:02x}{:02x}{:02x}\" draw:stroke=\"{}\" svg:stroke-color=\"#{:02x}{:02x}{:02x}\"/></style:style>",if o.fill_visible{"solid"}else{"none"},o.fill[0],o.fill[1],o.fill[2],if o.stroke_visible{"solid"}else{"none"},o.stroke[0],o.stroke[1],o.stroke[2]));
            let object_text = if o.paragraphs.is_empty() {
                styled_text(&o.text, &name, &o.text_style, &mut styles)
            } else {
                rich_text(&o.paragraphs, &name, &mut styles)
            };
            let geo = format!("draw:style-name=\"{name}\" {}", geometry_attrs(o));
            match o.kind {
                ObjectKind::Rectangle | ObjectKind::Ellipse => {
                    let tag = if o.kind == ObjectKind::Rectangle {
                        "rect"
                    } else {
                        "ellipse"
                    };
                    target.push_str(&format!("<draw:{tag} {geo}>{object_text}</draw:{tag}>"));
                }
                ObjectKind::Text => target.push_str(&format!(
                    "<draw:frame {geo}><draw:text-box>{}</draw:text-box></draw:frame>",
                    object_text
                )),
                ObjectKind::Picture => {
                    let (ext, mime) = if o.media.starts_with(b"\x89PNG\r\n\x1a\n") {
                        ("png", "image/png")
                    } else if o.media.starts_with(b"\xff\xd8\xff") {
                        ("jpg", "image/jpeg")
                    } else {
                        return Err("Unsupported image encoding".into());
                    };
                    let path = format!("Pictures/{si}_{oi}.{ext}");
                    target.push_str(&format!("<draw:frame {geo}><draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>"));
                    media.push((path, o.media.clone(), mime));
                }
                ObjectKind::Table => {
                    target.push_str(&format!(
                        "<draw:frame {geo}><table:table table:name=\"Table{si}_{oi}\">"
                    ));
                    for row in &o.cells {
                        target.push_str("<table:table-row>");
                        for cell in row {
                            target.push_str(&format!("<table:table-cell office:value-type=\"string\">{}</table:table-cell>",text(cell)));
                        }
                        target.push_str("</table:table-row>");
                    }
                    target.push_str("</table:table></draw:frame>");
                }
                ObjectKind::Line => {
                    let transform = if o.rotation_degrees.rem_euclid(360.0) != 0.0 {
                        format!(" draw:transform=\"{}\"", rotation_transform(o))
                    } else {
                        String::new()
                    };
                    let (x, y) = if transform.is_empty() {
                        (o.x, o.y)
                    } else {
                        (0.0, 0.0)
                    };
                    target.push_str(&format!("<draw:line draw:style-name=\"{name}\"{transform} svg:x1=\"{}\" svg:y1=\"{}\" svg:x2=\"{}\" svg:y2=\"{}\"/>",pt(x),pt(y),pt(x+o.width),pt(y+o.height)));
                }
            }
        }
        for group in group_order {
            let children = group_shapes.remove(&group).unwrap();
            pages = pages.replace(
                &format!("<!--suite42-odp-group-{si}-{group}-->"),
                &format!("<draw:g draw:name=\"Group {group}\">{children}</draw:g>"),
            );
        }
        // Legacy title/body are represented as ordinary editable text frames.
        for (j, value) in [&slide.title, &slide.body].iter().enumerate() {
            if !value.is_empty() {
                pages.push_str(&format!("<draw:frame svg:x=\"35pt\" svg:y=\"{}pt\" svg:width=\"650pt\" svg:height=\"50pt\"><draw:text-box>{}</draw:text-box></draw:frame>",30+j*55,text(value)));
            }
        }
        pages.push_str("</draw:page>");
    }
    let ns="xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
    let content=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {ns} office:version=\"1.2\"><office:automatic-styles>{styles}</office:automatic-styles><office:body><office:presentation>{pages}</office:presentation></office:body></office:document-content>");
    let styles_doc=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {ns} xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" office:version=\"1.2\"><office:automatic-styles><style:page-layout style:name=\"pm1\"><style:page-layout-properties fo:page-width=\"10in\" fo:page-height=\"5.625in\" style:print-orientation=\"landscape\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Default\" style:page-layout-name=\"pm1\"/></office:master-styles></office:document-styles>");
    let mut manifest=String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"application/vnd.oasis.opendocument.presentation\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/>");
    for (path, _, mime) in &media {
        manifest.push_str(&format!(
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            esc(path),
            mime
        ));
    }
    manifest.push_str("</manifest:manifest>");
    let mut z = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    z.start_file("mimetype", stored)
        .map_err(|e| e.to_string())?;
    z.write_all(b"application/vnd.oasis.opendocument.presentation")
        .map_err(|e| e.to_string())?;
    for (path, data) in [
        ("content.xml", content.as_bytes()),
        ("styles.xml", styles_doc.as_bytes()),
        ("META-INF/manifest.xml", manifest.as_bytes()),
    ] {
        z.start_file(path, deflated).map_err(|e| e.to_string())?;
        z.write_all(data).map_err(|e| e.to_string())?;
    }
    for (path, data, _) in media {
        z.start_file(path, deflated).map_err(|e| e.to_string())?;
        z.write_all(&data).map_err(|e| e.to_string())?;
    }
    Ok(z.finish().map_err(|e| e.to_string())?.into_inner())
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
            duration_ms: 1000,
        };
        deck.set_timing(timing).unwrap();
        let (mut opened, source) = import_with_source(&export(&deck).unwrap()).unwrap();
        assert_eq!(opened.slides()[0].timing, timing);
        let changed = SlideTiming {
            advance_ms: None,
            transition: Transition::Fade,
            duration_ms: 250,
        };
        opened.set_timing(changed).unwrap();
        let (again, _) = import_with_source(&export_overlay(&opened, &source).unwrap()).unwrap();
        assert_eq!(again.slides()[0].timing, changed);
    }
    #[test]
    fn rotation_roundtrips_new_and_overlay() {
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
        object.rotation_degrees = 35.0;
        object.x = 142.0;
        object.y = 96.0;
        deck.update_object(object.clone()).unwrap();
        let bytes = export(&deck).unwrap();
        let (mut reopened, source) = import_with_source(&bytes).unwrap();
        let actual = &reopened.slides()[0].objects[0];
        assert!((actual.rotation_degrees - object.rotation_degrees).abs() < 0.001);
        assert!((actual.x - object.x).abs() < 0.001 && (actual.y - object.y).abs() < 0.001);
        let mut edited = actual.clone();
        edited.rotation_degrees = 80.0;
        edited.x += 30.0;
        reopened.update_object(edited.clone()).unwrap();
        let bytes = export_overlay(&reopened, &source).unwrap();
        let (reopened, _) = import_with_source(&bytes).unwrap();
        let actual = &reopened.slides()[0].objects[0];
        assert!((actual.rotation_degrees - edited.rotation_degrees).abs() < 0.001);
        assert!((actual.x - edited.x).abs() < 0.001 && (actual.y - edited.y).abs() < 0.001);
        let mut lines = Presentation::new();
        lines
            .insert_object(ObjectKind::Line, String::new())
            .unwrap();
        let (mut lines, source) = import_with_source(&export(&lines).unwrap()).unwrap();
        let mut line = lines.slides()[0].objects[0].clone();
        line.rotation_degrees = 60.0;
        lines.update_object(line.clone()).unwrap();
        let (mut reopened, source) =
            import_with_source(&export_overlay(&lines, &source).unwrap()).unwrap();
        let actual = &reopened.slides()[0].objects[0];
        assert!((actual.rotation_degrees - 60.0).abs() < 0.001);
        assert!((actual.x - line.x).abs() < 0.001 && (actual.y - line.y).abs() < 0.001);
        let mut edited_line = actual.clone();
        edited_line.rotation_degrees = 120.0;
        reopened.update_object(edited_line).unwrap();
        let (again, _) = import_with_source(&export_overlay(&reopened, &source).unwrap()).unwrap();
        assert!((again.slides()[0].objects[0].rotation_degrees - 120.0).abs() < 0.001);
        let mut initial = Presentation::new();
        initial
            .insert_object(ObjectKind::Rectangle, String::new())
            .unwrap();
        let (mut initial, source) = import_with_source(&export(&initial).unwrap()).unwrap();
        let mut edited = initial.slides()[0].objects[0].clone();
        edited.rotation_degrees = 50.0;
        initial.update_object(edited.clone()).unwrap();
        let (reopened, _) =
            import_with_source(&export_overlay(&initial, &source).unwrap()).unwrap();
        let actual = &reopened.slides()[0].objects[0];
        assert!((actual.rotation_degrees - edited.rotation_degrees).abs() < 0.001);
        assert!((actual.x - edited.x).abs() < 0.001 && (actual.y - edited.y).abs() < 0.001);
    }
    use super::*;
    #[test]
    fn overlay_keeps_opaque_elements_and_package_parts() {
        let mut original = Presentation::new();
        original
            .insert_object(ObjectKind::Rectangle, "Hello".into())
            .unwrap();
        let bytes = export(&original).unwrap();
        let mut input = ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut output = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..input.len() {
            let file = input.by_index(i).unwrap();
            let name = file.name().to_owned();
            if name == "content.xml" {
                let mut data = String::new();
                let mut file = file;
                file.read_to_string(&mut data).unwrap();
                data = data.replace(
                    "</draw:page>",
                    "<draw:custom-shape draw:name=\"opaque\"/></draw:page>",
                );
                output
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                output.write_all(data.as_bytes()).unwrap();
            } else {
                output.raw_copy_file(file).unwrap();
            }
        }
        output
            .start_file("meta.xml", SimpleFileOptions::default())
            .unwrap();
        output.write_all(b"<opaque>metadata</opaque>").unwrap();
        let bytes = output.finish().unwrap().into_inner();
        let (mut deck, source) = import_with_source(&bytes).unwrap();
        let mut shape = deck.slides()[0].objects[0].clone();
        shape.x += 25.0;
        shape.text = "Edited & safe".into();
        shape.text_style.family = "Georgia".into();
        shape.text_style.points = 22.0;
        shape.text_style.bold = true;
        shape.text_style.color = [10, 20, 30];
        shape.text_style.align = TextAlign::Right;
        deck.update_object(shape).unwrap();
        deck.insert_object(ObjectKind::Ellipse, "inserted".into())
            .unwrap();
        deck.duplicate().unwrap();
        let mut duplicate = deck.slides()[1].objects[0].clone();
        duplicate.text = "Duplicate".into();
        deck.update_object(duplicate).unwrap();
        deck.add().unwrap();
        deck.insert_table(2, 2).unwrap();
        deck.insert_picture(b"\x89PNG\r\n\x1a\nfixture".to_vec())
            .unwrap();
        let out = export_overlay(&deck, &source).unwrap();
        let (reopened, _) = import_with_source(&out).unwrap();
        assert_eq!(reopened.slides()[0].objects[0].text, "Edited & safe");
        let style = &reopened.slides()[0].objects[0].text_style;
        assert_eq!(style.family, "Georgia");
        assert_eq!(style.points, 22.0);
        assert!(style.bold);
        assert_eq!(style.color, [10, 20, 30]);
        assert_eq!(style.align, TextAlign::Right);
        assert_eq!(reopened.slides().len(), 3);
        assert_eq!(reopened.slides()[0].objects[1].text, "inserted");
        assert_eq!(reopened.slides()[1].objects[0].text, "Duplicate");
        assert_eq!(reopened.slides()[2].objects.len(), 2);
        assert_eq!(
            reopened.slides()[2].objects[1].media,
            b"\x89PNG\r\n\x1a\nfixture"
        );
        let mut archive = ZipArchive::new(Cursor::new(out.as_slice())).unwrap();
        assert_eq!(
            zip_read(&mut archive, "meta.xml").unwrap(),
            b"<opaque>metadata</opaque>"
        );
        assert!(
            String::from_utf8(zip_read(&mut archive, "content.xml").unwrap())
                .unwrap()
                .contains("draw:custom-shape")
        );
    }
    #[test]
    fn standard_package_preserves_common_objects() {
        let mut deck = Presentation::new();
        let shape = deck
            .insert_object(ObjectKind::Rectangle, "hello & world".into())
            .unwrap();
        let mut object = deck.slides()[0]
            .objects
            .iter()
            .find(|o| o.id == shape)
            .unwrap()
            .clone();
        object.x = 42.0;
        object.fill = [12, 34, 56];
        object.stroke = [78, 90, 123];
        deck.update_object(object).unwrap();
        let table = deck.insert_table(2, 2).unwrap();
        let mut object = deck.slides()[0]
            .objects
            .iter()
            .find(|o| o.id == table)
            .unwrap()
            .clone();
        object.cells[1][1] = "cell".into();
        deck.update_object(object).unwrap();
        deck.insert_picture(b"\x89PNG\r\n\x1a\nfixture".to_vec())
            .unwrap();
        deck.add().unwrap();
        deck.insert_object(ObjectKind::Ellipse, "second".into())
            .unwrap();
        let bytes = export(&deck).unwrap();
        let reopened = import(&bytes).unwrap();
        let objects = &reopened.slides()[0].objects;
        assert_eq!(objects.len(), 3);
        assert_eq!(objects[0].text, "hello & world");
        assert_eq!(objects[0].x, 42.0);
        assert_eq!(objects[0].fill, [12, 34, 56]);
        assert_eq!(objects[0].stroke, [78, 90, 123]);
        assert_eq!(objects[1].cells[1][1], "cell");
        assert_eq!(objects[2].media, b"\x89PNG\r\n\x1a\nfixture");
        let (mut external, source) = import_with_source(&bytes).unwrap();
        external.move_slide(1, 0).unwrap();
        let reordered = export_overlay(&external, &source).unwrap();
        let (again, _) = import_with_source(&reordered).unwrap();
        assert_eq!(again.slides()[0].objects[0].kind, ObjectKind::Ellipse);
        external.select(1).unwrap();
        external.delete().unwrap();
        let reduced = export_overlay(&external, &source).unwrap();
        let mut package = ZipArchive::new(Cursor::new(reduced.as_slice())).unwrap();
        assert!(package.by_name("Pictures/0_2.png").is_err());
        let manifest =
            String::from_utf8(zip_read(&mut package, "META-INF/manifest.xml").unwrap()).unwrap();
        assert!(!manifest.contains("Pictures/0_2.png"));
        let (again, _) = import_with_source(&reduced).unwrap();
        assert_eq!(again.slides().len(), 1);
    }
}

fn patch_attr(
    node: roxmltree::Node<'_, '_>,
    name: &str,
    value: String,
    xml: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let attribute = node
        .attributes()
        .find(|a| a.name() == name)
        .ok_or("Missing ODP geometry")?;
    let range = attribute.range();
    let raw = &xml[range.clone()];
    let equal = raw.find('=').ok_or("Invalid ODP attribute")?;
    patches.push((range, format!("{}=\"{}\"", &raw[..equal], value)));
    Ok(())
}
fn patch_text(
    node: roxmltree::Node<'_, '_>,
    value: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    if value.contains('\n') {
        return Err("Multiline ODP text edits are not supported yet".into());
    }
    let texts = node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
        .flat_map(|p| p.descendants().filter(|n| n.is_text()))
        .collect::<Vec<_>>();
    if texts.is_empty() {
        return Err("This ODP text structure cannot be edited yet".into());
    }
    for (i, n) in texts.iter().enumerate() {
        patches.push((
            n.range(),
            if i == 0 {
                esc(value).replace('\r', "&#13;")
            } else {
                String::new()
            },
        ));
    }
    Ok(())
}
fn set_style(
    node: roxmltree::Node<'_, '_>,
    name: &str,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    if attr(node, "style-name").is_some() {
        patch_attr(node, "style-name", name.into(), source, patches)
    } else {
        let start = node.range().start;
        let end = source[start..node.range().end]
            .find('>')
            .ok_or("Invalid ODP element")?
            + start;
        let at = if source.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
            end - 1
        } else {
            end
        };
        patches.push((at..at,format!(" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" text:style-name=\"{}\"",esc(name))));
        Ok(())
    }
}
fn patch_source_style(
    node: roxmltree::Node<'_, '_>,
    slide: u64,
    object: u64,
    old: &TextStyle,
    new: &TextStyle,
    text_override: Option<&str>,
    source: &str,
    styles: &mut String,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    let font_changed = old.family != new.family
        || old.points != new.points
        || old.bold != new.bold
        || old.italic != new.italic
        || old.underline != new.underline
        || old.color != new.color;
    let mut counter = 0usize;
    let mut first = true;
    for paragraph in node
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name() == "p")
    {
        if old.align != new.align {
            let base = format!("suite42_par_{slide}_{object}_{counter}");
            counter += 1;
            if source.contains(&base) {
                return Err("ODP style collision".into());
            }
            let mut defs = String::new();
            styled_text("", &base, new, &mut defs);
            if let Some(parent) = attr(paragraph, "style-name") {
                defs = defs.replace(
                    "style:family=\"paragraph\"",
                    &format!(
                        "style:family=\"paragraph\" style:parent-style-name=\"{}\"",
                        esc(parent)
                    ),
                );
            }
            styles.push_str(&defs);
            set_style(paragraph, &format!("{base}_para"), source, patches)?;
        }
        if font_changed {
            let spans = paragraph
                .descendants()
                .filter(|n| n.is_element() && n.tag_name().name() == "span")
                .collect::<Vec<_>>();
            for span in spans {
                let base = format!("suite42_run_{slide}_{object}_{counter}");
                counter += 1;
                if source.contains(&base) {
                    return Err("ODP style collision".into());
                }
                let mut defs = String::new();
                styled_text("", &base, new, &mut defs);
                if let Some(parent) = attr(span, "style-name") {
                    defs = defs.replace(
                        "style:family=\"text\"",
                        &format!(
                            "style:family=\"text\" style:parent-style-name=\"{}\"",
                            esc(parent)
                        ),
                    );
                }
                styles.push_str(&defs);
                set_style(span, &format!("{base}_text"), source, patches)?;
            }
            for text in paragraph.descendants().filter(|n| n.is_text()) {
                let replacement = if let Some(value) = text_override {
                    if value.contains('\n') {
                        return Err("Multiline ODP text edits are not supported yet".into());
                    }
                    let v = if first {
                        esc(value).replace('\r', "&#13;")
                    } else {
                        String::new()
                    };
                    first = false;
                    v
                } else {
                    source[text.range()].to_owned()
                };
                if text
                    .ancestors()
                    .any(|a| a.is_element() && a.tag_name().name() == "span")
                {
                    if text_override.is_some() {
                        patches.push((text.range(), replacement));
                    }
                } else {
                    let base = format!("suite42_direct_{slide}_{object}_{counter}");
                    counter += 1;
                    if source.contains(&base) {
                        return Err("ODP style collision".into());
                    }
                    let mut defs = String::new();
                    styled_text("", &base, new, &mut defs);
                    styles.push_str(&defs);
                    patches.push((text.range(),format!("<text:span xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" text:style-name=\"{base}_text\">{replacement}</text:span>")));
                }
            }
        }
    }
    Ok(())
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
/// Preserve the original package and patch supported object geometry/text only.
pub fn export_overlay(deck: &Presentation, source: &Source) -> Result<Vec<u8>, String> {
    let mut input =
        ZipArchive::new(Cursor::new(source.bytes.as_slice())).map_err(|e| e.to_string())?;
    let bytes = zip_read(&mut input, "content.xml")?;
    let original = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let tree = roxmltree::Document::parse(original).map_err(|e| e.to_string())?;
    let source_nodes = tree
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "page"
                && n.tag_name().namespace()
                    == Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0")
        })
        .collect::<Vec<_>>();
    if source_nodes.len() != source.pages.len() {
        return Err("ODP slide list changed unexpectedly".into());
    }
    let mut rendered_pages = Vec::new();
    let mut extra_styles = String::new();
    let mut new_media = Vec::new();
    for slide in deck.slides() {
        let Some(index) = source
            .pages
            .iter()
            .position(|p| p.slide_id == slide.source_identity())
        else {
            let mut added = String::new();
            for object in &slide.objects {
                if original.contains(&format!("suite42_{}_{}", slide.id, object.id)) {
                    return Err("ODP generated style collision".into());
                }
                added.push_str(&appended_object(
                    slide.id,
                    object,
                    &mut extra_styles,
                    &mut new_media,
                )?);
            }
            if !slide.title.is_empty() || !slide.body.is_empty() {
                return Err(
                    "Slide title/body editing is not yet available for added ODP slides".into(),
                );
            }
            let master = source_nodes
                .first()
                .and_then(|n| attr(*n, "master-page-name"))
                .map(|v| format!(" draw:master-page-name=\"{}\"", esc(v)))
                .unwrap_or_default();
            let timing_name = timing_name(original, slide.id)?;
            extra_styles.push_str(&timing_style(&timing_name, slide.timing)?);
            rendered_pages.push(format!("<draw:page xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" draw:name=\"Suite42 {}\" draw:style-name=\"{timing_name}\"{master}>{added}</draw:page>",slide.id));
            continue;
        };
        let page = &source.pages[index];
        let page_node = source_nodes[index];
        let base = page_node.range().start;
        let mut patches = Vec::new();
        if slide.timing != page.timing {
            if !page.timing_editable {
                return Err("This ODP page style cannot be safely edited".into());
            }
            let timing_name = timing_name(original, slide.id)?;
            extra_styles.push_str(&timing_style(&timing_name, slide.timing)?);
            if attr(page_node, "style-name").is_some() {
                patch_attr(page_node, "style-name", timing_name, original, &mut patches)?;
            } else {
                let start = page_node.range().start;
                let end = original[start..].find('>').ok_or("Invalid ODP page")? + start;
                let at = if original.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
                    end - 1
                } else {
                    end
                };
                let ns = match page_node.lookup_namespace_uri(Some("draw")) {
                    Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0") => "",
                    None => " xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\"",
                    Some(_) => return Err("ODP draw namespace is incompatible".into()),
                };
                patches.push((at..at, format!("{ns} draw:style-name=\"{timing_name}\"")));
            }
        }
        if slide.id != page.slide_id && attr(page_node, "name").is_some() {
            patch_attr(
                page_node,
                "name",
                format!("Suite42 {}", slide.id),
                original,
                &mut patches,
            )?;
        }
        let mut added = String::new();
        for object in slide
            .objects
            .iter()
            .filter(|o| !page.objects.iter().any(|s| s.object_id == o.id))
        {
            if original.contains(&format!("suite42_{}_{}", slide.id, object.id)) {
                return Err("ODP generated style collision".into());
            }
            added.push_str(&appended_object(
                slide.id,
                object,
                &mut extra_styles,
                &mut new_media,
            )?);
        }
        for item in &page.objects {
            let node = tree
                .descendants()
                .find(|n| n.is_element() && n.range().start == item.start)
                .ok_or("ODP object missing")?;
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
                return Err("Saving this ODP object formatting is not supported yet".into());
            }
            if matches!(object.kind, ObjectKind::Line) {
                let transformed = attr(node, "transform").is_some();
                let wants_transform =
                    transformed || object.rotation_degrees.rem_euclid(360.0) != 0.0;
                let changed = object.x != item.original.x
                    || object.y != item.original.y
                    || object.width != item.original.width
                    || object.height != item.original.height
                    || object.rotation_degrees != item.original.rotation_degrees;
                if wants_transform && changed {
                    let value = rotation_transform(object);
                    if transformed {
                        patch_attr(node, "transform", value, original, &mut patches)?;
                    } else {
                        let start = node.range().start;
                        let end = original[start..].find('>').ok_or("Invalid ODP line")? + start;
                        let at = if original.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
                            end - 1
                        } else {
                            end
                        };
                        let namespace = match node.lookup_namespace_uri(Some("draw")) {
                            Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0") => "",
                            None => {
                                " xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\""
                            }
                            Some(_) => return Err("ODP draw namespace is incompatible".into()),
                        };
                        patches.push((at..at, format!("{namespace} draw:transform=\"{}\"", value)));
                    }
                    patch_attr(node, "x1", pt(0.0), original, &mut patches)?;
                    patch_attr(node, "y1", pt(0.0), original, &mut patches)?;
                    patch_attr(node, "x2", pt(object.width), original, &mut patches)?;
                    patch_attr(node, "y2", pt(object.height), original, &mut patches)?;
                } else if !wants_transform {
                    if object.x != item.original.x {
                        patch_attr(node, "x1", pt(object.x), original, &mut patches)?;
                    }
                    if object.y != item.original.y {
                        patch_attr(node, "y1", pt(object.y), original, &mut patches)?;
                    }
                    if object.x != item.original.x || object.width != item.original.width {
                        patch_attr(
                            node,
                            "x2",
                            pt(object.x + object.width),
                            original,
                            &mut patches,
                        )?;
                    }
                    if object.y != item.original.y || object.height != item.original.height {
                        patch_attr(
                            node,
                            "y2",
                            pt(object.y + object.height),
                            original,
                            &mut patches,
                        )?;
                    }
                }
            } else {
                let transformed = attr(node, "transform").is_some();
                let wants_transform =
                    transformed || object.rotation_degrees.rem_euclid(360.0) != 0.0;
                let geometry_changed = object.x != item.original.x
                    || object.y != item.original.y
                    || object.width != item.original.width
                    || object.height != item.original.height
                    || object.rotation_degrees != item.original.rotation_degrees;
                if wants_transform && geometry_changed {
                    let value = rotation_transform(object);
                    if transformed {
                        patch_attr(node, "transform", value, original, &mut patches)?;
                    } else {
                        let start = node.range().start;
                        let end = original[start..].find('>').ok_or("Invalid ODP shape")? + start;
                        let at = if original.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
                            end - 1
                        } else {
                            end
                        };
                        let namespace = match node.lookup_namespace_uri(Some("draw")) {
                            Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0") => "",
                            None => {
                                " xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\""
                            }
                            Some(_) => return Err("ODP draw namespace is incompatible".into()),
                        };
                        patches.push((at..at, format!("{namespace} draw:transform=\"{}\"", value)));
                        if attr(node, "x").is_some() {
                            patch_attr(node, "x", pt(0.0), original, &mut patches)?;
                        }
                        if attr(node, "y").is_some() {
                            patch_attr(node, "y", pt(0.0), original, &mut patches)?;
                        }
                    }
                } else if !wants_transform {
                    if object.x != item.original.x {
                        patch_attr(node, "x", pt(object.x), original, &mut patches)?;
                    }
                    if object.y != item.original.y {
                        patch_attr(node, "y", pt(object.y), original, &mut patches)?;
                    }
                }
                if object.width != item.original.width {
                    patch_attr(node, "width", pt(object.width), original, &mut patches)?;
                }
                if object.height != item.original.height {
                    patch_attr(node, "height", pt(object.height), original, &mut patches)?;
                }
            }
            let rich_changed = object.paragraphs != item.original.paragraphs;
            if rich_changed {
                if !item.rich_safe || object.paragraphs.is_empty() {
                    return Err(
                        "This ODP text structure cannot be safely edited as rich text".into(),
                    );
                }
                let container = node
                    .descendants()
                    .find(|n| n.is_element() && n.tag_name().name() == "text-box")
                    .unwrap_or(node);
                let blocks = container
                    .children()
                    .filter(|n| n.is_element() && matches!(n.tag_name().name(), "p" | "list"))
                    .collect::<Vec<_>>();
                if blocks.is_empty() {
                    return Err("ODP text body missing".into());
                }
                let name = format!("suite42_rich_{}_{}", slide.id, object.id);
                if original.contains(&name) {
                    return Err("ODP style collision".into());
                }
                let replacement = rich_text(&object.paragraphs, &name, &mut extra_styles);
                for (index, block) in blocks.iter().enumerate() {
                    patches.push((
                        block.range(),
                        if index == 0 {
                            replacement.clone()
                        } else {
                            String::new()
                        },
                    ));
                }
            }
            let font_changed = object.text_style.family != item.original.text_style.family
                || object.text_style.points != item.original.text_style.points
                || object.text_style.bold != item.original.text_style.bold
                || object.text_style.italic != item.original.text_style.italic
                || object.text_style.underline != item.original.text_style.underline
                || object.text_style.color != item.original.text_style.color;
            if !rich_changed && object.text != item.original.text && !font_changed {
                patch_text(node, &object.text, &mut patches)?;
            }
            if !rich_changed && object.text_style != item.original.text_style {
                patch_source_style(
                    node,
                    slide.id,
                    object.id,
                    &item.original.text_style,
                    &object.text_style,
                    if font_changed && object.text != item.original.text {
                        Some(&object.text)
                    } else {
                        None
                    },
                    original,
                    &mut extra_styles,
                    &mut patches,
                )?;
            }
            if object.cells != item.original.cells {
                let table = local_table(node).ok_or("ODP table missing")?;
                let rows = table
                    .children()
                    .filter(|n| n.is_element() && n.tag_name().name() == "table-row")
                    .collect::<Vec<_>>();
                if rows.len() != object.cells.len() || rows.len() != item.original.cells.len() {
                    return Err("Changing ODP table size is unsupported".into());
                }
                for (r, row) in rows.iter().enumerate() {
                    let cells = row
                        .children()
                        .filter(|n| n.is_element() && n.tag_name().name() == "table-cell")
                        .collect::<Vec<_>>();
                    if cells.len() != object.cells[r].len()
                        || cells.len() != item.original.cells[r].len()
                    {
                        return Err("Changing ODP table size is unsupported".into());
                    }
                    for (c, cell) in cells.iter().enumerate() {
                        if object.cells[r][c] != item.original.cells[r][c] {
                            patch_text(*cell, &object.cells[r][c], &mut patches)?;
                        }
                    }
                }
            }
        }
        patches.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
        let mut content = original[page_node.range()].to_owned();
        for (r, v) in patches {
            if r.start < base || r.end > page_node.range().end {
                return Err("Invalid ODP patch range".into());
            }
            content.replace_range(r.start - base..r.end - base, &v);
        }
        if !added.is_empty() {
            append_element_content(&mut content, &added)?;
        }
        rendered_pages.push(content);
    }
    let mut content = original.to_owned();
    let replacement = rendered_pages.concat();
    for (index, node) in source_nodes.iter().enumerate().rev() {
        content.replace_range(node.range(), if index == 0 { &replacement } else { "" });
    }
    if !extra_styles.is_empty() {
        let parsed = roxmltree::Document::parse(&content).map_err(|e| e.to_string())?;
        if let Some(styles) = parsed
            .descendants()
            .find(|n| n.is_element() && n.tag_name().name() == "automatic-styles")
        {
            let range = styles.range();
            drop(parsed);
            let mut fragment = content[range.clone()].to_owned();
            append_element_content(&mut fragment, &extra_styles)?;
            content.replace_range(range, &fragment);
        } else {
            let body = parsed
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "body")
                .ok_or("ODP body missing")?
                .range()
                .start;
            drop(parsed);
            content.insert_str(
                body,
                &format!("<office:automatic-styles>{extra_styles}</office:automatic-styles>"),
            );
        }
    }
    let (prune, mut manifest) = prune_removed_pages(&mut input, source, deck, original, &content)?;
    if !new_media.is_empty() {
        let mut value = match manifest.take() {
            Some(v) => String::from_utf8(v).map_err(|e| e.to_string())?,
            None => String::from_utf8(zip_read(&mut input, "META-INF/manifest.xml")?)
                .map_err(|e| e.to_string())?,
        };
        let additions=new_media.iter().map(|(path,_,mime)|format!("<manifest:file-entry xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",esc(path),mime)).collect::<String>();
        append_element_content(&mut value, &additions)?;
        manifest = Some(value.into_bytes());
    }
    let mut output = ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let file = input.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().to_owned();
        if prune.contains(&name) {
            continue;
        }
        if name == "content.xml" {
            output
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .map_err(|e| e.to_string())?;
            output
                .write_all(content.as_bytes())
                .map_err(|e| e.to_string())?;
        } else if name == "META-INF/manifest.xml" && manifest.is_some() {
            output
                .start_file(
                    name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
                )
                .map_err(|e| e.to_string())?;
            output
                .write_all(manifest.as_ref().unwrap())
                .map_err(|e| e.to_string())?;
        } else {
            output.raw_copy_file(file).map_err(|e| e.to_string())?;
        }
    }
    for (path, bytes, _) in new_media {
        if input.by_name(&path).is_ok() {
            return Err("ODP media path collision".into());
        }
        output
            .start_file(
                path,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .map_err(|e| e.to_string())?;
        output.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    Ok(output.finish().map_err(|e| e.to_string())?.into_inner())
}

fn prune_removed_pages(
    input: &mut ZipArchive<Cursor<&[u8]>>,
    source: &Source,
    deck: &Presentation,
    original: &str,
    updated: &str,
) -> Result<(HashSet<String>, Option<Vec<u8>>), String> {
    let removed = source
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| {
            !deck
                .slides()
                .iter()
                .any(|s| s.source_identity() == p.slide_id)
        })
        .map(|(i, _)| i)
        .collect::<Vec<_>>();
    if removed.is_empty() {
        return Ok((HashSet::new(), None));
    }
    let original_tree = roxmltree::Document::parse(original).map_err(|e| e.to_string())?;
    let pages = original_tree
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "page"
                && n.tag_name().namespace()
                    == Some("urn:oasis:names:tc:opendocument:xmlns:drawing:1.0")
        })
        .collect::<Vec<_>>();
    let names = (0..input.len())
        .map(|i| {
            input
                .by_index(i)
                .map(|f| f.name().to_owned())
                .map_err(|e| e.to_string())
        })
        .collect::<Result<HashSet<_>, _>>()?;
    let mut candidates = HashSet::new();
    for index in removed {
        let page = pages.get(index).ok_or("ODP page missing")?;
        for a in page
            .descendants()
            .filter(|n| n.is_element())
            .flat_map(|n| n.attributes())
        {
            if a.name() != "href" {
                continue;
            }
            let href = a.value().trim_start_matches("./");
            if href.starts_with('#') || href.contains("://") || href.starts_with("mailto:") {
                continue;
            }
            if href.contains('%') || href.split('/').any(|s| s == "..") || href.starts_with('/') {
                return Err("Cannot safely remove this ODP slide's references".into());
            }
            if names.contains(href) {
                if !href.starts_with("Pictures/") {
                    return Err("Cannot safely remove this ODP slide's embedded object".into());
                }
                candidates.insert(href.to_owned());
            }
        }
    }
    let mut referenced = HashSet::new();
    let mut xml_parts = HashMap::new();
    xml_parts.insert("content.xml".to_owned(), updated.as_bytes().to_vec());
    for name in names.iter().filter(|n| {
        n.ends_with(".xml") && n.as_str() != "content.xml" && n.as_str() != "META-INF/manifest.xml"
    }) {
        xml_parts.insert(name.clone(), zip_read(input, name)?);
    }
    for bytes in xml_parts.values() {
        let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
        let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
        for a in doc
            .descendants()
            .filter(|n| n.is_element())
            .flat_map(|n| n.attributes())
        {
            let v = a.value().trim_start_matches("./");
            if candidates.contains(v) {
                referenced.insert(v.to_owned());
            }
        }
    }
    let prune = candidates
        .difference(&referenced)
        .cloned()
        .collect::<HashSet<_>>();
    if prune.is_empty() {
        return Ok((prune, None));
    }
    let bytes = zip_read(input, "META-INF/manifest.xml")?;
    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
    let doc = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let mut ranges = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "file-entry"
                && attr(*n, "full-path").is_some_and(|v| prune.contains(v))
        })
        .map(|n| n.range())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    let mut manifest = text.to_owned();
    for r in ranges {
        manifest.replace_range(r, "");
    }
    Ok((prune, Some(manifest.into_bytes())))
}

fn appended_object(
    slide: u64,
    o: &SlideObject,
    styles: &mut String,
    media: &mut Vec<(String, Vec<u8>, &'static str)>,
) -> Result<String, String> {
    let name = format!("suite42_{slide}_{}", o.id);
    let ns="xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
    styles.push_str(&format!("<style:style xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" {ns} style:name=\"{name}\" style:family=\"graphic\"><style:graphic-properties draw:fill=\"{}\" draw:fill-color=\"#{:02x}{:02x}{:02x}\" draw:stroke=\"{}\" svg:stroke-color=\"#{:02x}{:02x}{:02x}\"/></style:style>",if o.fill_visible{"solid"}else{"none"},o.fill[0],o.fill[1],o.fill[2],if o.stroke_visible{"solid"}else{"none"},o.stroke[0],o.stroke[1],o.stroke[2]));
    let object_text = if o.paragraphs.is_empty() {
        styled_text(&o.text, &name, &o.text_style, styles)
    } else {
        rich_text(&o.paragraphs, &name, styles)
    };
    let geo = format!("{ns} draw:style-name=\"{name}\" {}", geometry_attrs(o));
    Ok(match o.kind {
        ObjectKind::Rectangle | ObjectKind::Ellipse => {
            let tag = if o.kind == ObjectKind::Rectangle {
                "rect"
            } else {
                "ellipse"
            };
            format!("<draw:{tag} {geo}>{object_text}</draw:{tag}>")
        }
        ObjectKind::Line => {
            let transform = if o.rotation_degrees.rem_euclid(360.0) != 0.0 {
                format!(" draw:transform=\"{}\"", rotation_transform(o))
            } else {
                String::new()
            };
            let (x, y) = if transform.is_empty() {
                (o.x, o.y)
            } else {
                (0.0, 0.0)
            };
            format!("<draw:line {ns} draw:style-name=\"{name}\"{transform} svg:x1=\"{}\" svg:y1=\"{}\" svg:x2=\"{}\" svg:y2=\"{}\"/>",pt(x),pt(y),pt(x+o.width),pt(y+o.height))
        }
        ObjectKind::Text => {
            format!("<draw:frame {geo}><draw:text-box>{object_text}</draw:text-box></draw:frame>")
        }
        ObjectKind::Picture => {
            let (ext, mime) = if o.media.starts_with(b"\x89PNG\r\n\x1a\n") {
                ("png", "image/png")
            } else if o.media.starts_with(b"\xff\xd8\xff") {
                ("jpg", "image/jpeg")
            } else {
                return Err("Unsupported image encoding".into());
            };
            let path = format!("Pictures/{name}.{ext}");
            media.push((path.clone(), o.media.clone(), mime));
            format!("<draw:frame {geo}><draw:image xlink:href=\"{path}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/></draw:frame>")
        }
        ObjectKind::Table => {
            let mut result = format!("<draw:frame {geo}><table:table table:name=\"{name}\">");
            for row in &o.cells {
                result.push_str("<table:table-row>");
                for cell in row {
                    result.push_str(&format!(
                        "<table:table-cell office:value-type=\"string\">{}</table:table-cell>",
                        text(cell)
                    ));
                }
                result.push_str("</table:table-row>");
            }
            result.push_str("</table:table></draw:frame>");
            result
        }
    })
}
