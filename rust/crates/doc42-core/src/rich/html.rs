//! HTML5 adapter for explicit paragraphs, inline styles, lists, simple tables and embedded images.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use html5ever::{parse_document, tendril::TendrilSink};
use markup5ever_rcdom::{Handle, NodeData, RcDom};
const MAX_IMAGE: usize = 8 * 1024 * 1024;
fn err(s: impl Into<String>) -> Error {
    Error::Unsupported(s.into())
}
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
fn attrs(n: &Handle) -> Vec<(String, String)> {
    match &n.data {
        NodeData::Element { attrs, .. } => attrs
            .borrow()
            .iter()
            .map(|a| (a.name.local.to_string(), a.value.to_string()))
            .collect(),
        _ => vec![],
    }
}
fn attribute(n: &Handle, name: &str) -> Option<String> {
    attrs(n)
        .into_iter()
        .find(|(k, _)| k == name)
        .map(|(_, v)| v)
}
fn node_name(n: &Handle) -> Option<String> {
    match &n.data {
        NodeData::Element { name, .. } => Some(name.local.to_string()),
        _ => None,
    }
}
fn color(s: &str) -> Result<u32, Error> {
    let value = s.trim().to_ascii_lowercase();
    let named = match value.as_str() {
        "black" => Some(0),
        "white" => Some(0xffffff),
        "red" => Some(0xff0000),
        "green" => Some(0x008000),
        "blue" => Some(0x0000ff),
        "yellow" => Some(0xffff00),
        "gray" | "grey" => Some(0x808080),
        "silver" => Some(0xc0c0c0),
        "maroon" => Some(0x800000),
        "purple" => Some(0x800080),
        "navy" => Some(0x000080),
        "teal" => Some(0x008080),
        "olive" => Some(0x808000),
        "orange" => Some(0xffa500),
        _ => None,
    };
    if let Some(v) = named {
        return Ok(v);
    }
    let v = value.strip_prefix('#').ok_or_else(|| err("HTML color"))?;
    let full = if v.len() == 3 {
        v.chars().flat_map(|c| [c, c]).collect::<String>()
    } else {
        v.to_owned()
    };
    if full.len() != 6 {
        return Err(err("HTML color"));
    }
    u32::from_str_radix(&full, 16).map_err(|_| err("HTML color"))
}
fn points(s: &str) -> Result<f64, Error> {
    points_with_limit(s, 10000.0)
}
fn points_with_limit(s: &str, limit: f64) -> Result<f64, Error> {
    let s = s.trim();
    let (v, scale) = if let Some(v) = s.strip_suffix("pt") {
        (v, 1.0)
    } else if let Some(v) = s.strip_suffix("px") {
        (v, 0.75)
    } else if let Some(v) = s.strip_suffix("in") {
        (v, 72.0)
    } else if let Some(v) = s.strip_suffix("cm") {
        (v, 72.0 / 2.54)
    } else if s == "0" {
        ("0", 1.0)
    } else {
        return Err(err("HTML length unit"));
    };
    let v = v.trim().parse::<f64>().map_err(|_| err("HTML length"))? * scale;
    if !v.is_finite() || v.abs() > limit {
        return Err(err("HTML length"));
    }
    Ok(v)
}
fn css(s: &str, text: &mut TextStyle, para: &mut ParagraphStyle) -> Result<(), Error> {
    for item in s.split(';').filter(|x| !x.trim().is_empty()) {
        let (k, v) = item
            .split_once(':')
            .ok_or_else(|| err("HTML style declaration"))?;
        let k = k.trim().to_ascii_lowercase();
        let v = v.trim();
        match k.as_str() {
            "font-family" => text.font = v.trim_matches(['\'', '"']).to_owned(),
            "font-size" => {
                let n = (points(v)? * 2.0).round();
                if !(1.0..=65535.0).contains(&n) {
                    return Err(err("HTML font size"));
                }
                text.size_half_points = n as u16
            }
            "font-weight" => {
                text.bold = match v {
                    "bold" | "700" | "800" | "900" => true,
                    "normal" | "400" => false,
                    _ => return Err(err("HTML font weight")),
                }
            }
            "font-style" => {
                text.italic = match v {
                    "italic" | "oblique" => true,
                    "normal" => false,
                    _ => return Err(err("HTML font style")),
                }
            }
            "text-decoration" | "text-decoration-line" => {
                text.underline = v.split_whitespace().any(|x| x == "underline");
                text.strike = v.split_whitespace().any(|x| x == "line-through");
                if v.split_whitespace()
                    .any(|x| !matches!(x, "underline" | "line-through" | "none"))
                {
                    return Err(err("HTML text decoration"));
                }
            }
            "color" => text.color = Some(color(v)?),
            "background-color" => text.highlight = Some(color(v)?),
            "text-align" => {
                para.alignment = match v {
                    "left" | "start" => Alignment::Left,
                    "center" => Alignment::Center,
                    "right" | "end" => Alignment::Right,
                    "justify" => Alignment::Justify,
                    _ => return Err(err("HTML alignment")),
                }
            }
            "page-break-before" | "break-before" => {
                para.page_break_before = match v {
                    "always" | "page" => true,
                    "auto" => false,
                    _ => return Err(err("HTML page break")),
                };
            }
            "margin-left" => para.left_twips = (points(v)? * 20.0).round() as i32,
            "margin-right" => para.right_twips = (points(v)? * 20.0).round() as i32,
            "text-indent" => para.first_line_twips = (points(v)? * 20.0).round() as i32,
            "margin-top" => {
                para.before_twips = u32::try_from((points(v)? * 20.0).round() as i64)
                    .map_err(|_| err("negative margin"))?
            }
            "margin-bottom" => {
                para.after_twips = u32::try_from((points(v)? * 20.0).round() as i64)
                    .map_err(|_| err("negative margin"))?
            }
            "line-height" => para.line_twips = (points(v)? * 20.0).round() as i32,
            _ => return Err(err(format!("HTML CSS property {k}"))),
        }
    }
    Ok(())
}
fn check_attrs(n: &Handle, allowed: &[&str]) -> Result<(), Error> {
    for (k, _) in attrs(n) {
        if !allowed.contains(&k.as_str()) {
            return Err(err(format!("HTML attribute {k}")));
        }
    }
    Ok(())
}
struct Reader<'a> {
    resolver: Option<&'a dyn Fn(&str) -> Result<Vec<u8>, Error>>,
    state: State,
    next: u64,
    list: Vec<(u64, bool)>,
}
impl Reader<'_> {
    fn id(&mut self) -> u64 {
        let id = self.next;
        self.next += 1;
        id
    }
    fn paragraph(
        &mut self,
        node: &Handle,
        level: Option<u8>,
        list: Option<(u64, bool)>,
    ) -> Result<Paragraph, Error> {
        check_attrs(node, &["style"])?;
        let mut ts = TextStyle::default();
        let mut ps = ParagraphStyle::default();
        ps.outline_level = level;
        ps.list = list.map(|(id, numbered)| ListMarker {
            id,
            numbered,
            level: (self.list.len().saturating_sub(1)) as u8,
        });
        if let Some(v) = attribute(node, "style") {
            css(&v, &mut ts, &mut ps)?
        }
        let mut runs = Vec::new();
        for child in node.children.borrow().iter() {
            self.inline(child, &ts, &ps, None, &mut runs)?
        }
        Ok(Paragraph {
            id: self.id(),
            style: ps,
            runs,
        })
    }
    fn inline(
        &mut self,
        node: &Handle,
        style: &TextStyle,
        para: &ParagraphStyle,
        link: Option<String>,
        out: &mut Vec<Run>,
    ) -> Result<(), Error> {
        match &node.data {
            NodeData::Text { contents } => {
                let text = contents.borrow().to_string();
                if text.contains(['\r', '\n']) {
                    let text = text.replace(['\r', '\n'], " ");
                    if !text.is_empty() {
                        out.push(Run {
                            text,
                            style: style.clone(),
                            link,
                        })
                    }
                } else if !text.is_empty() {
                    out.push(Run {
                        text,
                        style: style.clone(),
                        link,
                    })
                }
                Ok(())
            }
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                let mut next = style.clone();
                let mut p = para.clone();
                let mut l = link;
                match tag {
                    "span" => check_attrs(node, &["style"])?,
                    "strong" | "b" => {
                        check_attrs(node, &["style"])?;
                        next.bold = true
                    }
                    "em" | "i" => {
                        check_attrs(node, &["style"])?;
                        next.italic = true
                    }
                    "u" => {
                        check_attrs(node, &["style"])?;
                        next.underline = true
                    }
                    "s" | "del" | "strike" => {
                        check_attrs(node, &["style"])?;
                        next.strike = true
                    }
                    "code" => {
                        check_attrs(node, &["style"])?;
                        next.font = "monospace".into()
                    }
                    "a" => {
                        check_attrs(node, &["href", "style"])?;
                        l = Some(
                            attribute(node, "href").ok_or_else(|| err("HTML link without href"))?,
                        )
                    }
                    "br" => return Err(err("HTML line break within paragraph")),
                    _ => return Err(err(format!("HTML inline element {tag}"))),
                };
                if let Some(v) = attribute(node, "style") {
                    css(&v, &mut next, &mut p)?;
                    if p != *para {
                        return Err(err("paragraph CSS on inline HTML"));
                    }
                }
                for child in node.children.borrow().iter() {
                    self.inline(child, &next, &p, l.clone(), out)?
                }
                Ok(())
            }
            NodeData::Comment { .. } => Err(err("HTML comment")),
            _ => Ok(()),
        }
    }
    fn blank_cell(&mut self) -> Cell {
        Cell {
            paragraphs: vec![Paragraph {
                id: self.id(),
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
        }
    }
    fn table(&mut self, node: &Handle) -> Result<Table, Error> {
        check_attrs(node, &[])?;
        let mut rows = Vec::new();
        let mut widths = Vec::new();
        fn gather(
            n: &Handle,
            rows: &mut Vec<Handle>,
            columns: &mut Vec<Handle>,
        ) -> Result<(), Error> {
            for c in n.children.borrow().iter() {
                match node_name(c).as_deref() {
                    Some("tr") => rows.push(c.clone()),
                    Some("thead" | "tbody" | "tfoot") => {
                        check_attrs(c, &[])?;
                        gather(c, rows, columns)?;
                    }
                    Some("colgroup") => {
                        check_attrs(c, &[])?;
                        for col in c.children.borrow().iter() {
                            match node_name(col).as_deref() {
                                Some("col") => columns.push(col.clone()),
                                None if matches!(&col.data, NodeData::Text { contents } if contents.borrow().trim().is_empty()) =>
                                    {}
                                _ => return Err(err("HTML column group")),
                            }
                        }
                    }
                    None if matches!(&c.data, NodeData::Text { contents } if contents.borrow().trim().is_empty()) =>
                        {}
                    _ => return Err(err("HTML table content")),
                }
            }
            Ok(())
        }
        let mut source = Vec::new();
        let mut columns = Vec::new();
        gather(node, &mut source, &mut columns)?;
        for column in columns {
            check_attrs(&column, &["style"])?;
            let style = attribute(&column, "style").ok_or_else(|| err("HTML column width"))?;
            let (name, value) = style
                .trim()
                .trim_end_matches(';')
                .split_once(':')
                .ok_or_else(|| err("HTML column width"))?;
            if !name.trim().eq_ignore_ascii_case("width") {
                return Err(err("HTML column style"));
            }
            let width = (points_with_limit(value, 100_000.0)? * 20.0).round();
            if !(1.0..=2_000_000.0).contains(&width) {
                return Err(err("HTML column width"));
            }
            widths.push(width as u32);
        }
        let mut pending: Vec<u16> = Vec::new();
        for row in source {
            check_attrs(&row, &[])?;
            let mut cells = Vec::new();
            for c in row.children.borrow().iter() {
                if matches!(&c.data,NodeData::Text{contents} if contents.borrow().trim().is_empty())
                {
                    continue;
                }
                if !matches!(node_name(c).as_deref(), Some("td" | "th")) {
                    return Err(err("HTML table row content"));
                }
                while pending.get(cells.len()).copied().unwrap_or(0) > 0 {
                    pending[cells.len()] -= 1;
                    cells.push(self.blank_cell());
                }
                check_attrs(c, &["style", "colspan", "rowspan"])?;
                let row_span = attribute(c, "rowspan")
                    .unwrap_or_else(|| "1".into())
                    .parse::<u16>()
                    .map_err(|_| err("HTML row span"))?;
                if row_span == 0 || row_span > 256 {
                    return Err(err("HTML row span"));
                }
                let span = attribute(c, "colspan")
                    .unwrap_or_else(|| "1".into())
                    .parse::<u16>()
                    .map_err(|_| err("HTML cell span"))?;
                if span == 0 || span > 256 {
                    return Err(err("HTML cell span"));
                }
                let start = cells.len();
                let end = start.checked_add(span as usize).ok_or(Error::Limit)?;
                if end > 256 {
                    return Err(Error::Limit);
                }
                if pending.len() < end {
                    pending.resize(end, 0);
                }
                if pending[start..end].iter().any(|v| *v != 0) {
                    return Err(err("overlapping HTML cell spans"));
                }
                for entry in &mut pending[start..end] {
                    *entry = row_span - 1;
                }
                let mut fill = None;
                let mut border = None;
                let mut paragraph_css = String::new();
                if let Some(style) = attribute(c, "style") {
                    for item in style.split(';').filter(|x| !x.trim().is_empty()) {
                        let (name, value) =
                            item.split_once(':').ok_or_else(|| err("HTML cell style"))?;
                        match name.trim().to_ascii_lowercase().as_str() {
                            "background-color" => {
                                fill = if value.trim().eq_ignore_ascii_case("transparent") {
                                    None
                                } else {
                                    Some(color(value)?)
                                }
                            }
                            "border" => {
                                let parts = value.split_whitespace().collect::<Vec<_>>();
                                if parts.len() != 3 || !parts[1].eq_ignore_ascii_case("solid") {
                                    return Err(err("HTML cell border"));
                                }
                                let width = (points(parts[0])? * 20.0).round();
                                if !(1.0..=u16::MAX as f64).contains(&width) {
                                    return Err(err("HTML cell border"));
                                }
                                border = Some(Border {
                                    width_twips: width as u16,
                                    color: color(parts[2])?,
                                });
                            }
                            _ => {
                                paragraph_css.push_str(item);
                                paragraph_css.push(';');
                            }
                        }
                    }
                }
                let children = c.children.borrow();
                let has_blocks = children
                    .iter()
                    .any(|n| node_name(n).as_deref() == Some("p"));
                let mut paragraphs = Vec::new();
                if has_blocks {
                    if !paragraph_css.is_empty() {
                        return Err(err("inherited HTML cell paragraph styles"));
                    }
                    for child in children.iter() {
                        if node_name(child).as_deref() == Some("p") {
                            paragraphs.push(self.paragraph(child, None, None)?);
                        } else if !matches!(&child.data, NodeData::Text { contents } if contents.borrow().trim().is_empty())
                        {
                            return Err(err("mixed HTML cell content"));
                        }
                    }
                } else {
                    let mut text_style = TextStyle::default();
                    let mut para_style = ParagraphStyle::default();
                    css(&paragraph_css, &mut text_style, &mut para_style)?;
                    let mut runs = Vec::new();
                    for child in children.iter() {
                        self.inline(child, &text_style, &para_style, None, &mut runs)?;
                    }
                    paragraphs.push(Paragraph {
                        id: self.id(),
                        style: para_style,
                        runs,
                    });
                }
                cells.push(Cell {
                    paragraphs,
                    row_span,
                    col_span: span,
                    border,
                    fill,
                });
                for _ in 1..span {
                    cells.push(self.blank_cell());
                }
            }
            while pending.get(cells.len()).copied().unwrap_or(0) > 0 {
                pending[cells.len()] -= 1;
                cells.push(self.blank_cell());
            }
            if cells.is_empty()
                || rows
                    .first()
                    .is_some_and(|r: &Vec<Cell>| r.len() != cells.len())
            {
                return Err(err("HTML irregular table"));
            }
            rows.push(cells);
        }
        if pending.iter().any(|n| *n != 0) {
            return Err(err("HTML row span exceeds table"));
        }
        if rows.is_empty() || (!widths.is_empty() && rows[0].len() != widths.len()) {
            return Err(err("HTML table column count"));
        }
        Ok(Table {
            id: self.id(),
            rows,
            column_widths_twips: widths,
        })
    }
    fn block(&mut self, node: &Handle) -> Result<(), Error> {
        match &node.data {
            NodeData::Text { contents } if contents.borrow().trim().is_empty() => Ok(()),
            NodeData::Text { .. } => {
                let p = self.paragraph(node, None, None)?;
                self.state.blocks.push(Block::Paragraph(p));
                Ok(())
            }
            NodeData::Comment { .. } => Err(err("HTML comment")),
            NodeData::Element { name, .. } => {
                let tag = name.local.as_ref();
                match tag {
                    "html" | "body" | "div" | "main" | "article" | "section" => {
                        check_attrs(node, &[])?;
                        for child in node.children.borrow().iter() {
                            self.block(child)?
                        }
                        Ok(())
                    }
                    "head" => {
                        for c in node.children.borrow().iter() {
                            if !matches!(node_name(c).as_deref(), Some("meta"))
                                && !matches!(&c.data,NodeData::Text{contents} if contents.borrow().trim().is_empty())
                            {
                                return Err(err("HTML head metadata or CSS"));
                            }
                        }
                        Ok(())
                    }
                    "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                        if tag == "p" {
                            let children = node.children.borrow();
                            let images: Vec<_> = children
                                .iter()
                                .filter(|c| node_name(c).as_deref() == Some("img"))
                                .cloned()
                                .collect();
                            if !images.is_empty() {
                                if images.len()!=1 || children.iter().any(|c|!matches!(&c.data,NodeData::Text{contents} if contents.borrow().trim().is_empty()) && node_name(c).as_deref()!=Some("img")) || !attrs(node).is_empty() {return Err(err("inline image mixed with text"));}
                                return self.block(&images[0]);
                            }
                        }
                        let level = if tag.starts_with('h') {
                            Some(tag.as_bytes()[1] - b'1')
                        } else {
                            None
                        };
                        let p = self.paragraph(node, level, self.list.last().copied())?;
                        self.state.blocks.push(Block::Paragraph(p));
                        Ok(())
                    }
                    "ul" | "ol" => {
                        check_attrs(node, &[])?;
                        let id = self.id();
                        self.list.push((id, tag == "ol"));
                        if self.list.len() > 9 {
                            return Err(err("HTML list depth"));
                        }
                        for c in node.children.borrow().iter() {
                            if node_name(c).as_deref() == Some("li") {
                                self.list_item(c)?
                            } else if !matches!(&c.data,NodeData::Text{contents} if contents.borrow().trim().is_empty())
                            {
                                return Err(err("HTML list content"));
                            }
                        }
                        self.list.pop();
                        Ok(())
                    }
                    "table" => {
                        let table = self.table(node)?;
                        self.state.blocks.push(Block::Table(table));
                        Ok(())
                    }
                    "img" => {
                        check_attrs(node, &["src", "alt", "width", "height", "style"])?;
                        let src = attribute(node, "src").ok_or_else(|| err("HTML image source"))?;
                        let (mime, data) = if let Some((prefix, payload)) = src.split_once(',') {
                            let mime = match prefix {
                                "data:image/png;base64" => "image/png",
                                "data:image/jpeg;base64" => "image/jpeg",
                                _ => return Err(err("HTML image encoding")),
                            };
                            (
                                mime,
                                STANDARD
                                    .decode(payload)
                                    .map_err(|_| err("HTML image encoding"))?,
                            )
                        } else {
                            let resolver = self.resolver.ok_or_else(|| {
                                err("external HTML image needs a local resource resolver")
                            })?;
                            let data = resolver(&src)?;
                            let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
                                "image/png"
                            } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
                                "image/jpeg"
                            } else {
                                return Err(err("HTML image encoding"));
                            };
                            (mime, data)
                        };
                        if data.len() > MAX_IMAGE {
                            return Err(Error::Limit);
                        }
                        if (mime == "image/png" && !data.starts_with(b"\x89PNG\r\n\x1a\n"))
                            || (mime == "image/jpeg" && !data.starts_with(&[0xff, 0xd8, 0xff]))
                        {
                            return Err(err("HTML image bytes"));
                        }
                        let (source_w, source_h) =
                            image::ImageReader::new(std::io::Cursor::new(&data))
                                .with_guessed_format()
                                .map_err(|e| Error::Invalid(e.to_string()))?
                                .into_dimensions()
                                .map_err(|e| Error::Invalid(e.to_string()))?;
                        if source_w == 0
                            || source_h == 0
                            || u64::from(source_w) * u64::from(source_h) > 100_000_000
                        {
                            return Err(Error::Limit);
                        }
                        let dimension = |name: &str| -> Result<Option<u32>, Error> {
                            attribute(node, name)
                                .map(|v| {
                                    v.parse::<u32>()
                                        .map_err(|_| err("HTML image dimension"))?
                                        .checked_mul(15)
                                        .ok_or(Error::Limit)
                                })
                                .transpose()
                        };
                        let (mut w, mut h) = (dimension("width")?, dimension("height")?);
                        if let Some(style) = attribute(node, "style") {
                            for declaration in style.split(';').filter(|s| !s.trim().is_empty()) {
                                let (name, value) = declaration
                                    .split_once(':')
                                    .ok_or_else(|| err("HTML image style"))?;
                                let twips = (points_with_limit(value, u32::MAX as f64 / 20.0)?
                                    * 20.0)
                                    .round();
                                if !(1.0..=u32::MAX as f64).contains(&twips) {
                                    return Err(err("HTML image dimension"));
                                }
                                match name.trim().to_ascii_lowercase().as_str() {
                                    "width" => w = Some(twips as u32),
                                    "height" => h = Some(twips as u32),
                                    _ => return Err(err("HTML image style")),
                                }
                            }
                        }
                        let (width, height) = match (w, h) {
                            (Some(w), Some(h)) => (w, h),
                            (Some(w), None) => (
                                w,
                                u32::try_from(
                                    (u64::from(w) * u64::from(source_h) / u64::from(source_w))
                                        .max(1),
                                )
                                .map_err(|_| Error::Limit)?,
                            ),
                            (None, Some(h)) => (
                                u32::try_from(
                                    (u64::from(h) * u64::from(source_w) / u64::from(source_h))
                                        .max(1),
                                )
                                .map_err(|_| Error::Limit)?,
                                h,
                            ),
                            (None, None) => (
                                source_w.checked_mul(15).ok_or(Error::Limit)?,
                                source_h.checked_mul(15).ok_or(Error::Limit)?,
                            ),
                        };
                        let image = Image {
                            id: self.id(),
                            mime: mime.into(),
                            data,
                            width_twips: width,
                            height_twips: height,
                            alt: attribute(node, "alt").unwrap_or_default(),
                        };
                        self.state.blocks.push(Block::Image(image));
                        Ok(())
                    }
                    _ => Err(err(format!("HTML block element {tag}"))),
                }
            }
            NodeData::Document => {
                for c in node.children.borrow().iter() {
                    self.block(c)?
                }
                Ok(())
            }
            NodeData::Doctype { .. } => Ok(()),
            _ => Ok(()),
        }
    }
    fn list_item(&mut self, node: &Handle) -> Result<(), Error> {
        check_attrs(node, &["style"])?;
        let mut inline = Vec::new();
        let mut nested = Vec::new();
        for c in node.children.borrow().iter() {
            if matches!(node_name(c).as_deref(), Some("ul" | "ol")) {
                nested.push(c.clone())
            } else if node_name(c).as_deref() == Some("p") {
                if !inline.is_empty() {
                    return Err(err("complex HTML list item"));
                }
                let p = self.paragraph(c, None, self.list.last().copied())?;
                self.state.blocks.push(Block::Paragraph(p));
            } else {
                inline.push(c.clone())
            }
        }
        if !inline.is_empty() {
            let mut runs = Vec::new();
            let mut ts = TextStyle::default();
            let mut ps = ParagraphStyle::default();
            if let Some(value) = attribute(node, "style") {
                css(&value, &mut ts, &mut ps)?;
            }
            let (id, numbered) = *self.list.last().ok_or_else(|| err("HTML list"))?;
            ps.list = Some(ListMarker {
                id,
                numbered,
                level: (self.list.len() - 1) as u8,
            });
            for c in inline {
                self.inline(&c, &ts, &ps, None, &mut runs)?
            }
            let p = Paragraph {
                id: self.id(),
                style: ps,
                runs,
            };
            self.state.blocks.push(Block::Paragraph(p));
        }
        for c in nested {
            self.block(&c)?
        }
        Ok(())
    }
}
pub(super) fn decode(bytes: &[u8]) -> Result<State, Error> {
    decode_with(bytes, None)
}
pub(super) fn decode_with(
    bytes: &[u8],
    resolver: Option<&dyn Fn(&str) -> Result<Vec<u8>, Error>>,
) -> Result<State, Error> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
    let dom = parse_document(RcDom::default(), Default::default()).one(text);
    let mut r = Reader {
        resolver,
        state: State::default(),
        next: 1,
        list: vec![],
    };
    r.state.blocks.clear();
    r.block(&dom.document)?;
    if r.state.blocks.is_empty() {
        r.state.blocks.push(Block::Paragraph(Paragraph {
            id: r.next,
            style: ParagraphStyle::default(),
            runs: vec![],
        }));
        r.next += 1;
    }
    r.state.next_id = r.next;
    Ok(r.state)
}
fn fmt_color(c: u32) -> String {
    format!("#{:06x}", c & 0xffffff)
}
fn style_text(t: &TextStyle) -> Result<String, Error> {
    if t.source_style.is_some() || t.theme_color.is_some() || !t.odt_extra.is_empty() {
        return Err(err("HTML named or theme character style"));
    }
    let decoration = match (t.underline, t.strike) {
        (false, false) => "none",
        (true, false) => "underline",
        (false, true) => "line-through",
        (true, true) => "underline line-through",
    };
    let mut s = format!(
        "font-family:{};font-size:{}pt;font-weight:{};font-style:{};text-decoration:{decoration}",
        escape(&t.font),
        t.size_half_points as f64 / 2.0,
        if t.bold { "bold" } else { "normal" },
        if t.italic { "italic" } else { "normal" }
    );
    if let Some(c) = t.color {
        s.push_str(&format!(";color:{}", fmt_color(c)))
    }
    if let Some(c) = t.highlight {
        s.push_str(&format!(";background-color:{}", fmt_color(c)))
    }
    Ok(s)
}
fn paragraph_markup(p: &Paragraph, tag: &str) -> Result<String, Error> {
    if p.style.source_style.is_some()
        || !p.style.odt_extra.is_empty()
        || p.style.keep_with_next
        || p.style.keep_lines
        || !p.style.tabs_twips.is_empty()
    {
        return Err(err("HTML paragraph style metadata"));
    }
    let a = match p.style.alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "justify",
    };
    let mut line = if p.style.line_twips == 0 {
        String::new()
    } else {
        format!(";line-height:{}pt", p.style.line_twips as f64 / 20.0)
    };
    if p.style.page_break_before {
        line.push_str(";page-break-before:always");
    }
    let mut out=format!("<{tag} style=\"text-align:{a};margin-left:{}pt;margin-right:{}pt;text-indent:{}pt;margin-top:{}pt;margin-bottom:{}pt{line}\">",p.style.left_twips as f64/20.0,p.style.right_twips as f64/20.0,p.style.first_line_twips as f64/20.0,p.style.before_twips as f64/20.0,p.style.after_twips as f64/20.0);
    for r in &p.runs {
        let content = format!(
            "<span style=\"{}\">{}</span>",
            style_text(&r.style)?,
            escape(&r.text)
        );
        if let Some(link) = &r.link {
            out.push_str(&format!("<a href=\"{}\">{content}</a>", escape(link)))
        } else {
            out.push_str(&content)
        }
    }
    out.push_str(&format!("</{tag}>"));
    Ok(out)
}
pub(super) fn table_markup(table: &Table) -> Result<String, Error> {
    let mut out = String::from("<table>");
    if !table.column_widths_twips.is_empty() {
        out.push_str("<colgroup>");
        for width in &table.column_widths_twips {
            out.push_str(&format!("<col style=\"width:{}pt\">", *width as f64 / 20.0));
        }
        out.push_str("</colgroup>");
    }
    let layout = super::table_edit::table_layout(table)?;
    for (row_index, row) in table.rows.iter().enumerate() {
        out.push_str("<tr>");
        for (index, cell) in row.iter().enumerate() {
            if layout[row_index][index] != (row_index, index) {
                continue;
            }
            let mut attrs = String::new();
            if cell.col_span > 1 {
                attrs.push_str(&format!(" colspan=\"{}\"", cell.col_span));
            }
            if cell.row_span > 1 {
                attrs.push_str(&format!(" rowspan=\"{}\"", cell.row_span));
            }
            let mut styles = String::new();
            if let Some(fill) = cell.fill {
                if fill > 0xffffff {
                    return Err(err("HTML cell color"));
                }
                styles.push_str(&format!("background-color:#{fill:06x};"));
            }
            if let Some(border) = &cell.border {
                if border.width_twips == 0 || border.color > 0xffffff {
                    return Err(err("HTML cell border"));
                }
                styles.push_str(&format!(
                    "border:{}pt solid #{:06x};",
                    border.width_twips as f64 / 20.0,
                    border.color
                ));
            }
            if !styles.is_empty() {
                attrs.push_str(&format!(" style=\"{styles}\""));
            }
            out.push_str(&format!("<td{attrs}>"));
            for paragraph in &cell.paragraphs {
                if paragraph.style.list.is_some() || paragraph.style.outline_level.is_some() {
                    return Err(err("HTML table paragraph"));
                }
                out.push_str(&paragraph_markup(paragraph, "p")?);
            }
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</table>");
    Ok(out)
}
pub(super) fn encode(s: &State) -> Result<Vec<u8>, Error> {
    if s.sections.len() != 1
        || s.sections[0].page != PageSettings::default()
        || !s.sections[0].header.is_empty()
        || !s.sections[0].footer.is_empty()
        || !s.comments.is_empty()
        || !s.footnotes.is_empty()
        || !s.revisions.is_empty()
        || !s.fields.is_empty()
    {
        return Err(err("HTML would discard page or review content"));
    }
    let mut out = String::from("<!doctype html><html><head><meta charset=\"utf-8\"></head><body>");
    let mut list: Option<(u64, bool)> = None;
    for block in &s.blocks {
        let next = match block {
            Block::Paragraph(p) => p.style.list.as_ref().map(|l| (l.id, l.numbered)),
            _ => None,
        };
        if next != list {
            if let Some((_, numbered)) = list {
                out.push_str(if numbered { "</ol>" } else { "</ul>" })
            }
            if let Some((_, numbered)) = next {
                out.push_str(if numbered { "<ol>" } else { "<ul>" })
            }
            list = next;
        }
        match block {
            Block::Paragraph(p) => {
                if p.style.list.as_ref().is_some_and(|l| l.level != 0) {
                    return Err(err("nested HTML list export"));
                }
                let tag = if list.is_some() {
                    "li".to_owned()
                } else {
                    p.style
                        .outline_level
                        .map(|v| format!("h{}", v + 1))
                        .unwrap_or_else(|| "p".into())
                };
                out.push_str(&paragraph_markup(p, &tag)?);
            }
            Block::Table(t) => out.push_str(&table_markup(t)?),
            Block::Image(i) => {
                if !((i.mime == "image/png" && i.data.starts_with(b"\x89PNG\r\n\x1a\n"))
                    || (i.mime == "image/jpeg" && i.data.starts_with(&[0xff, 0xd8, 0xff])))
                {
                    return Err(err("HTML image encoding"));
                }
                out.push_str(&format!(
                    "<img src=\"data:{};base64,{}\" style=\"width:{}pt;height:{}pt\" alt=\"{}\">",
                    i.mime,
                    STANDARD.encode(&i.data),
                    i.width_twips as f64 / 20.0,
                    i.height_twips as f64 / 20.0,
                    escape(&i.alt)
                ));
            }
            _ => return Err(err("HTML section break")),
        }
    }
    if let Some((_, numbered)) = list {
        out.push_str(if numbered { "</ol>" } else { "</ul>" })
    }
    out.push_str("</body></html>");
    Ok(out.into_bytes())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn html5_styled_paragraph_list_and_table_round_trip() {
        let source=b"<!doctype html><html><body><p>Hello <strong>world</strong></p><ul><li>first</li><li>second</li></ul><table><tr><td>A</td><td>B</td></tr></table></body></html>";
        let state = decode(source).unwrap();
        assert_eq!(state.blocks.len(), 4);
        let bytes = encode(&state).unwrap();
        let reopened = decode(&bytes).unwrap();
        assert_eq!(reopened.blocks.len(), 4);
        let Block::Paragraph(p) = &reopened.blocks[0] else {
            panic!()
        };
        assert!(p.runs.iter().any(|r| r.text == "world" && r.style.bold));
        let Block::Table(t) = &reopened.blocks[3] else {
            panic!()
        };
        assert_eq!(t.rows[0].len(), 2);
    }
}
