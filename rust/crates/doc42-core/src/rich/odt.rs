//! OpenDocument Text package adapter for common text, styles, lists, tables and pictures.
use super::*;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    io::{Cursor, Read, Write},
};
use zip::{write::SimpleFileOptions, CompressionMethod, ZipArchive, ZipWriter};
const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const T: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const DC: &str = "http://purl.org/dc/elements/1.1/";
const S: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const F: &str = "urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0";
const TB: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const D: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";
const XL: &str = "http://www.w3.org/1999/xlink";
const MIME: &str = "application/vnd.oasis.opendocument.text";
const NS:&str="xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"";
fn bad(e: impl std::fmt::Display) -> Error {
    Error::Invalid(e.to_string())
}
fn unsupported(s: impl Into<String>) -> Error {
    Error::Unsupported(s.into())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn attr<'a>(n: roxmltree::Node<'a, 'a>, ns: &str, name: &str) -> Option<&'a str> {
    n.attribute((ns, name))
}
fn length(value: &str) -> Result<i32, Error> {
    let s = value.trim();
    let (n, factor) = if let Some(v) = s.strip_suffix("pt") {
        (v, 20.0)
    } else if let Some(v) = s.strip_suffix("cm") {
        (v, 1440.0 / 2.54)
    } else if let Some(v) = s.strip_suffix("mm") {
        (v, 1440.0 / 25.4)
    } else if let Some(v) = s.strip_suffix("in") {
        (v, 1440.0)
    } else if let Some(v) = s.strip_suffix("px") {
        (v, 15.0)
    } else if s == "0" {
        ("0", 1.0)
    } else {
        return Err(unsupported("ODT length unit"));
    };
    let n = n.parse::<f64>().map_err(bad)? * factor;
    if !n.is_finite() || !(-2_000_000.0..=2_000_000.0).contains(&n) {
        return Err(bad("ODT length"));
    }
    Ok(n.round() as i32)
}
fn nonnegative(v: &str) -> Result<u32, Error> {
    u32::try_from(length(v)?).map_err(|_| bad("negative ODT length"))
}
fn color(v: &str) -> Result<u32, Error> {
    let h = v
        .strip_prefix('#')
        .ok_or_else(|| unsupported("ODT color"))?;
    if h.len() != 6 {
        return Err(unsupported("ODT color"));
    }
    u32::from_str_radix(h, 16).map_err(bad)
}
#[derive(Clone, Default)]
struct Def {
    parent: Option<String>,
    run: RunPatch,
    para: ParaPatch,
}
#[derive(Clone, Default)]
struct RunPatch {
    font: Option<String>,
    size: Option<u16>,
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    color: Option<u32>,
    highlight: Option<u32>,
    extra: BTreeMap<String, String>,
}
#[derive(Clone, Default)]
struct ParaPatch {
    align: Option<Alignment>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    before: Option<u32>,
    after: Option<u32>,
    line: Option<i32>,
    tabs: Option<Vec<i32>>,
    extra: BTreeMap<String, String>,
}
struct Styles {
    defs: HashMap<(String, String), Def>,
    list_types: HashMap<(String, u8), bool>,
    cell_fills: HashMap<String, Option<u32>>,
    cell_borders: HashMap<String, Option<Border>>,
    column_widths: HashMap<String, u32>,
    unsupported_column_styles: HashSet<String>,
    unsupported_cell_styles: HashSet<String>,
    default_paragraph: Def,
    default_text: Def,
    named: HashSet<(String, String)>,
}
impl Styles {
    fn new(docs: &[&roxmltree::Document<'_>]) -> Result<Self, Error> {
        let mut out = Self {
            defs: HashMap::new(),
            list_types: HashMap::new(),
            cell_fills: HashMap::new(),
            cell_borders: HashMap::new(),
            column_widths: HashMap::new(),
            unsupported_column_styles: HashSet::new(),
            unsupported_cell_styles: HashSet::new(),
            default_paragraph: Def::default(),
            default_text: Def::default(),
            named: HashSet::new(),
        };
        let mut fonts = HashMap::new();
        for doc in docs {
            for n in doc
                .descendants()
                .filter(|n| n.has_tag_name((S, "font-face")))
            {
                if let (Some(name), Some(face)) = (attr(n, S, "name"), attr(n, SVG, "font-family"))
                {
                    fonts.insert(name.to_owned(), face.trim_matches(['\'', '"']).to_owned());
                }
            }
        }
        for doc in docs {
            for n in doc
                .descendants()
                .filter(|n| n.has_tag_name((S, "default-style")))
            {
                let family = attr(n, S, "family").unwrap_or("");
                let mut d = Def::default();
                for p in n.children().filter(|n| n.is_element()) {
                    match p.tag_name().name() {
                        "text-properties" => d.run = run_patch(p)?,
                        "paragraph-properties" => d.para = para_patch(p)?,
                        _ => {}
                    }
                }
                match family {
                    "paragraph" => out.default_paragraph = d,
                    "text" => out.default_text = d,
                    _ => {}
                }
            }
            for n in doc.descendants().filter(|n| n.has_tag_name((S, "style"))) {
                let Some(name) = attr(n, S, "name") else {
                    continue;
                };
                let family = attr(n, S, "family").unwrap_or("");
                if family == "table-column" {
                    let mut width = None;
                    let mut unsupported_style = attr(n, S, "parent-style-name").is_some();
                    for property in n.children().filter(|p| p.is_element()) {
                        if !property.has_tag_name((S, "table-column-properties"))
                            || property.children().any(|c| c.is_element())
                        {
                            unsupported_style = true;
                            continue;
                        }
                        for a in property.attributes() {
                            if a.namespace() == Some(S) && a.name() == "column-width" {
                                match nonnegative(a.value()) {
                                    Ok(v) if v > 0 && v <= 2_000_000 => width = Some(v),
                                    _ => unsupported_style = true,
                                }
                            } else {
                                unsupported_style = true;
                            }
                        }
                    }
                    if let Some(width) = width {
                        out.column_widths.insert(name.into(), width);
                    } else {
                        unsupported_style = true;
                    }
                    if unsupported_style {
                        out.unsupported_column_styles.insert(name.into());
                    }
                    continue;
                }
                if family == "table-cell" {
                    let mut fill = None;
                    let mut border = None;
                    let mut unsupported_style = attr(n, S, "parent-style-name").is_some();
                    for property in n.children().filter(|p| p.is_element()) {
                        if !property.has_tag_name((S, "table-cell-properties"))
                            || property.children().any(|c| c.is_element())
                        {
                            unsupported_style = true;
                            continue;
                        }
                        for a in property.attributes() {
                            if a.namespace() == Some(F) && a.name() == "background-color" {
                                if a.value() != "transparent" {
                                    match color(a.value()) {
                                        Ok(value) => fill = Some(value),
                                        Err(_) => unsupported_style = true,
                                    }
                                }
                            } else if a.namespace() == Some(F) && a.name() == "border" {
                                if a.value() != "none" {
                                    let parts = a.value().split_whitespace().collect::<Vec<_>>();
                                    if parts.len() != 3 || parts[1] != "solid" {
                                        unsupported_style = true;
                                    } else {
                                        match (nonnegative(parts[0]), color(parts[2])) {
                                            (Ok(width), Ok(color))
                                                if width > 0 && width <= u16::MAX as u32 =>
                                            {
                                                border = Some(Border {
                                                    width_twips: width as u16,
                                                    color,
                                                })
                                            }
                                            _ => unsupported_style = true,
                                        }
                                    }
                                }
                            } else {
                                unsupported_style = true;
                            }
                        }
                    }
                    out.cell_fills.insert(name.into(), fill);
                    out.cell_borders.insert(name.into(), border);
                    if unsupported_style {
                        out.unsupported_cell_styles.insert(name.into());
                    }
                    continue;
                }
                if !matches!(family, "paragraph" | "text") {
                    continue;
                }
                if n.parent().is_some_and(|p| p.has_tag_name((O, "styles"))) {
                    out.named.insert((family.into(), name.into()));
                }
                let mut d = Def {
                    parent: attr(n, S, "parent-style-name").map(str::to_owned),
                    ..Def::default()
                };
                for p in n.children().filter(|n| n.is_element()) {
                    match p.tag_name().name() {
                        "text-properties" => d.run = run_patch(p)?,
                        "paragraph-properties" => d.para = para_patch(p)?,
                        _ => {}
                    }
                }
                out.defs.insert((family.into(), name.into()), d);
            }
            for n in doc
                .descendants()
                .filter(|n| n.has_tag_name((T, "list-style")))
            {
                if let Some(name) = attr(n, S, "name") {
                    for child in n.children().filter(|c| c.is_element()) {
                        let numbered = match child.tag_name().name() {
                            "list-level-style-number" => true,
                            "list-level-style-bullet" => false,
                            _ => return Err(unsupported("ODT list level style")),
                        };
                        let level = attr(child, T, "level")
                            .unwrap_or("1")
                            .parse::<u8>()
                            .map_err(bad)?;
                        out.list_types.insert((name.into(), level), numbered);
                    }
                }
            }
        }
        for d in out
            .defs
            .values_mut()
            .chain([&mut out.default_paragraph, &mut out.default_text])
        {
            if let Some(font) = &d.run.font {
                if let Some(face) = fonts.get(font) {
                    d.run.font = Some(face.clone());
                }
            }
        }
        Ok(out)
    }
    fn defaults(&self, run: &mut TextStyle, para: &mut ParagraphStyle) {
        apply_run(run, &self.default_text.run);
        apply_run(run, &self.default_paragraph.run);
        apply_para(para, &self.default_paragraph.para);
    }
    fn resolve(
        &self,
        family: &str,
        name: &str,
        run: &mut TextStyle,
        para: &mut ParagraphStyle,
        seen: &mut HashSet<String>,
    ) -> Result<(), Error> {
        if !seen.insert(name.into()) {
            return Err(bad("cyclic ODT style"));
        }
        let d = self
            .defs
            .get(&(family.into(), name.into()))
            .ok_or_else(|| unsupported(format!("ODT style {name}")))?;
        if let Some(parent) = &d.parent {
            self.resolve(family, parent, run, para, seen)?
        }
        apply_run(run, &d.run);
        apply_para(para, &d.para);
        Ok(())
    }
    fn named_source(&self, family: &str, name: Option<&str>) -> Option<String> {
        let mut current = name?;
        let mut seen = HashSet::new();
        loop {
            if !seen.insert(current.to_owned()) {
                return None;
            }
            if self.named.contains(&(family.into(), current.into())) {
                return Some(current.into());
            }
            current = self
                .defs
                .get(&(family.into(), current.into()))?
                .parent
                .as_deref()?;
        }
    }
    fn apply(
        &self,
        family: &str,
        name: Option<&str>,
        run: &mut TextStyle,
        para: &mut ParagraphStyle,
    ) -> Result<(), Error> {
        if let Some(name) = name {
            self.resolve(family, name, run, para, &mut HashSet::new())?;
        }
        Ok(())
    }
}
fn attr_key(a: roxmltree::Attribute<'_, '_>) -> Result<String, Error> {
    let prefix = match a.namespace() {
        Some(F) => "fo",
        Some(S) => "style",
        Some(T) => "text",
        Some(SVG) => "svg",
        _ => return Err(unsupported("unknown ODT style namespace")),
    };
    Ok(format!("{prefix}:{}", a.name()))
}
fn extra_attrs(extra: &BTreeMap<String, String>) -> Result<String, Error> {
    let mut out = String::new();
    for (k, v) in extra {
        let (prefix, local) = k.split_once(':').ok_or_else(|| bad("ODT attribute name"))?;
        if !matches!(prefix, "fo" | "style" | "text" | "svg")
            || local.is_empty()
            || !local
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(bad("ODT attribute name"));
        }
        out.push_str(&format!(" {k}=\"{}\"", esc(v)));
    }
    Ok(out)
}
fn run_patch(n: roxmltree::Node<'_, '_>) -> Result<RunPatch, Error> {
    let mut p = RunPatch::default();
    for a in n.attributes() {
        match (a.namespace(), a.name()) {
            (Some(S), "font-name") => p.font = Some(a.value().into()),
            (Some(F), "font-family") => p.font = Some(a.value().trim_matches(['\'', '"']).into()),
            (Some(F), "font-size") => {
                let n = length(a.value())?;
                p.size = Some(u16::try_from((n + 5) / 10).map_err(|_| bad("ODT font size"))?)
            }
            (Some(F), "font-weight") => {
                p.bold = Some(matches!(a.value(), "bold" | "700" | "800" | "900"))
            }
            (Some(F), "font-style") => p.italic = Some(matches!(a.value(), "italic" | "oblique")),
            (Some(S), "text-underline-style") => p.underline = Some(a.value() != "none"),
            (Some(S), "text-line-through-style") => p.strike = Some(a.value() != "none"),
            (Some(F), "color") => p.color = Some(color(a.value())?),
            (Some(F), "background-color") if a.value() != "transparent" => {
                p.highlight = Some(color(a.value())?)
            }
            _ => {
                p.extra.insert(attr_key(a)?, a.value().into());
            }
        }
    }
    Ok(p)
}
fn para_patch(n: roxmltree::Node<'_, '_>) -> Result<ParaPatch, Error> {
    let mut p = ParaPatch::default();
    for a in n.attributes() {
        match (a.namespace(), a.name()) {
            (Some(F), "text-align") => {
                p.align = Some(match a.value() {
                    "start" | "left" => Alignment::Left,
                    "center" => Alignment::Center,
                    "end" | "right" => Alignment::Right,
                    "justify" => Alignment::Justify,
                    _ => return Err(unsupported("ODT alignment")),
                })
            }
            (Some(F), "margin-left") => p.left = Some(length(a.value())?),
            (Some(F), "margin-right") => p.right = Some(length(a.value())?),
            (Some(F), "text-indent") => p.first = Some(length(a.value())?),
            (Some(F), "margin-top") => p.before = Some(nonnegative(a.value())?),
            (Some(F), "margin-bottom") => p.after = Some(nonnegative(a.value())?),
            (Some(F), "line-height") => {
                if a.value().ends_with('%') {
                    p.extra.insert(attr_key(a)?, a.value().into());
                } else {
                    p.line = Some(length(a.value())?)
                }
            }
            _ => {
                p.extra.insert(attr_key(a)?, a.value().into());
            }
        }
    }
    if let Some(stops) = n.children().find(|c| c.has_tag_name((S, "tab-stops"))) {
        let mut tabs = Vec::new();
        for tab in stops.children().filter(|c| c.is_element()) {
            if !tab.has_tag_name((S, "tab-stop"))
                || !matches!(attr(tab, S, "type"), None | Some("left"))
            {
                return Err(unsupported("ODT tab type"));
            }
            tabs.push(length(
                attr(tab, S, "position").ok_or_else(|| bad("ODT tab position"))?,
            )?);
        }
        tabs.sort_unstable();
        tabs.dedup();
        p.tabs = Some(tabs);
    }
    Ok(p)
}
fn apply_run(t: &mut TextStyle, p: &RunPatch) {
    t.odt_extra.extend(p.extra.clone());
    if let Some(v) = &p.font {
        t.font = v.clone()
    }
    if let Some(v) = p.size {
        t.size_half_points = v
    }
    if let Some(v) = p.bold {
        t.bold = v
    }
    if let Some(v) = p.italic {
        t.italic = v
    }
    if let Some(v) = p.underline {
        t.underline = v
    }
    if let Some(v) = p.strike {
        t.strike = v
    }
    if let Some(v) = p.color {
        t.color = Some(v)
    }
    if let Some(v) = p.highlight {
        t.highlight = Some(v)
    }
}
fn apply_para(t: &mut ParagraphStyle, p: &ParaPatch) {
    if let Some(v) = &p.tabs {
        t.tabs_twips = v.clone();
    }
    t.odt_extra.extend(p.extra.clone());
    if let Some(v) = p.align {
        t.alignment = v
    }
    if let Some(v) = p.left {
        t.left_twips = v
    }
    if let Some(v) = p.right {
        t.right_twips = v
    }
    if let Some(v) = p.first {
        t.first_line_twips = v
    }
    if let Some(v) = p.before {
        t.before_twips = v
    }
    if let Some(v) = p.after {
        t.after_twips = v
    }
    if let Some(v) = p.line {
        t.line_twips = v
    }
}
fn read_parts(bytes: &[u8]) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut z = ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
    if z.len() > 1024 {
        return Err(Error::Limit);
    }
    let mut total = 0u64;
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(bad)?;
        if f.is_dir() {
            continue;
        }
        let name = f.name().to_owned();
        if name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|x| x == ".." || x.is_empty())
            || out.contains_key(&name)
        {
            return Err(bad("invalid ODT package path"));
        }
        total = total.checked_add(f.size()).ok_or(Error::Limit)?;
        if total > MAX_BYTES as u64 {
            return Err(Error::Limit);
        }
        let mut data = Vec::new();
        f.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut data)
            .map_err(bad)?;
        if data.len() > MAX_BYTES {
            return Err(Error::Limit);
        }
        out.insert(name, data);
    }
    if out.get("mimetype").map(Vec::as_slice) != Some(MIME.as_bytes()) {
        return Err(bad("not an ODT package"));
    }
    Ok(out)
}
struct OdtComment {
    author: String,
    text: String,
    date: Option<String>,
}
fn read_comment(node: roxmltree::Node<'_, '_>) -> Result<OdtComment, Error> {
    for attr in node.attributes() {
        if attr.namespace() != Some(O) || attr.name() != "name" {
            return Err(unsupported("ODT comment metadata"));
        }
    }
    let mut author = String::new();
    let mut date = None;
    let mut paragraphs = Vec::new();
    for child in node.children().filter(|n| n.is_element()) {
        if child.has_tag_name((DC, "creator")) {
            author = child.text().unwrap_or("").into();
        } else if child.has_tag_name((DC, "date")) {
            date = Some(child.text().unwrap_or("").into());
        } else if child.has_tag_name((T, "p")) {
            if child.attributes().len() != 0 {
                return Err(unsupported("formatted ODT comment"));
            }
            let mut text = String::new();
            for node in child.children() {
                if node.is_text() {
                    text.push_str(node.text().unwrap_or(""));
                } else if node.has_tag_name((T, "s")) {
                    let count = attr(node, T, "c")
                        .unwrap_or("1")
                        .parse::<usize>()
                        .map_err(bad)?;
                    if count > 1_000_000 {
                        return Err(Error::Limit);
                    }
                    text.push_str(&" ".repeat(count));
                } else if node.has_tag_name((T, "tab")) {
                    text.push('\t');
                } else if node.is_element() {
                    return Err(unsupported("formatted ODT comment"));
                }
            }
            paragraphs.push(text);
        } else {
            return Err(unsupported("ODT comment content"));
        }
    }
    Ok(OdtComment {
        author,
        text: paragraphs.join("\n"),
        date,
    })
}
fn standalone_xml(node: roxmltree::Node<'_, '_>) -> Result<String, Error> {
    let mut xml = node.document().input_text()[node.range()].to_owned();
    let bytes = xml.as_bytes();
    let mut end = None;
    let mut quote = None;
    for (i, &byte) in bytes.iter().enumerate() {
        match (quote, byte) {
            (Some(q), b) if b == q => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| bad("ODT source element"))?;
    // Namespace declarations are not exposed as ordinary attributes by roxmltree.
    // Tokenize only the start tag to distinguish xmlns:t from xmlns:text and
    // avoid matching declaration-like text inside attribute values.
    let mut declared = HashSet::new();
    let mut i = 1;
    while i < end && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' {
        i += 1;
    }
    while i < end {
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == end || bytes[i] == b'/' {
            break;
        }
        let start = i;
        while i < end && !bytes[i].is_ascii_whitespace() && bytes[i] != b'=' {
            i += 1;
        }
        let name = std::str::from_utf8(&bytes[start..i]).map_err(bad)?;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == end || bytes[i] != b'=' {
            return Err(bad("ODT source attribute"));
        }
        i += 1;
        while i < end && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i == end || !matches!(bytes[i], b'\'' | b'"') {
            return Err(bad("ODT source attribute quote"));
        }
        let q = bytes[i];
        i += 1;
        while i < end && bytes[i] != q {
            i += 1;
        }
        if i == end {
            return Err(bad("ODT source attribute quote"));
        }
        i += 1;
        if name == "xmlns" || name.starts_with("xmlns:") {
            declared.insert(name.to_owned());
        }
    }
    let mut insert = end;
    while insert > 0 && bytes[insert - 1].is_ascii_whitespace() {
        insert -= 1;
    }
    if insert > 0 && bytes[insert - 1] == b'/' {
        insert -= 1;
    }
    let mut declarations = String::new();
    // roxmltree's node namespaces are the complete in-scope set, with the
    // nearest declaration winning for re-bound prefixes.
    for ns in node.namespaces() {
        let name = match ns.name() {
            Some("xml") => continue,
            Some(prefix) => format!("xmlns:{prefix}"),
            None => "xmlns".to_owned(),
        };
        if !declared.contains(&name) {
            declarations.push_str(&format!(" {name}=\"{}\"", esc(ns.uri())));
        }
    }
    xml.insert_str(insert, &declarations);
    Ok(xml)
}

struct PendingToc {
    entries: Vec<(u64, String)>,
    cache: OdtTocCache,
    max_level: u8,
}
struct Reader<'a> {
    styles: Styles,
    parts: &'a BTreeMap<String, Vec<u8>>,
    state: State,
    next: u64,
    pending_fields: Vec<(usize, FieldKind)>,
    pending_comments: Vec<(usize, String, bool)>,
    pending_notes: Vec<(usize, Vec<Paragraph>, bool, Option<String>)>,
    inside_note: bool,
    comment_marks: Vec<(u64, usize, String, bool)>,
    comment_data: BTreeMap<String, OdtComment>,
    point_counter: u64,
    bookmarks: BTreeMap<String, u64>,
    pending_bookmarks: Vec<(usize, String, u8)>,
    bookmark_marks: Vec<(Position, String, u8)>,
    pending_tocs: Vec<PendingToc>,
    source_automatic_styles: BTreeMap<String, (String, Vec<String>)>,
}
impl Reader<'_> {
    fn index_styles(&self, node: roxmltree::Node<'_, '_>) -> String {
        let mut pending = node
            .descendants()
            .filter(|n| n.is_element())
            .flat_map(|n| n.attributes())
            .filter(|a| a.name().ends_with("style-name"))
            .map(|a| a.value().to_owned())
            .collect::<Vec<_>>();
        let mut selected = std::collections::BTreeSet::new();
        while let Some(name) = pending.pop() {
            if !selected.insert(name.clone()) {
                continue;
            }
            if let Some((_, dependencies)) = self.source_automatic_styles.get(&name) {
                pending.extend(dependencies.iter().cloned());
            }
        }
        selected
            .into_iter()
            .filter_map(|name| {
                self.source_automatic_styles
                    .get(&name)
                    .map(|(xml, _)| xml.as_str())
            })
            .collect()
    }
    fn id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
    fn inline(
        &mut self,
        n: roxmltree::Node<'_, '_>,
        run: &TextStyle,
        para: &ParagraphStyle,
        link: Option<String>,
        out: &mut Vec<Run>,
    ) -> Result<(), Error> {
        if n.is_text() {
            let text = n.text().unwrap_or("");
            if !text.is_empty() {
                out.push(Run {
                    text: text.into(),
                    style: run.clone(),
                    link,
                })
            }
            return Ok(());
        }
        if !n.is_element() {
            return Ok(());
        }
        if n.has_tag_name((T, "note")) {
            if self.inside_note {
                return Err(unsupported("nested ODT note"));
            }
            let endnote = match attr(n, T, "note-class") {
                None | Some("footnote") => false,
                Some("endnote") => true,
                _ => return Err(unsupported("ODT note class")),
            };
            for a in n.attributes() {
                if a.namespace() != Some(T) || !["id", "note-class"].contains(&a.name()) {
                    return Err(unsupported("ODT note metadata"));
                }
            }
            let citation =
                if let Some(c) = n.children().find(|c| c.has_tag_name((T, "note-citation"))) {
                    if c.attributes().len() != 0 || c.children().any(|n| n.is_element()) {
                        return Err(unsupported("ODT note citation metadata"));
                    }
                    Some(c.text().unwrap_or("").to_owned())
                } else {
                    None
                };
            let body = n
                .children()
                .find(|c| c.has_tag_name((T, "note-body")))
                .ok_or_else(|| bad("missing ODT note body"))?;
            if n.children()
                .filter(|c| c.is_element())
                .any(|c| !c.has_tag_name((T, "note-citation")) && !c.has_tag_name((T, "note-body")))
            {
                return Err(unsupported("ODT note content"));
            }
            let fields = std::mem::take(&mut self.pending_fields);
            let comments = std::mem::take(&mut self.pending_comments);
            let notes = std::mem::take(&mut self.pending_notes);
            let old_marks = self.comment_marks.len();
            let old_fields = self.state.fields.len();
            let old_notes = self.state.footnotes.len();
            self.inside_note = true;
            let mut paragraphs = Vec::new();
            for paragraph in body.children().filter(|c| c.is_element()) {
                if !paragraph.has_tag_name((T, "p")) {
                    return Err(unsupported("ODT note body"));
                }
                paragraphs.push(self.para(paragraph, None)?);
            }
            self.inside_note = false;
            if paragraphs.is_empty()
                || !self.pending_fields.is_empty()
                || !self.pending_comments.is_empty()
                || !self.pending_notes.is_empty()
                || self.comment_marks.len() != old_marks
                || self.state.fields.len() != old_fields
                || self.state.footnotes.len() != old_notes
            {
                return Err(unsupported("ODT note body metadata"));
            }
            self.pending_fields = fields;
            self.pending_comments = comments;
            self.pending_notes = notes;
            self.pending_notes.push((
                out.iter().map(|r| r.text.len()).sum(),
                paragraphs,
                endnote,
                citation,
            ));
            return Ok(());
        }
        if n.has_tag_name((O, "annotation")) {
            let offset = out.iter().map(|r| r.text.len()).sum();
            let named = attr(n, O, "name");
            let name = if let Some(name) = named {
                name.to_owned()
            } else {
                self.point_counter += 1;
                format!("__DOC42_POINT_{}", self.point_counter)
            };
            let item = read_comment(n)?;
            if self.comment_data.insert(name.clone(), item).is_some() {
                return Err(bad("duplicate ODT comment name"));
            }
            self.pending_comments.push((offset, name.clone(), true));
            if named.is_none() {
                self.pending_comments.push((offset, name, false));
            }
            return Ok(());
        }
        if n.has_tag_name((O, "annotation-end")) {
            let name = attr(n, O, "name").ok_or_else(|| bad("missing ODT comment name"))?;
            if n.attributes().len() != 1 || n.children().any(|c| c.is_element()) {
                return Err(unsupported("ODT comment end"));
            }
            self.pending_comments.push((
                out.iter().map(|r| r.text.len()).sum(),
                name.to_owned(),
                false,
            ));
            return Ok(());
        }
        match n.tag_name().name() {
            "bookmark" | "bookmark-start" | "bookmark-end"
                if n.tag_name().namespace() == Some(T) =>
            {
                if attr(n, T, "name").is_none()
                    || n.attributes().len() != 1
                    || n.children().any(|c| c.is_element())
                {
                    return Err(unsupported("ODT bookmark metadata"));
                }
                if self.inside_note {
                    return Err(unsupported("ODT note bookmark"));
                }
                let name = attr(n, T, "name").unwrap().to_owned();
                let kind = match n.tag_name().name() {
                    "bookmark" => 0,
                    "bookmark-start" => 1,
                    _ => 2,
                };
                self.pending_bookmarks
                    .push((out.iter().map(|r| r.text.len()).sum(), name, kind));
                Ok(())
            }
            "span" => {
                let mut t = run.clone();
                let mut p = para.clone();
                self.styles
                    .apply("text", attr(n, T, "style-name"), &mut t, &mut p)?;
                t.source_style = self.styles.named_source("text", attr(n, T, "style-name"));
                for c in n.children() {
                    self.inline(c, &t, &p, link.clone(), out)?
                }
                Ok(())
            }
            "a" => {
                if n.attributes().any(|a| !match (a.namespace(), a.name()) {
                    (Some(XL), "href") => true,
                    (Some(XL), "type") => a.value() == "simple",
                    (Some(XL), "show") => a.value() == "replace",
                    (Some(XL), "actuate") => a.value() == "onRequest",
                    (Some(T), "style-name") => true,
                    _ => false,
                }) {
                    return Err(unsupported("ODT hyperlink metadata"));
                }
                let target = attr(n, XL, "href")
                    .ok_or_else(|| bad("ODT link"))?
                    .to_owned();
                let mut t = run.clone();
                let mut p = para.clone();
                self.styles
                    .apply("text", attr(n, T, "style-name"), &mut t, &mut p)?;
                if let Some(name) = self.styles.named_source("text", attr(n, T, "style-name")) {
                    t.source_style = Some(name);
                }
                for c in n.children() {
                    self.inline(c, &t, &p, Some(target.clone()), out)?
                }
                Ok(())
            }
            "s" => {
                let count = attr(n, T, "c")
                    .unwrap_or("1")
                    .parse::<usize>()
                    .map_err(bad)?;
                if count > 1_000_000 {
                    return Err(Error::Limit);
                }
                out.push(Run {
                    text: " ".repeat(count),
                    style: run.clone(),
                    link,
                });
                Ok(())
            }
            "tab" => {
                out.push(Run {
                    text: "\t".into(),
                    style: run.clone(),
                    link,
                });
                Ok(())
            }
            "page-number" | "page-count" if n.tag_name().namespace() == Some(T) => {
                if n.tag_name().name() == "page-number"
                    && !matches!(attr(n, T, "select-page"), None | Some("current"))
                {
                    return Err(unsupported("ODT page field selection"));
                }
                let offset = out.iter().map(|r| r.text.len()).sum();
                self.pending_fields.push((
                    offset,
                    if n.tag_name().name() == "page-number" {
                        FieldKind::PageNumber
                    } else {
                        FieldKind::PageCount
                    },
                ));
                Ok(())
            }
            "line-break" => Err(unsupported("ODT line break inside paragraph")),
            name => Err(unsupported(format!("ODT inline element {name}"))),
        }
    }
    fn para(
        &mut self,
        n: roxmltree::Node<'_, '_>,
        list: Option<ListMarker>,
    ) -> Result<Paragraph, Error> {
        let mut run = TextStyle::default();
        let mut para = ParagraphStyle::default();
        self.styles.defaults(&mut run, &mut para);
        let name = attr(n, T, "style-name");
        self.styles.apply("paragraph", name, &mut run, &mut para)?;
        para.source_style = self.styles.named_source("paragraph", name);
        if n.tag_name().name() == "h" {
            let l = attr(n, T, "outline-level")
                .unwrap_or("1")
                .parse::<u8>()
                .map_err(bad)?;
            para.outline_level = Some(l.saturating_sub(1));
        }
        para.list = list;
        let mut runs = Vec::new();
        for c in n.children() {
            self.inline(c, &run, &para, None, &mut runs)?
        }
        let id = self.id();
        for (offset, name, kind) in std::mem::take(&mut self.pending_bookmarks) {
            if kind != 2 && self.bookmarks.insert(name.clone(), id).is_some() {
                return Err(bad("duplicate ODT bookmark"));
            }
            self.bookmark_marks.push((
                Position {
                    paragraph: id,
                    offset,
                },
                name,
                kind,
            ));
        }
        for (offset, paragraphs, endnote, citation) in std::mem::take(&mut self.pending_notes) {
            let note_id = self.id();
            self.state.footnotes.push(Footnote {
                id: note_id,
                at: Position {
                    paragraph: id,
                    offset,
                },
                paragraphs,
                endnote,
                citation,
            });
        }
        for (offset, name, start) in std::mem::take(&mut self.pending_comments) {
            self.comment_marks.push((id, offset, name, start));
        }
        for (offset, kind) in std::mem::take(&mut self.pending_fields) {
            let field_id = self.id();
            self.state.fields.push(Field {
                id: field_id,
                at: Position {
                    paragraph: id,
                    offset,
                },
                kind,
            });
        }
        Ok(Paragraph {
            id,
            style: para,
            runs,
        })
    }
    fn blocks(
        &mut self,
        parent: roxmltree::Node<'_, '_>,
        level: u8,
        active: Option<(u64, bool)>,
    ) -> Result<(), Error> {
        for n in parent.children().filter(|n| n.is_element()) {
            match n.tag_name().name() {
                "p" | "h" => {
                    if let Some(frame) = n.children().find(|c| c.has_tag_name((D, "frame"))) {
                        if n.children()
                            .any(|c| c.is_text() && !c.text().unwrap_or("").trim().is_empty())
                            || n.children().filter(|c| c.is_element()).count() != 1
                        {
                            return Err(unsupported("ODT inline image mixed with text"));
                        }
                        let image = self.image(frame)?;
                        self.state.blocks.push(Block::Image(image));
                    } else {
                        let list = active.map(|(id, numbered)| ListMarker {
                            id,
                            numbered,
                            level,
                        });
                        let p = self.para(n, list)?;
                        self.state.blocks.push(Block::Paragraph(p));
                    }
                }
                "list" => {
                    let name = attr(n, T, "style-name").unwrap_or("");
                    let actual_level = if active.is_some() {
                        level.checked_add(1).ok_or(Error::Limit)?
                    } else {
                        level
                    };
                    let numbered = self
                        .styles
                        .list_types
                        .get(&(name.into(), actual_level + 1))
                        .copied()
                        .or(active.map(|v| v.1))
                        .unwrap_or(false);
                    let id = self.id();
                    for item in n.children().filter(|n| n.is_element()) {
                        if !item.has_tag_name((T, "list-item")) {
                            return Err(unsupported("ODT list item"));
                        }
                        self.blocks(item, actual_level, Some((id, numbered)))?;
                    }
                }
                "table-of-content" if n.tag_name().namespace() == Some(T) => {
                    let source = n
                        .children()
                        .find(|c| c.has_tag_name((T, "table-of-content-source")))
                        .ok_or_else(|| bad("ODT index source"))?;
                    if n.attributes().any(|a| {
                        a.namespace() != Some(T)
                            || !match a.name() {
                                "name" | "style-name" => true,
                                "protected" => matches!(a.value(), "true" | "false"),
                                _ => false,
                            }
                    }) || source.attributes().any(|a| {
                        a.namespace() != Some(T)
                            || !match a.name() {
                                "outline-level" => {
                                    a.value().parse::<u8>().is_ok_and(|n| (1..=10).contains(&n))
                                }
                                "index-scope" => a.value() == "document",
                                "use-outline-level" => a.value() == "true",
                                "use-index-marks" | "use-index-source-styles" => {
                                    a.value() == "false"
                                }
                                _ => false,
                            }
                    }) {
                        return Err(unsupported("ODT index options"));
                    }
                    let max_level = attr(source, T, "outline-level")
                        .unwrap_or("10")
                        .parse::<u8>()
                        .map_err(bad)?
                        - 1;
                    if source.children().filter(|c| c.is_element()).any(|c| {
                        !c.has_tag_name((T, "table-of-content-entry-template"))
                            && !c.has_tag_name((T, "index-title-template"))
                    }) {
                        return Err(unsupported("ODT index source options"));
                    }
                    let body = n
                        .children()
                        .find(|c| c.has_tag_name((T, "index-body")))
                        .ok_or_else(|| bad("ODT index body"))?;
                    let has_pages = source
                        .descendants()
                        .any(|c| c.has_tag_name((T, "index-entry-page-number")))
                        && (body.descendants().any(|c| c.has_tag_name((T, "tab")))
                            || body
                                .children()
                                .filter(|n| n.has_tag_name((T, "p")))
                                .any(|p| {
                                    p.descendants().filter(|n| n.has_tag_name((T, "a"))).count() > 1
                                }));
                    let mut cache = OdtTocCache {
                        title_xml: None,
                        title_paragraphs: Vec::new(),
                        index_xml: standalone_xml(n)?,
                        source_xml: standalone_xml(source)?,
                        automatic_styles: self.index_styles(n),
                        has_pages,
                        needs_refresh: false,
                    };
                    if n.children().filter(|c| c.is_element()).any(|c| {
                        !c.has_tag_name((T, "index-body"))
                            && !c.has_tag_name((T, "table-of-content-source"))
                    }) {
                        return Err(unsupported("ODT index content"));
                    }
                    let mut entries = Vec::new();
                    for item in body.children().filter(|c| c.is_element()) {
                        if item.has_tag_name((T, "index-title")) {
                            if cache.title_xml.is_some() || !entries.is_empty() {
                                return Err(unsupported("ODT index title position"));
                            }
                            cache.title_xml = Some(standalone_xml(item)?);
                            for title in item.children().filter(|c| c.is_element()) {
                                if !title.has_tag_name((T, "p")) {
                                    return Err(unsupported("ODT index title content"));
                                }
                                let marks = (
                                    self.comment_marks.len(),
                                    self.bookmark_marks.len(),
                                    self.state.footnotes.len(),
                                    self.state.fields.len(),
                                );
                                let p = self.para(title, None)?;
                                if marks
                                    != (
                                        self.comment_marks.len(),
                                        self.bookmark_marks.len(),
                                        self.state.footnotes.len(),
                                        self.state.fields.len(),
                                    )
                                {
                                    return Err(unsupported("annotations in ODT index title"));
                                }
                                cache.title_paragraphs.push(p.id);
                                self.state.blocks.push(Block::Paragraph(p));
                            }
                            if cache.title_paragraphs.is_empty() {
                                return Err(unsupported("empty ODT index title"));
                            }
                            continue;
                        }
                        if !item.has_tag_name((T, "p")) {
                            return Err(unsupported("ODT index entry"));
                        }
                        let p = self.para(item, None)?;
                        let target = p
                            .runs
                            .iter()
                            .filter_map(|r| r.link.as_deref())
                            .next()
                            .and_then(|v| v.strip_prefix('#'))
                            .ok_or_else(|| unsupported("ODT unlinked index entry"))?
                            .to_owned();
                        if p.runs
                            .iter()
                            .filter_map(|r| r.link.as_deref())
                            .any(|link| link != format!("#{target}"))
                        {
                            return Err(unsupported("ODT index entry with multiple targets"));
                        }
                        entries.push((p.id, target));
                        self.state.blocks.push(Block::Paragraph(p));
                    }
                    if entries.is_empty() {
                        return Err(unsupported("empty ODT index"));
                    }
                    self.pending_tocs.push(PendingToc {
                        entries,
                        cache,
                        max_level,
                    });
                }
                "sequence-decls" if n.tag_name().namespace() == Some(T) => {}
                "table" if n.tag_name().namespace() == Some(TB) => {
                    let table = self.table(n)?;
                    self.state.blocks.push(Block::Table(table));
                }
                _ => {
                    return Err(unsupported(format!(
                        "ODT content element {}",
                        n.tag_name().name()
                    )))
                }
            }
        }
        Ok(())
    }
    fn image(&mut self, frame: roxmltree::Node<'_, '_>) -> Result<Image, Error> {
        let image = frame
            .children()
            .find(|c| c.has_tag_name((D, "image")))
            .ok_or_else(|| unsupported("ODT frame"))?;
        let href = attr(image, XL, "href")
            .ok_or_else(|| bad("ODT image href"))?
            .trim_start_matches("./");
        if href.starts_with('/') || href.contains(':') || href.split('/').any(|x| x == "..") {
            return Err(unsupported("external ODT image"));
        }
        let data = self
            .parts
            .get(href)
            .ok_or_else(|| bad("missing ODT image"))?
            .clone();
        let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else {
            return Err(unsupported("ODT image format"));
        };
        let width = nonnegative(attr(frame, SVG, "width").ok_or_else(|| bad("ODT image width"))?)?;
        let height =
            nonnegative(attr(frame, SVG, "height").ok_or_else(|| bad("ODT image height"))?)?;
        let alt = frame
            .children()
            .find(|n| n.has_tag_name((SVG, "desc")))
            .and_then(|n| n.text())
            .unwrap_or("")
            .to_owned();
        Ok(Image {
            id: self.id(),
            mime: mime.into(),
            data,
            width_twips: width,
            height_twips: height,
            alt,
        })
    }
    fn table(&mut self, n: roxmltree::Node<'_, '_>) -> Result<Table, Error> {
        let mut widths = Vec::new();
        for column in n
            .children()
            .filter(|c| c.has_tag_name((TB, "table-column")))
        {
            let count = attr(column, TB, "number-columns-repeated")
                .unwrap_or("1")
                .parse::<usize>()
                .map_err(bad)?;
            if count == 0 || count > 256 || widths.len() + count > 256 {
                return Err(Error::Limit);
            }
            let width = if let Some(name) = attr(column, TB, "style-name") {
                if self.styles.unsupported_column_styles.contains(name) {
                    return Err(unsupported("ODT table column style"));
                }
                *self
                    .styles
                    .column_widths
                    .get(name)
                    .ok_or_else(|| unsupported("unknown ODT table column style"))?
            } else {
                1440
            };
            widths.extend(std::iter::repeat_n(width, count));
        }
        let mut rows = Vec::new();
        let mut pending: Vec<u16> = Vec::new();
        for r in n.children().filter(|c| c.has_tag_name((TB, "table-row"))) {
            let repeat = attr(r, TB, "number-rows-repeated")
                .unwrap_or("1")
                .parse::<usize>()
                .map_err(bad)?;
            if repeat != 1 {
                return Err(unsupported("repeated ODT table rows"));
            }
            let mut cells = Vec::new();
            let mut covered = 0u16;
            for c in r.children().filter(|c| c.is_element()) {
                if c.has_tag_name((TB, "covered-table-cell")) {
                    if c.attributes().len() != 0
                        || c.children().any(|n| {
                            n.is_element() || n.text().is_some_and(|t| !t.trim().is_empty())
                        })
                    {
                        return Err(unsupported("ODT covered table cell content"));
                    }
                    if covered > 0 {
                        covered -= 1;
                    } else if pending.get(cells.len()).copied().unwrap_or(0) > 0 {
                        pending[cells.len()] -= 1;
                    } else {
                        return Err(unsupported("unexpected ODT covered cell"));
                    }
                    let id = self.id();
                    cells.push(Cell {
                        paragraphs: vec![Paragraph {
                            id,
                            style: ParagraphStyle::default(),
                            runs: vec![Run {
                                text: String::new(),
                                style: TextStyle::default(),
                                link: None,
                            }],
                        }],
                        row_span: 1,
                        col_span: 1,
                        border: None,
                        fill: None,
                    });
                    continue;
                }
                if covered != 0
                    || pending.get(cells.len()).copied().unwrap_or(0) > 0
                    || !c.has_tag_name((TB, "table-cell"))
                {
                    return Err(unsupported("ODT covered table cell"));
                }
                let row_span = attr(c, TB, "number-rows-spanned")
                    .unwrap_or("1")
                    .parse::<u16>()
                    .map_err(bad)?;
                if row_span == 0 || row_span > 256 {
                    return Err(bad("ODT table row span"));
                }
                let span = attr(c, TB, "number-columns-spanned")
                    .unwrap_or("1")
                    .parse::<u16>()
                    .map_err(bad)?;
                if span == 0 || span > 256 {
                    return Err(bad("ODT table span"));
                }
                covered = span - 1;
                let start = cells.len();
                let end = start.checked_add(span as usize).ok_or(Error::Limit)?;
                if end > 256 {
                    return Err(Error::Limit);
                }
                if pending.len() < end {
                    pending.resize(end, 0);
                }
                if pending[start..end].iter().any(|n| *n != 0) {
                    return Err(bad("overlapping ODT table spans"));
                }
                for entry in &mut pending[start..end] {
                    *entry = row_span - 1;
                }
                let repeats = attr(c, TB, "number-columns-repeated")
                    .unwrap_or("1")
                    .parse::<usize>()
                    .map_err(bad)?;
                if repeats != 1 {
                    return Err(unsupported("repeated ODT table cells"));
                }
                let fill = if let Some(name) = attr(c, TB, "style-name") {
                    if self.styles.unsupported_cell_styles.contains(name) {
                        return Err(unsupported("ODT table cell style"));
                    }
                    *self
                        .styles
                        .cell_fills
                        .get(name)
                        .ok_or_else(|| unsupported("unknown ODT table cell style"))?
                } else {
                    None
                };
                let border = if let Some(name) = attr(c, TB, "style-name") {
                    self.styles
                        .cell_borders
                        .get(name)
                        .ok_or_else(|| unsupported("unknown ODT table cell style"))?
                        .clone()
                } else {
                    None
                };
                let mut paragraphs = Vec::new();
                for p in c.children().filter(|n| n.is_element()) {
                    if !p.has_tag_name((T, "p")) {
                        return Err(unsupported("ODT table cell content"));
                    }
                    paragraphs.push(self.para(p, None)?)
                }
                cells.push(Cell {
                    paragraphs,
                    row_span,
                    col_span: span,
                    border,
                    fill,
                });
            }
            if covered != 0 {
                return Err(bad("missing ODT covered cells"));
            }
            if cells.is_empty()
                || rows
                    .first()
                    .is_some_and(|x: &Vec<Cell>| x.len() != cells.len())
            {
                return Err(unsupported("irregular ODT table"));
            }
            rows.push(cells);
        }
        if pending.iter().any(|n| *n != 0) {
            return Err(bad("ODT row span exceeds table"));
        }
        if rows.is_empty() {
            return Err(unsupported("empty ODT table"));
        }
        if !widths.is_empty() && rows.iter().any(|row| row.len() != widths.len()) {
            return Err(unsupported("ODT table columns do not match cells"));
        }
        Ok(Table {
            id: self.id(),
            rows,
            column_widths_twips: widths,
        })
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<State, Error> {
    let mut parts = read_parts(bytes)?;
    let content = parts
        .remove("content.xml")
        .ok_or_else(|| bad("missing ODT content"))?;
    let styles_bytes = parts
        .get("styles.xml")
        .ok_or_else(|| bad("missing ODT styles"))?;
    let content = std::str::from_utf8(&content).map_err(bad)?;
    let styles_str = std::str::from_utf8(styles_bytes).map_err(bad)?;
    let content_doc = roxmltree::Document::parse(content).map_err(bad)?;
    let styles_doc = roxmltree::Document::parse(styles_str).map_err(bad)?;
    let style_map = Styles::new(&[&styles_doc, &content_doc])?;
    let master = styles_doc
        .descendants()
        .find(|n| n.has_tag_name((S, "master-page")) && attr(*n, S, "name") == Some("Standard"))
        .or_else(|| {
            styles_doc
                .descendants()
                .find(|n| n.has_tag_name((S, "master-page")))
        });
    let layout_name = master.and_then(|n| attr(n, S, "page-layout-name"));
    let layout = styles_doc.descendants().find(|n| {
        n.has_tag_name((S, "page-layout"))
            && (layout_name.is_none() || attr(*n, S, "name") == layout_name)
    });
    let mut page = PageSettings::default();
    if let Some(p) = layout.and_then(|n| {
        n.children()
            .find(|c| c.has_tag_name((S, "page-layout-properties")))
    }) {
        if let Some(v) = attr(p, F, "margin") {
            let v = nonnegative(v)?;
            page.margin_left = v;
            page.margin_right = v;
            page.margin_top = v;
            page.margin_bottom = v;
        }
        for (name, target) in [
            ("page-width", &mut page.width_twips),
            ("page-height", &mut page.height_twips),
            ("margin-left", &mut page.margin_left),
            ("margin-right", &mut page.margin_right),
            ("margin-top", &mut page.margin_top),
            ("margin-bottom", &mut page.margin_bottom),
        ] {
            if let Some(v) = attr(p, F, name) {
                *target = nonnegative(v)?;
            }
        }
    }
    let body = content_doc
        .descendants()
        .find(|n| n.has_tag_name((O, "text")))
        .ok_or_else(|| bad("missing ODT text"))?;
    let mut r = Reader {
        styles: style_map,
        parts: &parts,
        state: State::default(),
        next: 1,
        pending_fields: vec![],
        pending_comments: vec![],
        pending_notes: vec![],
        inside_note: false,
        comment_marks: vec![],
        comment_data: BTreeMap::new(),
        point_counter: 0,
        bookmarks: BTreeMap::new(),
        pending_bookmarks: vec![],
        bookmark_marks: vec![],
        pending_tocs: vec![],
        source_automatic_styles: {
            let mut map = BTreeMap::new();
            if let Some(node) = content_doc
                .descendants()
                .find(|n| n.has_tag_name((O, "automatic-styles")))
            {
                for child in node.children().filter(|n| n.is_element()) {
                    if let Some(name) = attr(child, S, "name") {
                        let dependencies = child
                            .descendants()
                            .filter(|n| n.is_element())
                            .flat_map(|n| n.attributes())
                            .filter(|a| a.name().ends_with("style-name"))
                            .map(|a| a.value().to_owned())
                            .collect();
                        if map
                            .insert(name.to_owned(), (standalone_xml(child)?, dependencies))
                            .is_some()
                        {
                            return Err(bad("duplicate ODT automatic style"));
                        }
                    }
                }
            }
            map
        },
    };
    r.state.blocks.clear();
    r.blocks(body, 0, None)?;
    for pending in std::mem::take(&mut r.pending_tocs) {
        let id = r.id();
        let mut entries = Vec::new();
        for (paragraph, bookmark) in pending.entries {
            let heading = *r
                .bookmarks
                .get(&bookmark)
                .ok_or_else(|| unsupported("unresolved ODT index link"))?;
            let p =
                paragraph_mut(&mut r.state, paragraph).ok_or_else(|| bad("ODT index paragraph"))?;
            for run in &mut p.runs {
                run.link = Some(format!("#DOC42_H{heading}"));
            }
            entries.push(TocEntry { paragraph, heading });
        }
        r.state.tables_of_contents.push(TableOfContents {
            id,
            entries,
            min_level: 0,
            max_level: pending.max_level,
            docx_cached: None,
            odt_cached: Some(pending.cache),
        });
    }
    if r.state.blocks.is_empty() {
        return Err(bad("empty ODT document"));
    }
    if let Some(master) = master {
        let mut header = Vec::new();
        let mut footer = Vec::new();
        for section in master
            .children()
            .filter(|n| n.is_element() && matches!(n.tag_name().name(), "header" | "footer"))
        {
            let target = if section.has_tag_name((S, "header")) {
                &mut header
            } else {
                &mut footer
            };
            for p in section.children().filter(|n| n.is_element()) {
                if !p.has_tag_name((T, "p")) {
                    return Err(unsupported("ODT header content"));
                }
                target.push(r.para(p, None)?);
            }
        }
        r.state.sections[0].header = header.clone();
        r.state.sections[0].footer = footer.clone();
        let header_ids: HashSet<_> = header.iter().chain(&footer).map(|p| p.id).collect();
        r.state.odt_source_header_fields = Some(
            r.state
                .fields
                .iter()
                .filter(|f| header_ids.contains(&f.at.paragraph))
                .cloned()
                .collect(),
        );
        r.state.odt_source_headers = Some((header, footer));
    }
    let mut grouped: BTreeMap<String, (Option<Position>, Option<Position>)> = BTreeMap::new();
    for (paragraph, offset, name, start) in std::mem::take(&mut r.comment_marks) {
        let pair = grouped.entry(name).or_default();
        let slot = if start { &mut pair.0 } else { &mut pair.1 };
        if slot.replace(Position { paragraph, offset }).is_some() {
            return Err(bad("duplicate ODT comment marker"));
        }
    }
    for (name, (start, end)) in grouped {
        let item = r
            .comment_data
            .remove(&name)
            .ok_or_else(|| bad("ODT comment without content"))?;
        let start = start.ok_or_else(|| bad("ODT comment start"))?;
        let end = end.ok_or_else(|| bad("ODT comment end"))?;
        let id = r.id();
        r.state.comments.push(Annotation {
            id,
            start,
            end,
            author: item.author,
            text: item.text,
            resolved: false,
            date: item.date,
            initials: None,
        });
    }
    if !r.comment_data.is_empty() {
        return Err(unsupported("unanchored ODT comment"));
    }
    let mut ranges: BTreeMap<String, (Option<Position>, Option<Position>)> = BTreeMap::new();
    for (position, name, kind) in std::mem::take(&mut r.bookmark_marks) {
        let pair = ranges.entry(name).or_default();
        match kind {
            0 if pair.0.is_none() && pair.1.is_none() => {
                pair.0 = Some(position.clone());
                pair.1 = Some(position);
            }
            1 if pair.0.is_none() => pair.0 = Some(position),
            2 if pair.1.is_none() => pair.1 = Some(position),
            _ => return Err(bad("duplicate ODT bookmark marker")),
        }
    }
    for (name, (start, end)) in ranges {
        let id = r.id();
        r.state.bookmarks.push(Bookmark {
            id,
            name,
            start: start.ok_or_else(|| bad("ODT bookmark start"))?,
            end: end.ok_or_else(|| bad("ODT bookmark end"))?,
        });
    }
    r.state.next_id = r.next;
    r.state.sections[0].page = page.clone();
    r.state.odt_source_page = Some(page);
    let mut state = r.state;
    state.odt_parts = parts;
    Ok(state)
}
struct Writer {
    automatic: String,
    next: usize,
    pictures: Vec<(String, String, Vec<u8>)>,
    used: HashSet<String>,
    fields: Vec<Field>,
    comments: Vec<Annotation>,
    notes: Vec<Footnote>,
    bookmarks: Vec<Bookmark>,
}
impl Writer {
    fn name(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("DOC42{prefix}{}", self.next)
    }
    fn paragraph(&mut self, p: &Paragraph) -> Result<String, Error> {
        let name = self.name("P");
        let align = match p.style.alignment {
            Alignment::Left => "left",
            Alignment::Center => "center",
            Alignment::Right => "right",
            Alignment::Justify => "justify",
        };
        let parent = p
            .style
            .source_style
            .as_ref()
            .map(|s| format!(" style:parent-style-name=\"{}\"", esc(s)))
            .unwrap_or_default();
        let keep = if p.style.keep_with_next {
            " fo:keep-with-next=\"always\""
        } else {
            ""
        };
        let together = if p.style.keep_lines {
            " fo:keep-together=\"always\""
        } else {
            ""
        };
        let pagebreak = if p.style.page_break_before {
            " fo:break-before=\"page\""
        } else {
            ""
        };
        let line = if p.style.line_twips == 0 {
            String::new()
        } else {
            format!(" fo:line-height=\"{}pt\"", p.style.line_twips as f64 / 20.0)
        };
        let extra = extra_attrs(&p.style.odt_extra)?;
        let tabs = if p.style.tabs_twips.is_empty() {
            String::new()
        } else {
            format!(
                "<style:tab-stops>{}</style:tab-stops>",
                p.style
                    .tabs_twips
                    .iter()
                    .map(|n| format!(
                        "<style:tab-stop style:position=\"{}pt\" style:type=\"left\"/>",
                        *n as f64 / 20.0
                    ))
                    .collect::<String>()
            )
        };
        self.automatic.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"paragraph\"{parent}><style:paragraph-properties fo:text-align=\"{align}\" fo:margin-left=\"{}pt\" fo:margin-right=\"{}pt\" fo:text-indent=\"{}pt\" fo:margin-top=\"{}pt\" fo:margin-bottom=\"{}pt\"{line}{keep}{together}{pagebreak}{extra}>{tabs}</style:paragraph-properties></style:style>",p.style.left_twips as f64/20.0,p.style.right_twips as f64/20.0,p.style.first_line_twips as f64/20.0,p.style.before_twips as f64/20.0,p.style.after_twips as f64/20.0));
        let (tag, outline) = if let Some(l) = p.style.outline_level {
            if l >= 10 {
                return Err(unsupported("ODT outline level"));
            }
            ("h", format!(" text:outline-level=\"{}\"", l + 1))
        } else {
            ("p", String::new())
        };
        let mut out = format!("<text:{tag} text:style-name=\"{name}\"{outline}>");
        #[derive(Clone)]
        enum Marker {
            Field(FieldKind),
            Start(Annotation),
            End(u64),
            Note(String),
            BookmarkStart(String),
            BookmarkEnd(String),
            BookmarkPoint(String),
        }
        let mut markers: Vec<(usize, u8, Marker)> = self
            .fields
            .iter()
            .filter(|f| f.at.paragraph == p.id)
            .map(|f| (f.at.offset, 1, Marker::Field(f.kind.clone())))
            .collect();
        for comment in &self.comments {
            if comment.start.paragraph == p.id {
                markers.push((comment.start.offset, 0, Marker::Start(comment.clone())));
            }
            if comment.end.paragraph == p.id {
                markers.push((comment.end.offset, 2, Marker::End(comment.id)));
            }
        }
        for bookmark in &self.bookmarks {
            if bookmark.start == bookmark.end {
                if bookmark.start.paragraph == p.id {
                    markers.push((
                        bookmark.start.offset,
                        1,
                        Marker::BookmarkPoint(bookmark.name.clone()),
                    ));
                }
            } else {
                if bookmark.start.paragraph == p.id {
                    markers.push((
                        bookmark.start.offset,
                        0,
                        Marker::BookmarkStart(bookmark.name.clone()),
                    ));
                }
                if bookmark.end.paragraph == p.id {
                    markers.push((
                        bookmark.end.offset,
                        2,
                        Marker::BookmarkEnd(bookmark.name.clone()),
                    ));
                }
            }
        }
        let notes = self
            .notes
            .iter()
            .filter(|n| n.at.paragraph == p.id)
            .cloned()
            .collect::<Vec<_>>();
        for note in notes {
            let number = self
                .notes
                .iter()
                .filter(|n| n.endnote == note.endnote)
                .position(|n| n.id == note.id)
                .unwrap_or(0)
                + 1;
            let label = note.citation.clone().unwrap_or_else(|| number.to_string());
            let mut body = String::new();
            for paragraph in &note.paragraphs {
                body.push_str(&self.paragraph(paragraph)?);
            }
            let class = if note.endnote { "endnote" } else { "footnote" };
            let markup = format!("<text:note text:id=\"DOC42N{}\" text:note-class=\"{class}\"><text:note-citation>{}</text:note-citation><text:note-body>{body}</text:note-body></text:note>", note.id, esc(&label));
            markers.push((note.at.offset, 1, Marker::Note(markup)));
        }
        markers.sort_by_key(|(offset, priority, _)| (*offset, *priority));
        let emit = |marker: &Marker| -> Result<String, Error> {
            Ok(match marker {
                Marker::Field(kind) => odt_field(kind)?.to_owned(),
                Marker::Start(comment) => {
                    if comment
                        .text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        return Err(unsupported("ODT comment text"));
                    }
                    let date = comment
                        .date
                        .as_ref()
                        .map(|d| format!("<dc:date xmlns:dc=\"{DC}\">{}</dc:date>", esc(d)))
                        .unwrap_or_default();
                    let text = comment
                        .text
                        .split('\n')
                        .map(|p| format!("<text:p>{}</text:p>", odt_text(p.trim_end_matches('\r'))))
                        .collect::<String>();
                    format!("<office:annotation office:name=\"DOC42C{}\"><dc:creator xmlns:dc=\"{DC}\">{}</dc:creator>{date}{text}</office:annotation>", comment.id, esc(&comment.author))
                }
                Marker::End(id) => format!("<office:annotation-end office:name=\"DOC42C{id}\"/>"),
                Marker::Note(markup) => markup.clone(),
                Marker::BookmarkStart(name) => {
                    format!("<text:bookmark-start text:name=\"{}\"/>", esc(name))
                }
                Marker::BookmarkEnd(name) => {
                    format!("<text:bookmark-end text:name=\"{}\"/>", esc(name))
                }
                Marker::BookmarkPoint(name) => {
                    format!("<text:bookmark text:name=\"{}\"/>", esc(name))
                }
            })
        };
        let mut marker_index = 0;
        let mut base = 0usize;
        for run in &p.runs {
            if run.style.theme_color.is_some() {
                return Err(unsupported("ODT theme color"));
            }
            let style = self.name("T");
            let t = &run.style;
            let parent = t
                .source_style
                .as_ref()
                .map(|s| format!(" style:parent-style-name=\"{}\"", esc(s)))
                .unwrap_or_default();
            let c = t
                .color
                .map(|c| format!(" fo:color=\"#{:06x}\"", c & 0xffffff))
                .unwrap_or_default();
            let h = t
                .highlight
                .map(|c| format!(" fo:background-color=\"#{:06x}\"", c & 0xffffff))
                .unwrap_or_default();
            let extra = extra_attrs(&t.odt_extra)?;
            self.automatic.push_str(&format!("<style:style style:name=\"{style}\" style:family=\"text\"{parent}><style:text-properties fo:font-family=\"{}\" fo:font-size=\"{}pt\" fo:font-weight=\"{}\" fo:font-style=\"{}\" style:text-underline-style=\"{}\" style:text-line-through-style=\"{}\"{c}{h}{extra}/></style:style>",esc(&t.font),t.size_half_points as f64/2.0,if t.bold{"bold"}else{"normal"},if t.italic{"italic"}else{"normal"},if t.underline{"solid"}else{"none"},if t.strike{"solid"}else{"none"}));
            let end = base + run.text.len();
            let mut cursor = base;
            let mut inner = String::new();
            while marker_index < markers.len() && markers[marker_index].0 <= end {
                let marker = &markers[marker_index];
                let piece = run
                    .text
                    .get(cursor - base..marker.0 - base)
                    .ok_or_else(|| bad("ODT field boundary"))?;
                inner.push_str(&odt_text(piece));
                inner.push_str(&emit(&marker.2)?);
                cursor = marker.0;
                marker_index += 1;
            }
            inner.push_str(&odt_text(
                run.text
                    .get(cursor - base..)
                    .ok_or_else(|| bad("ODT field boundary"))?,
            ));
            base = end;
            let content = format!("<text:span text:style-name=\"{style}\">{inner}</text:span>");
            if let Some(link) = &run.link {
                out.push_str(&format!(
                    "<text:a xlink:href=\"{}\">{content}</text:a>",
                    esc(link)
                ))
            } else {
                out.push_str(&content)
            }
        }
        while marker_index < markers.len() {
            if markers[marker_index].0 != base {
                return Err(bad("ODT anchor position"));
            }
            out.push_str(&emit(&markers[marker_index].2)?);
            marker_index += 1;
        }
        out.push_str(&format!("</text:{tag}>"));
        Ok(out)
    }
    fn image(&mut self, i: &Image) -> Result<String, Error> {
        let ext = match i.mime.as_str() {
            "image/png" => "png",
            "image/jpeg" => "jpg",
            _ => return Err(unsupported("ODT image format")),
        };
        let mut n = 0;
        let path = loop {
            let candidate = format!("Pictures/DOC42-{}-{n}.{ext}", i.id);
            if self.used.insert(candidate.clone()) {
                break candidate;
            }
            n += 1;
        };
        self.pictures
            .push((path.clone(), i.mime.clone(), i.data.clone()));
        Ok(format!("<text:p><draw:frame text:anchor-type=\"paragraph\" svg:width=\"{}pt\" svg:height=\"{}pt\"><draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/><svg:desc>{}</svg:desc></draw:frame></text:p>",i.width_twips as f64/20.0,i.height_twips as f64/20.0,esc(&path),esc(&i.alt)))
    }
    fn table(&mut self, t: &Table) -> Result<String, Error> {
        let mut out = String::from("<table:table>");
        for width in &t.column_widths_twips {
            let name = self.name("W");
            self.automatic.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"table-column\"><style:table-column-properties style:column-width=\"{}pt\"/></style:style>", *width as f64 / 20.0));
            out.push_str(&format!(
                "<table:table-column table:style-name=\"{name}\"/>"
            ));
        }
        let layout = super::table_edit::table_layout(t)?;
        for (row_index, row) in t.rows.iter().enumerate() {
            out.push_str("<table:table-row>");
            for (index, cell) in row.iter().enumerate() {
                if layout[row_index][index] != (row_index, index) {
                    out.push_str("<table:covered-table-cell/>");
                    continue;
                }
                if cell.paragraphs.is_empty() {
                    return Err(unsupported("ODT merged or styled table cells"));
                }
                let mut attributes = String::new();
                if cell.col_span > 1 {
                    attributes.push_str(&format!(
                        " table:number-columns-spanned=\"{}\"",
                        cell.col_span
                    ));
                }
                if cell.row_span > 1 {
                    attributes
                        .push_str(&format!(" table:number-rows-spanned=\"{}\"", cell.row_span));
                }
                if cell.fill.is_some() || cell.border.is_some() {
                    let name = self.name("C");
                    let mut properties = String::new();
                    if let Some(fill) = cell.fill {
                        if fill > 0xffffff {
                            return Err(bad("invalid table cell background"));
                        }
                        properties.push_str(&format!(" fo:background-color=\"#{fill:06x}\""));
                    }
                    if let Some(border) = &cell.border {
                        if border.width_twips == 0 || border.color > 0xffffff {
                            return Err(bad("invalid cell border"));
                        }
                        properties.push_str(&format!(
                            " fo:border=\"{}pt solid #{:06x}\"",
                            border.width_twips as f64 / 20.0,
                            border.color
                        ));
                    }
                    self.automatic.push_str(&format!("<style:style style:name=\"{name}\" style:family=\"table-cell\"><style:table-cell-properties{properties}/></style:style>"));
                    attributes.push_str(&format!(" table:style-name=\"{name}\""));
                }
                out.push_str(&format!("<table:table-cell{attributes}>"));
                for p in &cell.paragraphs {
                    if p.style.list.is_some() {
                        return Err(unsupported("ODT list inside table"));
                    }
                    out.push_str(&self.paragraph(p)?);
                }
                out.push_str("</table:table-cell>");
            }
            out.push_str("</table:table-row>");
        }
        out.push_str("</table:table>");
        Ok(out)
    }
}
fn odt_field(kind: &FieldKind) -> Result<&'static str, Error> {
    match kind {
        FieldKind::PageNumber => {
            Ok("<text:page-number text:select-page=\"current\">1</text:page-number>")
        }
        FieldKind::PageCount => Ok("<text:page-count>1</text:page-count>"),
        _ => Err(unsupported("ODT field")),
    }
}
fn odt_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            ' ' => out.push_str("<text:s/>"),
            '\t' => out.push_str("<text:tab/>"),
            _ => out.push_str(&esc(&c.to_string())),
        }
    }
    out
}
fn close_lists(out: &mut String, stack: &mut Vec<(u64, bool)>) {
    while stack.pop().is_some() {
        out.push_str("</text:list-item></text:list>")
    }
}
fn xml_insert(xml: &mut String, addition: &str) -> Result<(), Error> {
    let i = xml.rfind("</").ok_or_else(|| bad("invalid ODT manifest"))?;
    xml.insert_str(i, addition);
    Ok(())
}
fn opening_tag_end(xml: &str, start: usize) -> Result<usize, Error> {
    let mut quote = None;
    for (offset, byte) in xml.as_bytes()[start..].iter().copied().enumerate() {
        match (quote, byte) {
            (Some(q), c) if q == c => quote = None,
            (None, b'\'' | b'"') => quote = Some(byte),
            (None, b'>') => return Ok(start + offset),
            _ => {}
        }
    }
    Err(bad("invalid ODT element"))
}
// Apply narrowly scoped XML edits so unrelated ODT package styles survive.
fn overlay_styles(
    xml: &str,
    state: &State,
    writer: &mut Writer,
    page_changed: bool,
    headers_changed: bool,
) -> Result<String, Error> {
    let doc = roxmltree::Document::parse(xml).map_err(bad)?;
    let master = doc
        .descendants()
        .find(|n| n.has_tag_name((S, "master-page")) && attr(*n, S, "name") == Some("Standard"))
        .or_else(|| {
            doc.descendants()
                .find(|n| n.has_tag_name((S, "master-page")))
        })
        .ok_or_else(|| unsupported("ODT master page missing"))?;
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    if page_changed {
        let layout_name = attr(master, S, "page-layout-name");
        let layout = doc
            .descendants()
            .find(|n| {
                n.has_tag_name((S, "page-layout"))
                    && (layout_name.is_none() || attr(*n, S, "name") == layout_name)
            })
            .and_then(|n| {
                n.children()
                    .find(|c| c.has_tag_name((S, "page-layout-properties")))
            })
            .ok_or_else(|| unsupported("ODT page layout missing"))?;
        let p = &state.sections[0].page;
        let values = [
            ("page-width", p.width_twips),
            ("page-height", p.height_twips),
            ("margin-left", p.margin_left),
            ("margin-right", p.margin_right),
            ("margin-top", p.margin_top),
            ("margin-bottom", p.margin_bottom),
        ];
        let mut prefix = String::from("doc42fo");
        while xml.contains(&format!("xmlns:{prefix}")) {
            prefix.push('_');
        }
        let mut missing = String::new();
        for (name, value) in values {
            let value = format!("{}pt", value as f64 / 20.0);
            if let Some(a) = layout
                .attributes()
                .find(|a| a.namespace() == Some(F) && a.name() == name)
            {
                edits.push((a.range_value(), value));
            } else {
                missing.push_str(&format!(" {prefix}:{name}=\"{value}\""));
            }
        }
        if !missing.is_empty() {
            let start = layout.range().start;
            let end = opening_tag_end(xml, start)?;
            let at = if xml.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
                end - 1
            } else {
                end
            };
            edits.push((at..at, format!(" xmlns:{prefix}=\"{F}\"{missing}")));
        }
    }
    if headers_changed {
        let auto_start = writer.automatic.len();
        let mut fragments = Vec::new();
        for (tag, paragraphs) in [
            ("header", &state.sections[0].header),
            ("footer", &state.sections[0].footer),
        ] {
            let mut text = String::new();
            if !paragraphs.is_empty() {
                text.push_str(&format!("<style:{tag} {NS}>"));
                for p in paragraphs {
                    text.push_str(&writer.paragraph(p)?);
                }
                text.push_str(&format!("</style:{tag}>"));
            }
            fragments.push((tag, text));
        }
        let styles = &writer.automatic[auto_start..];
        let styles = styles.replace("<style:style ", &format!("<style:style {NS} "));
        let automatic = doc
            .descendants()
            .find(|n| n.has_tag_name((O, "automatic-styles")))
            .ok_or_else(|| unsupported("ODT automatic styles missing"))?;
        insert_child(xml, automatic, &styles, &mut edits)?;
        let mut additions = String::new();
        for (tag, text) in fragments {
            if let Some(existing) = master.children().find(|n| n.has_tag_name((S, tag))) {
                if existing.attributes().len() != 0 {
                    return Err(unsupported(
                        "editing ODT header/footer with container attributes",
                    ));
                }
                edits.push((existing.range(), text));
            } else {
                additions.push_str(&text);
            }
        }
        if !additions.is_empty() {
            insert_child(xml, master, &additions, &mut edits)?;
        }
    }
    edits.sort_by(|a, b| b.0.start.cmp(&a.0.start));
    let mut out = xml.to_owned();
    for (range, value) in edits {
        out.replace_range(range, &value);
    }
    Ok(out)
}
fn insert_child(
    xml: &str,
    node: roxmltree::Node<'_, '_>,
    text: &str,
    edits: &mut Vec<(std::ops::Range<usize>, String)>,
) -> Result<(), Error> {
    if text.is_empty() {
        return Ok(());
    }
    let range = node.range();
    let source = &xml[range.clone()];
    if source.ends_with("/>") {
        let tag_end = source
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .ok_or_else(|| bad("invalid ODT element"))?;
        let tag = &source[1..tag_end];
        edits.push((
            range,
            format!("{}>{text}</{tag}>", &source[..source.len() - 2]),
        ));
    } else {
        let at = range.start
            + source
                .rfind("</")
                .ok_or_else(|| bad("invalid ODT element"))?;
        edits.push((at..at, text.into()));
    }
    Ok(())
}
pub(super) fn encode(s: &State) -> Result<Vec<u8>, Error> {
    if s.sections.len() != 1 || s.sections[0].page.columns != 1 || !s.revisions.is_empty() {
        return Err(unsupported("ODT sections, columns or review content"));
    }
    let body_ids: HashSet<_> = s
        .blocks
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph(p) => vec![p.id],
            Block::Table(t) => t
                .rows
                .iter()
                .flatten()
                .flat_map(|c| c.paragraphs.iter().map(|p| p.id))
                .collect(),
            _ => vec![],
        })
        .collect();
    if s.footnotes
        .iter()
        .any(|n| !body_ids.contains(&n.at.paragraph))
    {
        return Err(unsupported("ODT note anchors outside body"));
    }
    if s.comments.iter().any(|c| {
        !body_ids.contains(&c.start.paragraph)
            || !body_ids.contains(&c.end.paragraph)
            || c.resolved
            || c.initials.is_some()
    }) {
        return Err(unsupported("ODT comment metadata or anchors"));
    }
    let header_ids: HashSet<_> = s.sections[0]
        .header
        .iter()
        .chain(&s.sections[0].footer)
        .map(|p| p.id)
        .collect();
    if s.fields.iter().any(|f| {
        !header_ids.contains(&f.at.paragraph)
            || !matches!(f.kind, FieldKind::PageNumber | FieldKind::PageCount)
    }) {
        return Err(unsupported("ODT body or custom fields"));
    }
    if s.odt_source_page.is_none()
        && (s.sections[0].page.header_distance_twips
            != PageSettings::default().header_distance_twips
            || s.sections[0].page.footer_distance_twips
                != PageSettings::default().footer_distance_twips)
    {
        return Err(unsupported("ODT header/footer distance"));
    }
    if let Some(original) = &s.odt_source_page {
        let current = &s.sections[0].page;
        if original.header_distance_twips != current.header_distance_twips
            || original.footer_distance_twips != current.footer_distance_twips
        {
            return Err(unsupported("editing ODT header/footer distance"));
        }
    }
    if s.tables_of_contents
        .iter()
        .flat_map(|t| &t.entries)
        .any(|entry| {
            s.bookmarks.iter().any(|b| {
                b.name == format!("DOC42_H{}", entry.heading)
                    && (b.start.paragraph != entry.heading || b.start.offset != 0)
            })
        })
    {
        return Err(unsupported("ODT bookmark conflicts with TOC target"));
    }
    let mut parts = s.odt_parts.clone();
    if parts
        .keys()
        .any(|k| k.to_ascii_lowercase().contains("signature"))
    {
        return Err(unsupported("signed ODT package"));
    }
    parts.remove("mimetype");
    parts.remove("content.xml");
    let mut w = Writer {
        automatic: String::new(),
        next: 0,
        pictures: vec![],
        used: parts.keys().cloned().collect(),
        fields: s.fields.clone(),
        comments: s.comments.clone(),
        notes: s.footnotes.clone(),
        bookmarks: s.bookmarks.clone(),
    };
    if let Some(styles) = parts.get("styles.xml") {
        let xml = std::str::from_utf8(styles).map_err(bad)?;
        let doc = roxmltree::Document::parse(xml).map_err(bad)?;
        for node in doc.descendants().filter(|n| n.is_element()) {
            if let Some(number) = attr(node, S, "name")
                .and_then(|name| {
                    name.strip_prefix("DOC42P")
                        .or_else(|| name.strip_prefix("DOC42T"))
                        .or_else(|| name.strip_prefix("DOC42C"))
                        .or_else(|| name.strip_prefix("DOC42W"))
                })
                .and_then(|n| n.parse::<usize>().ok())
            {
                if number.checked_add(MAX_BYTES).is_none() {
                    return Err(Error::Limit);
                }
                w.next = w.next.max(number);
            }
        }
    }
    let mut source_by_name = BTreeMap::<String, String>::new();
    for cache in s
        .tables_of_contents
        .iter()
        .filter_map(|t| t.odt_cached.as_ref())
    {
        let wrap = format!("<root>{}</root>", cache.automatic_styles);
        let doc = roxmltree::Document::parse(&wrap).map_err(bad)?;
        for child in doc.root_element().children().filter(|n| n.is_element()) {
            let name =
                attr(child, S, "name").ok_or_else(|| unsupported("unnamed cached ODT style"))?;
            let raw = standalone_xml(child)?;
            if let Some(prior) = source_by_name.insert(name.to_owned(), raw.clone()) {
                if prior != raw {
                    return Err(unsupported("conflicting cached ODT style"));
                }
            }
        }
    }
    let mut style_names = source_by_name.keys().cloned().collect::<HashSet<_>>();
    for name in source_by_name.keys() {
        if let Some(number) = ["DOC42P", "DOC42T", "DOC42C", "DOC42W"]
            .iter()
            .find_map(|prefix| name.strip_prefix(prefix))
            .and_then(|n| n.parse::<usize>().ok())
        {
            if number.checked_add(MAX_BYTES).is_none() {
                return Err(Error::Limit);
            }
            w.next = w.next.max(number);
        }
    }
    let source_styles = source_by_name.into_values().collect::<String>();
    let mut lists = String::new();
    let mut list_names = HashMap::new();
    let mut seen = HashSet::new();
    for p in s.blocks.iter().filter_map(|b| {
        if let Block::Paragraph(p) = b {
            Some(p)
        } else {
            None
        }
    }) {
        if let Some(l) = &p.style.list {
            if l.level > 8 {
                return Err(unsupported("ODT list depth"));
            }
            if seen.insert((l.id, l.numbered)) {
                let kind = if l.numbered { "number" } else { "bullet" };
                let base = format!("DOC42L{}{}", l.id, if l.numbered { "N" } else { "B" });
                let mut name = base.clone();
                let mut suffix = 0usize;
                while style_names.contains(&name) {
                    suffix += 1;
                    name = format!("{base}_{suffix}");
                }
                style_names.insert(name.clone());
                list_names.insert((l.id, l.numbered), name.clone());
                lists.push_str(&format!("<text:list-style style:name=\"{}\">", esc(&name)));
                for level in 1..=9 {
                    let extra = if l.numbered {
                        " style:num-format=\"1\""
                    } else {
                        " text:bullet-char=\"•\""
                    };
                    lists.push_str(&format!(
                        "<text:list-level-style-{kind} text:level=\"{level}\"{extra}/>"
                    ));
                }
                lists.push_str("</text:list-style>");
            }
        }
    }
    let mut body = String::new();
    let mut stack: Vec<(u64, bool)> = Vec::new();
    for block in &s.blocks {
        if let Block::Paragraph(p) = block {
            if let Some(toc) = s.tables_of_contents.iter().find(|t| {
                t.odt_cached
                    .as_ref()
                    .is_some_and(|c| c.title_paragraphs.contains(&p.id))
            }) {
                let cache = toc.odt_cached.as_ref().unwrap();
                if s.comments
                    .iter()
                    .any(|c| c.start.paragraph == p.id || c.end.paragraph == p.id)
                    || s.footnotes.iter().any(|n| n.at.paragraph == p.id)
                    || s.bookmarks
                        .iter()
                        .any(|b| b.start.paragraph == p.id || b.end.paragraph == p.id)
                    || s.fields.iter().any(|f| f.at.paragraph == p.id)
                {
                    return Err(unsupported("annotations in ODT index title"));
                }
                if cache.title_paragraphs.first() == Some(&p.id) {
                    close_lists(&mut body, &mut stack);
                    if !cache.needs_refresh {
                        body.push_str(&cache.index_xml);
                    } else {
                        body.push_str(&format!(
                            "<text:table-of-content text:name=\"DOC42_T{}\">",
                            toc.id
                        ));
                        body.push_str(&cache.source_xml);
                        body.push_str("<text:index-body>");
                        body.push_str(
                            cache
                                .title_xml
                                .as_deref()
                                .ok_or_else(|| bad("ODT index title"))?,
                        );
                    }
                }
                continue;
            }
        }
        if let Block::Paragraph(p) = block {
            if let Some(toc) = s.tables_of_contents.iter().find(|t| {
                t.odt_cached.as_ref().is_some_and(|c| !c.needs_refresh)
                    && t.entries.iter().any(|e| e.paragraph == p.id)
            }) {
                if s.comments
                    .iter()
                    .any(|c| c.start.paragraph == p.id || c.end.paragraph == p.id)
                    || s.footnotes.iter().any(|n| n.at.paragraph == p.id)
                    || s.bookmarks
                        .iter()
                        .any(|b| b.start.paragraph == p.id || b.end.paragraph == p.id)
                    || s.fields.iter().any(|f| f.at.paragraph == p.id)
                {
                    return Err(unsupported("annotations in cached ODT index"));
                }
                if toc.entries.first().is_some_and(|e| e.paragraph == p.id)
                    && toc.odt_cached.as_ref().unwrap().title_paragraphs.is_empty()
                {
                    body.push_str(&toc.odt_cached.as_ref().unwrap().index_xml);
                }
                continue;
            }
        }
        if let Block::Paragraph(p) = block {
            if let Some(list) = &p.style.list {
                let level = list.level as usize;
                while stack.len() > level + 1 {
                    stack.pop();
                    body.push_str("</text:list-item></text:list>");
                }
                if stack.len() == level + 1 {
                    if stack[level] == (list.id, list.numbered) {
                        body.push_str("</text:list-item>")
                    } else {
                        stack.pop();
                        body.push_str("</text:list-item></text:list>");
                    }
                }
                if stack.len() < level {
                    return Err(unsupported("discontinuous ODT list depth"));
                }
                if stack.len() == level {
                    let name = list_names
                        .get(&(list.id, list.numbered))
                        .ok_or_else(|| bad("ODT list style"))?;
                    body.push_str(&format!("<text:list text:style-name=\"{}\">", esc(name)));
                    stack.push((list.id, list.numbered));
                }
                body.push_str("<text:list-item>");
                body.push_str(&w.paragraph(p)?);
                continue;
            }
        }
        close_lists(&mut body, &mut stack);
        if let Block::Paragraph(p) = block {
            if let Some(toc) = s.tables_of_contents.iter().find(|t| {
                t.entries.first().is_some_and(|e| e.paragraph == p.id)
                    && t.odt_cached
                        .as_ref()
                        .is_none_or(|c| c.title_paragraphs.is_empty())
            }) {
                body.push_str(&format!(
                    "<text:table-of-content text:name=\"DOC42_T{}\">",
                    toc.id
                ));
                if let Some(cache) = &toc.odt_cached {
                    body.push_str(&cache.source_xml);
                } else {
                    if toc.min_level != 0 {
                        return Err(unsupported("ODT index minimum heading level"));
                    }
                    body.push_str(&format!(
                        "<text:table-of-content-source text:outline-level=\"{}\">",
                        toc.max_level + 1
                    ));
                    for level in 1..=toc.max_level + 1 {
                        body.push_str(&format!("<text:table-of-content-entry-template text:outline-level=\"{level}\"><text:index-entry-link-start/><text:index-entry-text/><text:index-entry-link-end/></text:table-of-content-entry-template>"));
                    }
                    body.push_str("</text:table-of-content-source>");
                }
                body.push_str("<text:index-body>");
            }
        }
        match block {
            Block::Paragraph(p) => {
                let mut xml = w.paragraph(p)?;
                if s.tables_of_contents
                    .iter()
                    .filter(|t| t.odt_cached.as_ref().is_none_or(|c| c.needs_refresh))
                    .flat_map(|t| &t.entries)
                    .any(|e| e.heading == p.id)
                    && !s.bookmarks.iter().any(|b| {
                        b.name == format!("DOC42_H{}", p.id)
                            && b.start.paragraph == p.id
                            && b.start.offset == 0
                    })
                {
                    let start = xml.find('>').ok_or_else(|| bad("ODT paragraph"))? + 1;
                    xml.insert_str(
                        start,
                        &format!("<text:bookmark text:name=\"DOC42_H{}\"/>", p.id),
                    );
                }
                body.push_str(&xml);
            }
            Block::Image(i) => body.push_str(&w.image(i)?),
            Block::Table(t) => body.push_str(&w.table(t)?),
            Block::SectionBreak => return Err(unsupported("ODT section break")),
        }
        if let Block::Paragraph(p) = block {
            if s.tables_of_contents
                .iter()
                .any(|t| t.entries.last().is_some_and(|e| e.paragraph == p.id))
            {
                body.push_str("</text:index-body></text:table-of-content>");
            }
        }
    }
    close_lists(&mut body, &mut stack);
    let content=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-content {NS} office:version=\"1.3\"><office:automatic-styles>{source_styles}{lists}{}</office:automatic-styles><office:body><office:text>{body}</office:text></office:body></office:document-content>",w.automatic);
    parts.insert("content.xml".into(), content.into_bytes());
    if let Some(existing) = parts.get("styles.xml").cloned() {
        let text = String::from_utf8(existing).map_err(bad)?;
        let changed_page = s
            .odt_source_page
            .as_ref()
            .is_some_and(|p| p != &s.sections[0].page);
        let changed_headers = s
            .odt_source_headers
            .as_ref()
            .is_some_and(|(h, f)| h != &s.sections[0].header || f != &s.sections[0].footer)
            || s.odt_source_header_fields.as_ref().is_some_and(|original| {
                let current: Vec<_> = s
                    .fields
                    .iter()
                    .filter(|f| header_ids.contains(&f.at.paragraph))
                    .cloned()
                    .collect();
                *original != current
            });
        if changed_page || changed_headers {
            // Start beyond every generated name present in the original style package.
            let source = roxmltree::Document::parse(&text).map_err(bad)?;
            for node in source.descendants().filter(|n| n.is_element()) {
                if let Some(name) = attr(node, S, "name") {
                    if let Some(number) = name
                        .strip_prefix("DOC42P")
                        .or_else(|| name.strip_prefix("DOC42T"))
                        .or_else(|| name.strip_prefix("DOC42C"))
                        .or_else(|| name.strip_prefix("DOC42W"))
                        .and_then(|n| n.parse::<usize>().ok())
                    {
                        if number.checked_add(MAX_BYTES).is_none() {
                            return Err(Error::Limit);
                        }
                        w.next = w.next.max(number);
                    }
                }
            }
            let updated = overlay_styles(&text, s, &mut w, changed_page, changed_headers)?;
            parts.insert("styles.xml".into(), updated.into_bytes());
        }
    }
    if !parts.contains_key("styles.xml") {
        let p = &s.sections[0].page;
        let header = if s.sections[0].header.is_empty() {
            String::new()
        } else {
            let mut out = String::from("<style:header>");
            for paragraph in &s.sections[0].header {
                out.push_str(&w.paragraph(paragraph)?)
            }
            out.push_str("</style:header>");
            out
        };
        let footer = if s.sections[0].footer.is_empty() {
            String::new()
        } else {
            let mut out = String::from("<style:footer>");
            for paragraph in &s.sections[0].footer {
                out.push_str(&w.paragraph(paragraph)?)
            }
            out.push_str("</style:footer>");
            out
        };
        let styles=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-styles {NS} office:version=\"1.3\"><office:styles/><office:automatic-styles>{}<style:page-layout style:name=\"DOC42Page\"><style:page-layout-properties fo:page-width=\"{}pt\" fo:page-height=\"{}pt\" fo:margin-left=\"{}pt\" fo:margin-right=\"{}pt\" fo:margin-top=\"{}pt\" fo:margin-bottom=\"{}pt\"/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"DOC42Page\">{header}{footer}</style:master-page></office:master-styles></office:document-styles>",w.automatic,p.width_twips as f64/20.0,p.height_twips as f64/20.0,p.margin_left as f64/20.0,p.margin_right as f64/20.0,p.margin_top as f64/20.0,p.margin_bottom as f64/20.0);
        parts.insert("styles.xml".into(), styles.into_bytes());
    }
    let mut manifest = if let Some(bytes) = parts.remove("META-INF/manifest.xml") {
        let text = String::from_utf8(bytes).map_err(bad)?;
        if text.contains("encryption-data") {
            return Err(unsupported("encrypted ODT package"));
        }
        text
    } else {
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.3\"><manifest:file-entry manifest:full-path=\"/\" manifest:media-type=\"{MIME}\"/><manifest:file-entry manifest:full-path=\"content.xml\" manifest:media-type=\"text/xml\"/><manifest:file-entry manifest:full-path=\"styles.xml\" manifest:media-type=\"text/xml\"/></manifest:manifest>")
    };
    for (path, mime, data) in w.pictures {
        xml_insert(
            &mut manifest,
            &format!(
                "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
                esc(&path),
                esc(&mime)
            ),
        )?;
        parts.insert(path, data);
    }
    parts.insert("META-INF/manifest.xml".into(), manifest.into_bytes());
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "mimetype",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .map_err(bad)?;
    zip.write_all(MIME.as_bytes()).map_err(bad)?;
    for (name, data) in parts {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )
        .map_err(bad)?;
        zip.write_all(&data).map_err(bad)?;
    }
    let out = zip.finish().map_err(bad)?.into_inner();
    if out.len() > MAX_BYTES {
        return Err(Error::Limit);
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cached_odt_preserves_self_closing_styles_and_inherited_rebound_namespaces() {
        let content = format!(
            r##"<office:document-content xmlns:office="{O}" xmlns:text="{T}" xmlns:ctx="urn:wrong-index" xmlns:style="urn:wrong-style" xmlns:xlink="{XL}"><office:automatic-styles xmlns:style="{S}"><style:style style:name="PIndex" style:family="paragraph"/></office:automatic-styles><office:body><office:text xmlns="{T}" xmlns:ctx="{T}" xmlns:t="{T}"><ctx:table-of-content xmlns:text="{T}" text:name="Table > 1"><table-of-content-source text:outline-level="1"><table-of-content-entry-template text:outline-level="1" text:style-name="PIndex"><index-entry-link-start/><index-entry-text/><index-entry-tab-stop/><index-entry-page-number/><index-entry-link-end/></table-of-content-entry-template></table-of-content-source><index-body><p text:style-name="PIndex"><a xlink:href="#Target">Heading</a><tab/><a xlink:href="#Target">4</a></p></index-body></ctx:table-of-content><p>Body</p><h text:outline-level="1"><bookmark text:name="Target"/>Heading</h></office:text></office:body></office:document-content>"##
        );
        let styles = format!("<office:document-styles {NS}><office:automatic-styles><style:page-layout style:name=\"P\"><style:page-layout-properties/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"P\"/></office:master-styles></office:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let mut doc =
            RichDocument::decode(&zip.finish().unwrap().into_inner(), Format::Odt).unwrap();
        let projection = String::from_utf8(doc.rich_edit_projection().unwrap())
            .unwrap()
            .replacen("Body", "Body edited", 1);
        doc.apply_rich_edit_projection(projection.as_bytes(), (0, 0))
            .unwrap();
        let reopened =
            RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        let cache = reopened.state().tables_of_contents[0]
            .odt_cached
            .as_ref()
            .unwrap();
        let index = roxmltree::Document::parse(&cache.index_xml).unwrap();
        assert!(index.root_element().has_tag_name((T, "table-of-content")));
        assert_eq!(
            index.root_element().lookup_namespace_uri(Some("ctx")),
            Some(T)
        );
        assert_eq!(
            index.root_element().lookup_namespace_uri(Some("t")),
            Some(T)
        );
        assert_eq!(index.root_element().lookup_namespace_uri(None), Some(T));
        assert!(index.descendants().any(|n| n.has_tag_name((T, "p"))));
        assert!(index
            .descendants()
            .any(|n| n.has_tag_name((T, "a")) && n.text() == Some("4")));
        let style = roxmltree::Document::parse(&cache.automatic_styles).unwrap();
        assert!(style.root_element().has_tag_name((S, "style")));
        assert_eq!(attr(style.root_element(), S, "name"), Some("PIndex"));
        assert!(style.root_element().children().all(|n| !n.is_element()));
        assert!(reopened.state().blocks.iter().any(|b| matches!(b, Block::Paragraph(p) if p.runs.iter().map(|r| r.text.as_str()).collect::<String>() == "Body edited")));
    }
    #[test]
    fn external_odt_toc_cache_and_styles_survive_body_edit_and_update() {
        let content = format!(
            r##"<office:document-content {NS}><office:automatic-styles><style:style style:name="PIndex" style:family="paragraph"><style:paragraph-properties fo:margin-left="12pt"/></style:style><style:style style:name="PTitle" style:family="paragraph"><style:paragraph-properties fo:text-align="center"/></style:style></office:automatic-styles><office:body><office:text><text:table-of-content text:name="Table1" text:protected="true"><text:table-of-content-source text:outline-level="3" text:use-outline-level="true"><text:index-title-template>Contents</text:index-title-template><text:table-of-content-entry-template text:outline-level="1" text:style-name="PIndex"><text:index-entry-link-start/><text:index-entry-text/><text:index-entry-tab-stop/><text:index-entry-page-number/><text:index-entry-link-end/></text:table-of-content-entry-template></text:table-of-content-source><text:index-body><text:index-title text:name="Title1"><text:p text:style-name="PTitle">Contents</text:p></text:index-title><text:p text:style-name="PIndex"><text:a xlink:type="simple" xlink:href="#Target">Heading</text:a><text:tab/><text:a xlink:type="simple" xlink:href="#Target">7</text:a></text:p></text:index-body></text:table-of-content><text:p>Body</text:p><text:h text:outline-level="1"><text:bookmark-start text:name="Target"/>Heading<text:bookmark-end text:name="Target"/></text:h></office:text></office:body></office:document-content>"##
        );
        let styles = format!("<office:document-styles {NS}><office:automatic-styles><style:page-layout style:name=\"P\"><style:page-layout-properties/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"P\"/></office:master-styles></office:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
            ("custom/keep.bin", b"opaque".as_slice()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let mut doc =
            RichDocument::decode(&zip.finish().unwrap().into_inner(), Format::Odt).unwrap();
        let toc = doc.state().tables_of_contents[0].id;
        assert_eq!(doc.state().tables_of_contents[0].max_level, 2);
        assert!(
            doc.state().tables_of_contents[0]
                .odt_cached
                .as_ref()
                .unwrap()
                .has_pages
        );
        let projection = String::from_utf8(doc.rich_edit_projection().unwrap())
            .unwrap()
            .replacen("Body", "Body edited", 1);
        doc.apply_rich_edit_projection(projection.as_bytes(), (0, 0))
            .unwrap();
        let reopened =
            RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        let cache = reopened.state().tables_of_contents[0]
            .odt_cached
            .as_ref()
            .unwrap();
        assert!(cache.index_xml.contains(">7</text:a>"));
        assert!(cache.index_xml.contains("index-title"));
        assert!(cache.automatic_styles.contains("PIndex"));
        assert!(cache.automatic_styles.contains("PTitle"));
        assert_eq!(reopened.state().odt_parts["custom/keep.bin"], b"opaque");
        doc.update_toc(toc).unwrap();
        let updated = RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        let cache = updated.state().tables_of_contents[0]
            .odt_cached
            .as_ref()
            .unwrap();
        assert!(!cache.has_pages);
        assert!(cache.index_xml.contains("index-title"));
        assert!(cache.source_xml.contains("index-entry-page-number"));
        assert_eq!(updated.state().tables_of_contents[0].max_level, 2);
    }
    #[test]
    fn external_odt_links_and_bookmarks_survive_resave() {
        let content = format!(
            r##"<office:document-content xmlns:office="{O}" xmlns:text="{T}" xmlns:xlink="{XL}"><office:body><office:text><text:p><text:bookmark-start text:name="Target"/>Heading<text:bookmark-end text:name="Target"/></text:p><text:p><text:a xlink:type="simple" xlink:href="#Target">Jump</text:a> <text:a xlink:type="simple" xlink:href="https://example.org/">Visit</text:a></text:p></office:text></office:body></office:document-content>"##
        );
        let styles = format!("<office:document-styles {NS}><office:automatic-styles><style:page-layout style:name=\"P\"><style:page-layout-properties/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"P\"/></office:master-styles></office:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
            ("custom/keep.bin", b"opaque".as_slice()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let doc = RichDocument::decode(&zip.finish().unwrap().into_inner(), Format::Odt).unwrap();
        assert_eq!(doc.state().bookmarks[0].name, "Target");
        let Block::Paragraph(p) = &doc.state().blocks[1] else {
            panic!()
        };
        assert!(p.runs.iter().any(|r| r.link.as_deref() == Some("#Target")));
        assert!(p
            .runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://example.org/")));
        let reopened =
            RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        assert_eq!(reopened.state().bookmarks[0].name, "Target");
        assert_eq!(reopened.state().odt_parts["custom/keep.bin"], b"opaque");
    }
    fn paragraph(id: u64, text: &str) -> Paragraph {
        Paragraph {
            id,
            style: ParagraphStyle::default(),
            runs: vec![Run {
                text: text.into(),
                style: TextStyle::default(),
                link: None,
            }],
        }
    }
    #[test]
    fn external_odt_note_preserves_class_citation_and_text() {
        let content = format!(
            r#"<office:document-content xmlns:office="{O}" xmlns:text="{T}"><office:body><office:text><text:p>Hello<text:note text:id="ftn1" text:note-class="endnote"><text:note-citation>i</text:note-citation><text:note-body><text:p>External note</text:p></text:note-body></text:note></text:p></office:text></office:body></office:document-content>"#
        );
        let styles = format!("<office:document-styles {NS}><office:automatic-styles><style:page-layout style:name=\"P\"><style:page-layout-properties/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"P\"/></office:master-styles></office:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let doc = RichDocument::decode(&zip.finish().unwrap().into_inner(), Format::Odt).unwrap();
        assert!(doc.state().footnotes[0].endnote);
        assert_eq!(doc.state().footnotes[0].citation.as_deref(), Some("i"));
        assert_eq!(doc.state().footnotes[0].at.offset, 5);
        let reopened =
            RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        assert_eq!(reopened.state().footnotes[0].citation.as_deref(), Some("i"));
        assert_eq!(
            reopened.state().footnotes[0].paragraphs[0]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect::<String>(),
            "External note"
        );
    }
    #[test]
    fn external_odt_annotation_survives_edit_and_resave() {
        let content = format!(
            r#"<office:document-content xmlns:office="{O}" xmlns:text="{T}" xmlns:dc="{DC}"><office:body><office:text><text:p>Hello <office:annotation office:name="review"><dc:creator>Ada</dc:creator><dc:date>2026-01-02</dc:date><text:p>Review<text:s/>this</text:p></office:annotation>world<office:annotation-end office:name="review"/></text:p></office:text></office:body></office:document-content>"#
        );
        let styles = format!("<office:document-styles {NS}><office:automatic-styles><style:page-layout style:name=\"P\"><style:page-layout-properties/></style:page-layout></office:automatic-styles><office:master-styles><style:master-page style:name=\"Standard\" style:page-layout-name=\"P\"/></office:master-styles></office:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
            ("custom/keep.bin", b"opaque".as_slice()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let mut doc =
            RichDocument::decode(&zip.finish().unwrap().into_inner(), Format::Odt).unwrap();
        assert_eq!(doc.state().comments[0].text, "Review this");
        assert_eq!(
            (
                doc.state().comments[0].start.offset,
                doc.state().comments[0].end.offset
            ),
            (6, 11)
        );
        let id = doc.state().comments[0].id;
        doc.edit_comment(id, "Updated".into(), false).unwrap();
        let reopened =
            RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        assert_eq!(reopened.state().comments[0].text, "Updated");
        assert_eq!(
            reopened.state().comments[0].date.as_deref(),
            Some("2026-01-02")
        );
        assert_eq!(reopened.state().odt_parts["custom/keep.bin"], b"opaque");
    }
    #[test]
    fn external_style_overlay_preserves_package_and_uses_namespace_aliases() {
        let content = format!("<o:document-content xmlns:o=\"{O}\" xmlns:t=\"{T}\"><o:body><o:text><t:p>External</t:p></o:text></o:body></o:document-content>");
        let styles = format!("<o:document-styles xmlns:o=\"{O}\" xmlns:s=\"{S}\" xmlns:f=\"{F}\"><o:automatic-styles><s:page-layout s:name=\"P\"><s:page-layout-properties f:margin=\"2cm\"/></s:page-layout></o:automatic-styles><o:master-styles><s:master-page s:name=\"Standard\" s:page-layout-name=\"P\"/></o:master-styles></o:document-styles>");
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [
            ("mimetype", MIME.as_bytes()),
            ("content.xml", content.as_bytes()),
            ("styles.xml", styles.as_bytes()),
            ("custom/data.bin", b"keep me".as_slice()),
        ] {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        let input = zip.finish().unwrap().into_inner();
        let mut d = RichDocument::decode(&input, Format::Odt).unwrap();
        d.transact(|s| {
            s.sections[0].page.margin_left = 1200;
            let p = paragraph(s.next_id, "Page ");
            s.next_id += 1;
            let id = p.id;
            s.sections[0].header.push(p);
            let field_id = s.next_id;
            s.next_id += 1;
            s.fields.push(Field {
                id: field_id,
                at: Position {
                    paragraph: id,
                    offset: 5,
                },
                kind: FieldKind::PageNumber,
            });
            Ok(())
        })
        .unwrap();
        let bytes = d.encode(Format::Odt).unwrap();
        let reopened = RichDocument::decode(&bytes, Format::Odt).unwrap();
        assert_eq!(reopened.state().sections[0].page.margin_left, 1200);
        assert_eq!(reopened.state().fields.len(), 1);
        assert_eq!(reopened.state().odt_parts["custom/data.bin"], b"keep me");
    }
    #[test]
    fn retained_style_attributes_survive_rich_projection_and_save() {
        let mut s = State::default();
        let Block::Paragraph(p) = &mut s.blocks[0] else {
            panic!()
        };
        p.runs[0].text = "hello".into();
        p.runs[0]
            .style
            .odt_extra
            .insert("fo:language".into(), "en".into());
        p.style
            .odt_extra
            .insert("fo:line-height".into(), "120%".into());
        let bytes = encode(&s).unwrap();
        let mut d = RichDocument::decode(&bytes, Format::Odt).unwrap();
        let projection = d.rich_edit_projection().unwrap();
        d.apply_rich_edit_projection(&projection, (0, 0)).unwrap();
        let output = d.encode(Format::Odt).unwrap();
        let reopened = RichDocument::decode(&output, Format::Odt).unwrap();
        let Block::Paragraph(p) = &reopened.state().blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.style.odt_extra.get("fo:line-height").map(String::as_str),
            Some("120%")
        );
        assert_eq!(
            p.runs[0]
                .style
                .odt_extra
                .get("fo:language")
                .map(String::as_str),
            Some("en")
        );
    }
    #[test]
    fn styled_list_table_image_page_and_header_round_trip() {
        let mut s = State::default();
        let Block::Paragraph(p) = &mut s.blocks[0] else {
            panic!()
        };
        p.runs[0].text = "Hello world".into();
        p.runs[0].style.bold = true;
        p.runs[0].style.color = Some(0x336699);
        p.style.alignment = Alignment::Center;
        s.sections[0].page.margin_top = 1000;
        let mut list = paragraph(2, "first");
        list.style.list = Some(ListMarker {
            id: 3,
            level: 0,
            numbered: true,
        });
        s.blocks.push(Block::Paragraph(list));
        let mut child = paragraph(4, "nested");
        child.style.list = Some(ListMarker {
            id: 5,
            level: 1,
            numbered: false,
        });
        s.blocks.push(Block::Paragraph(child));
        s.blocks.push(Block::Table(Table {
            column_widths_twips: Vec::new(),
            id: 6,
            rows: vec![vec![Cell {
                paragraphs: vec![paragraph(7, "cell")],
                row_span: 1,
                col_span: 1,
                border: None,
                fill: None,
            }]],
        }));
        s.blocks.push(Block::Image(Image {
            id: 8,
            mime: "image/png".into(),
            data: vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0],
            width_twips: 1440,
            height_twips: 720,
            alt: "picture".into(),
        }));
        s.sections[0].header.push(paragraph(9, "header"));
        s.next_id = 10;
        let bytes = encode(&s).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(decoded.blocks.len(), 5);
        let Block::Paragraph(p) = &decoded.blocks[0] else {
            panic!()
        };
        assert!(p.runs[0].style.bold);
        assert_eq!(p.runs[0].style.color, Some(0x336699));
        assert_eq!(p.style.alignment, Alignment::Center);
        let Block::Paragraph(child) = &decoded.blocks[2] else {
            panic!()
        };
        assert_eq!(child.style.list.as_ref().unwrap().level, 1);
        let Block::Table(table) = &decoded.blocks[3] else {
            panic!()
        };
        assert_eq!(table.rows[0][0].paragraphs[0].runs[0].text, "cell");
        let Block::Image(image) = &decoded.blocks[4] else {
            panic!()
        };
        assert_eq!(image.alt, "picture");
        assert_eq!(decoded.sections[0].page.margin_top, 1000);
        assert_eq!(decoded.sections[0].header[0].runs[0].text, "header");
        let reopened = decode(&encode(&decoded).unwrap()).unwrap();
        assert_eq!(reopened.blocks.len(), 5);
    }
}
