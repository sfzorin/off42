//! OOXML style inheritance for supported paragraph and character properties.
use super::*;
use std::collections::{BTreeMap, HashMap, HashSet};
type Node<'a> = roxmltree::Node<'a, 'a>;
#[derive(Clone, Default)]
struct Patch {
    base: Option<String>,
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
    color: Option<Option<u32>>,
    theme_color: Option<String>,
    theme_tint: Option<String>,
    theme_shade: Option<String>,
}
#[derive(Clone, Default)]
struct ParaPatch {
    alignment: Option<Alignment>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    before: Option<u32>,
    after: Option<u32>,
    line: Option<i32>,
    tabs: Option<Vec<i32>>,
    list: Option<Option<ListMarker>>,
    keep_with_next: Option<bool>,
    keep_lines: Option<bool>,
    page_break_before: Option<bool>,
    outline_level: Option<u8>,
}
pub(super) struct Styles {
    defs: HashMap<String, Patch>,
    default_run: RunPatch,
    default_para: ParaPatch,
    default_paragraph: Option<String>,
    major: String,
    minor: String,
    theme_colors: HashMap<String, u32>,
}
impl Styles {
    pub(super) fn new(
        parts: &BTreeMap<String, Vec<u8>>,
        numbering: &numbering::Numbering,
    ) -> Result<Self, Error> {
        let mut s = Self {
            defs: HashMap::new(),
            default_run: RunPatch::default(),
            default_para: ParaPatch::default(),
            default_paragraph: None,
            major: "Cambria".into(),
            minor: "Calibri".into(),
            theme_colors: HashMap::new(),
        };
        if let Some(theme) = parts.get("word/theme/theme1.xml") {
            let text = std::str::from_utf8(theme).map_err(bad)?;
            let xml = roxmltree::Document::parse(text).map_err(bad)?;
            if let Some(scheme) = xml
                .descendants()
                .find(|n| n.is_element() && n.tag_name().name() == "clrScheme")
            {
                for slot in scheme.children().filter(|n| n.is_element()) {
                    if let Some(value) = slot
                        .children()
                        .find(|n| n.is_element())
                        .and_then(|n| n.attribute("val").or_else(|| n.attribute("lastClr")))
                    {
                        if let Ok(color) = u32::from_str_radix(value, 16) {
                            s.theme_colors.insert(slot.tag_name().name().into(), color);
                        }
                    }
                }
            }
            for n in xml.descendants().filter(|n| {
                n.is_element() && matches!(n.tag_name().name(), "majorFont" | "minorFont")
            }) {
                if let Some(font) = n
                    .children()
                    .find(|c| c.is_element() && c.tag_name().name() == "latin")
                    .and_then(|c| c.attribute("typeface"))
                {
                    if !font.is_empty() {
                        if n.tag_name().name() == "majorFont" {
                            s.major = font.into()
                        } else {
                            s.minor = font.into()
                        }
                    }
                }
            }
        }
        if let Some(data) = parts.get("word/styles.xml") {
            let text = std::str::from_utf8(data).map_err(bad)?;
            let xml = roxmltree::Document::parse(text).map_err(bad)?;
            let root = xml.root_element();
            if let Some(defaults) = root.children().find(|n| n.has_tag_name((W, "docDefaults"))) {
                if let Some(r) = defaults.descendants().find(|n| n.has_tag_name((W, "rPr"))) {
                    s.default_run = s.run_patch(r)?;
                }
                if let Some(p) = defaults.descendants().find(|n| n.has_tag_name((W, "pPr"))) {
                    s.default_para = para_patch(p, numbering)?;
                }
            }
            for node in root.children().filter(|n| n.has_tag_name((W, "style"))) {
                let id = val(node, "styleId")
                    .ok_or_else(|| bad("style without ID"))?
                    .to_owned();
                let mut patch = Patch::default();
                for item in node.children().filter(|n| n.is_element()) {
                    match item.tag_name().name() {
                        "basedOn" => patch.base = val(item, "val").map(str::to_owned),
                        "rPr" => patch.run = s.run_patch(item)?,
                        "pPr" => patch.para = para_patch(item, numbering)?,
                        _ => {}
                    }
                }
                if val(node, "type") == Some("paragraph")
                    && matches!(val(node, "default"), Some("1" | "true"))
                {
                    s.default_paragraph = Some(id.clone());
                }
                s.defs.insert(id, patch);
            }
        }
        Ok(s)
    }
    fn run_patch(&self, node: Node<'_>) -> Result<RunPatch, Error> {
        let mut p = RunPatch::default();
        for c in node.children().filter(|n| n.is_element()) {
            let v = || val(c, "val");
            let on = || !matches!(v(), Some("0" | "false" | "off"));
            match c.tag_name().name() {
                "rFonts" => {
                    p.font = val(c, "ascii")
                        .or_else(|| val(c, "hAnsi"))
                        .map(str::to_owned)
                        .or_else(|| {
                            val(c, "asciiTheme")
                                .or_else(|| val(c, "hAnsiTheme"))
                                .map(|v| {
                                    if v.starts_with("major") {
                                        self.major.clone()
                                    } else {
                                        self.minor.clone()
                                    }
                                })
                        });
                }
                "sz" => p.size = Some(num(c, "val", 24)?),
                "b" => p.bold = Some(on()),
                "i" => p.italic = Some(on()),
                "u" => p.underline = Some(!matches!(v(), Some("none" | "0"))),
                "strike" => p.strike = Some(on()),
                "color" => {
                    p.theme_color = val(c, "themeColor").map(str::to_owned);
                    p.theme_tint = val(c, "themeTint").map(str::to_owned);
                    p.theme_shade = val(c, "themeShade").map(str::to_owned);
                    if let Some(x) = v() {
                        p.color = Some(if x == "auto" {
                            None
                        } else {
                            Some(u32::from_str_radix(x, 16).map_err(bad)?)
                        });
                    } else if let Some(theme) = &p.theme_color {
                        let key = match theme.as_str() {
                            "text1" => "dk1",
                            "text2" => "dk2",
                            "background1" => "lt1",
                            "background2" => "lt2",
                            other => other,
                        };
                        let mut color = *self.theme_colors.get(key).ok_or_else(|| {
                            Error::Unsupported(format!("unknown theme color {theme}"))
                        })?;
                        for (attr, tint) in [
                            (p.theme_tint.as_deref(), true),
                            (p.theme_shade.as_deref(), false),
                        ] {
                            if let Some(value) = attr {
                                let scale = u32::from_str_radix(value, 16).map_err(bad)?;
                                if scale > 255 {
                                    return Err(bad("theme color transform"));
                                }
                                let channel = |shift: u32| {
                                    let v = (color >> shift) & 255;
                                    if tint {
                                        255 - (255 - v) * scale / 255
                                    } else {
                                        v * scale / 255
                                    }
                                };
                                color = (channel(16) << 16) | (channel(8) << 8) | channel(0);
                            }
                        }
                        p.color = Some(Some(color));
                    }
                }
                "rStyle" => {}
                _ => {}
            }
        }
        Ok(p)
    }
    fn apply_chain(
        &self,
        id: &str,
        t: &mut TextStyle,
        p: &mut ParagraphStyle,
        seen: &mut HashSet<String>,
    ) -> Result<(), Error> {
        if !seen.insert(id.to_owned()) {
            return Err(bad("cyclic style inheritance"));
        }
        let def = self
            .defs
            .get(id)
            .ok_or_else(|| Error::Unsupported(format!("missing DOCX style {id}")))?;
        if let Some(base) = &def.base {
            self.apply_chain(base, t, p, seen)?;
        }
        apply_run(t, &def.run);
        apply_para(p, &def.para);
        Ok(())
    }
    pub(super) fn paragraph(
        &self,
        node: Option<Node<'_>>,
        numbering: &numbering::Numbering,
    ) -> Result<(TextStyle, ParagraphStyle), Error> {
        let mut t = TextStyle::default();
        let mut p = ParagraphStyle::default();
        apply_run(&mut t, &self.default_run);
        apply_para(&mut p, &self.default_para);
        let explicit = node
            .and_then(|n| n.children().find(|x| x.has_tag_name((W, "pStyle"))))
            .and_then(|x| val(x, "val"));
        let name = explicit.or(self.default_paragraph.as_deref());
        if let Some(id) = name {
            self.apply_chain(id, &mut t, &mut p, &mut HashSet::new())?;
        }
        p.source_style = explicit.map(str::to_owned);
        if let Some(n) = node {
            apply_para(&mut p, &para_patch(n, numbering)?);
        }
        Ok((t, p))
    }
    pub(super) fn run(&self, node: Option<Node<'_>>, base: &TextStyle) -> Result<TextStyle, Error> {
        let mut t = base.clone();
        t.source_style = None;
        if let Some(n) = node {
            let explicit = n
                .children()
                .find(|x| x.has_tag_name((W, "rStyle")))
                .and_then(|x| val(x, "val"));
            if let Some(id) = explicit {
                self.apply_chain(
                    id,
                    &mut t,
                    &mut ParagraphStyle::default(),
                    &mut HashSet::new(),
                )?;
                t.source_style = Some(id.to_owned());
            }
            apply_run(&mut t, &self.run_patch(n)?);
        }
        Ok(t)
    }
}
fn para_patch(node: Node<'_>, numbering: &numbering::Numbering) -> Result<ParaPatch, Error> {
    let mut p = ParaPatch::default();
    for c in node.children().filter(|n| n.is_element()) {
        match c.tag_name().name() {
            "tabs" => {
                let mut tabs = Vec::new();
                for tab in c.children().filter(|n| n.is_element()) {
                    if !tab.has_tag_name((W, "tab"))
                        || !matches!(val(tab, "val"), Some("left" | "start"))
                    {
                        return Err(Error::Unsupported("DOCX tab type".into()));
                    }
                    tabs.push(num(tab, "pos", 0i32)?);
                }
                tabs.sort_unstable();
                tabs.dedup();
                p.tabs = Some(tabs);
            }
            "keepNext" => {
                p.keep_with_next = Some(!matches!(val(c, "val"), Some("0" | "false" | "off")))
            }
            "keepLines" => {
                p.keep_lines = Some(!matches!(val(c, "val"), Some("0" | "false" | "off")))
            }
            "pageBreakBefore" => {
                p.page_break_before = Some(!matches!(val(c, "val"), Some("0" | "false" | "off")))
            }
            "outlineLvl" => p.outline_level = Some(num(c, "val", 0u8)?),
            "numPr" => {
                let level = c
                    .children()
                    .find(|n| n.has_tag_name((W, "ilvl")))
                    .map(|n| num(n, "val", 0u8))
                    .transpose()?
                    .unwrap_or(0);
                if let Some(id) = c
                    .children()
                    .find(|n| n.has_tag_name((W, "numId")))
                    .map(|n| num(n, "val", 0u64))
                    .transpose()?
                {
                    p.list = Some(if id == 0 {
                        None
                    } else {
                        Some(numbering.marker(id, level)?)
                    });
                }
            }
            "jc" => {
                p.alignment = Some(match val(c, "val").unwrap_or("left") {
                    "left" | "start" => Alignment::Left,
                    "center" => Alignment::Center,
                    "right" | "end" => Alignment::Right,
                    "both" => Alignment::Justify,
                    _ => return Err(Error::Unsupported("DOCX alignment".into())),
                })
            }
            "ind" => {
                if val(c, "left").is_some() {
                    p.left = Some(num(c, "left", 0)?)
                }
                if val(c, "right").is_some() {
                    p.right = Some(num(c, "right", 0)?)
                }
                if let Some(v) = val(c, "firstLine") {
                    p.first = Some(v.parse().map_err(bad)?)
                }
                if let Some(v) = val(c, "hanging") {
                    let n = v.parse::<i32>().map_err(bad)?;
                    p.first = Some(n.checked_neg().ok_or_else(|| bad("indent"))?)
                }
            }
            "spacing" => {
                if val(c, "before").is_some() {
                    p.before = Some(num(c, "before", 0)?)
                }
                if val(c, "after").is_some() {
                    p.after = Some(num(c, "after", 0)?)
                }
                if val(c, "line").is_some() {
                    p.line = Some(num(c, "line", 0)?)
                }
            }
            _ => {}
        }
    }
    Ok(p)
}
fn apply_run(t: &mut TextStyle, p: &RunPatch) {
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
        t.color = v;
        t.theme_color = p.theme_color.clone();
        t.theme_tint = p.theme_tint.clone();
        t.theme_shade = p.theme_shade.clone();
    }
}
fn apply_para(t: &mut ParagraphStyle, p: &ParaPatch) {
    if let Some(v) = &p.tabs {
        t.tabs_twips = v.clone();
    }
    if let Some(v) = p.keep_with_next {
        t.keep_with_next = v;
    }
    if let Some(v) = p.keep_lines {
        t.keep_lines = v;
    }
    if let Some(v) = p.page_break_before {
        t.page_break_before = v;
    }
    if let Some(v) = p.outline_level {
        t.outline_level = Some(v);
    }
    if let Some(v) = &p.list {
        t.list = v.clone();
    }
    if let Some(v) = p.alignment {
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
