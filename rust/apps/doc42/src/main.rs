#![cfg_attr(windows, windows_subsystem = "windows")]
use doc42_core::rich::{
    Alignment, Block, Cell, Field, FieldKind, Format, Image, ListMarker, Paragraph, ParagraphStyle,
    Position, RichDocument, Run, Selection, State, Table, TextStyle,
};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
};
use win42::{App, CommandSpec, CommandState, DialogRequest, DialogResult, FileFormat};
struct Doc {
    doc: RichDocument,
    selection: (usize, usize),
    requested_selection: Option<(usize, usize)>,
    external_url: Option<String>,
    zoom_percent: u16,
    next_revision: u64,
    snapshots: HashMap<u64, State>,
}
impl Doc {
    fn new() -> Self {
        Self {
            doc: RichDocument::new(),
            selection: (0, 0),
            requested_selection: None,
            external_url: None,
            zoom_percent: 100,
            next_revision: 1,
            snapshots: HashMap::new(),
        }
    }
    fn format(id: &str) -> Result<Format, String> {
        match id {
            "doc42" => Ok(Format::Project),
            "txt" => Ok(Format::Text),
            "rtf" => Ok(Format::Rtf),
            "docx" => Ok(Format::Docx),
            "odt" => Ok(Format::Odt),
            "html" | "htm" => Ok(Format::Html),
            "md" | "markdown" => Ok(Format::Markdown),
            _ => Err("Unsupported format".into()),
        }
    }
    fn table_anchor(
        &self,
        index: usize,
        row: usize,
        col: usize,
    ) -> Result<(u64, usize, usize), String> {
        let id = self
            .doc
            .state()
            .blocks
            .iter()
            .filter_map(|b| {
                if let Block::Table(t) = b {
                    Some(t.id)
                } else {
                    None
                }
            })
            .nth(index)
            .ok_or("Table not found")?;
        let (row, col) = self
            .doc
            .table_cell_anchor(id, row, col)
            .map_err(|e| e.to_string())?;
        Ok((id, row, col))
    }
    fn selection_style(
        &self,
    ) -> Result<(u64, std::ops::Range<usize>, TextStyle, Selection), String> {
        let start = self
            .doc
            .position_from_utf16(self.selection.0.min(self.selection.1))
            .map_err(|e| e.to_string())?;
        let end = self
            .doc
            .position_from_utf16(self.selection.0.max(self.selection.1))
            .map_err(|e| e.to_string())?;
        if start.paragraph != end.paragraph || start.offset == end.offset {
            return Err("Select text within one paragraph".into());
        }
        let p = self
            .doc
            .state()
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Paragraph(p) if p.id == start.paragraph => Some(p),
                _ => None,
            })
            .ok_or("Paragraph missing")?;
        let mut offset = 0;
        let mut style = TextStyle::default();
        for run in &p.runs {
            if start.offset < offset + run.text.len() {
                style = run.style.clone();
                break;
            }
            offset += run.text.len();
        }
        let selection = Selection {
            anchor: Some(start.clone()),
            focus: Some(end.clone()),
        };
        Ok((start.paragraph, start.offset..end.offset, style, selection))
    }
    fn apply_style(&mut self, style: TextStyle) -> Result<(), String> {
        let (p, r, _, s) = self.selection_style()?;
        self.doc
            .apply_text_style(p, r, style, s)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn ruler_context(&self) -> (usize, Option<(u64, ParagraphStyle, Selection)>) {
        let a = self.doc.position_from_utf16(self.selection.0).ok();
        let b = self.doc.position_from_utf16(self.selection.1).ok();
        if let Some(pos) = a {
            if let Some((index, p)) =
                self.doc
                    .state()
                    .blocks
                    .iter()
                    .enumerate()
                    .find_map(|(i, block)| match block {
                        Block::Paragraph(p) if p.id == pos.paragraph => Some((i, p)),
                        _ => None,
                    })
            {
                let section = self
                    .doc
                    .state()
                    .sections
                    .iter()
                    .rposition(|s| s.start_block <= index)
                    .unwrap_or(0);
                let selection = Selection {
                    anchor: Some(pos),
                    focus: b,
                };
                return (section, Some((p.id, p.style.clone(), selection)));
            }
        }
        (0, None)
    }
    fn header_input(&self, footer: bool) -> Result<String, String> {
        let (section, _) = self.ruler_context();
        let sec = &self.doc.state().sections[section];
        let paras = if footer { &sec.footer } else { &sec.header };
        if paras.len() > 1 {
            return Err("This header has multiple paragraphs".into());
        }
        let Some(p) = paras.first() else {
            return Ok(String::new());
        };
        if p.runs.iter().any(|r| r.link.is_some())
            || p.runs.windows(2).any(|w| w[0].style != w[1].style)
        {
            return Err("This header has mixed formatting".into());
        }
        let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        let mut fields = self
            .doc
            .state()
            .fields
            .iter()
            .filter(|f| f.at.paragraph == p.id)
            .collect::<Vec<_>>();
        fields.sort_by_key(|f| f.at.offset);
        let mut result = String::new();
        let mut at = 0;
        for field in fields {
            let part = text
                .get(at..field.at.offset)
                .ok_or("Invalid header field")?;
            result.push_str(&part.replace('{', "{{").replace('}', "}}"));
            result.push_str(match field.kind {
                FieldKind::PageNumber => "{PAGE}",
                FieldKind::PageCount => "{PAGES}",
                _ => return Err("Unsupported header field".into()),
            });
            at = field.at.offset;
        }
        result.push_str(&text[at..].replace('{', "{{").replace('}', "}}"));
        Ok(result)
    }
    fn set_header_input(&mut self, footer: bool, input: &str) -> Result<(), String> {
        if input.len() > 4096 || input.contains(['\r', '\n', '\0']) {
            return Err("Header/footer must be a single line up to 4096 bytes".into());
        }
        self.header_input(footer)?;
        let (section, _) = self.ruler_context();
        let mut text = String::new();
        let mut fields = Vec::new();
        let mut rest = input;
        while !rest.is_empty() {
            if let Some(next) = rest.strip_prefix("{PAGES}") {
                fields.push((text.len(), FieldKind::PageCount));
                rest = next;
            } else if let Some(next) = rest.strip_prefix("{PAGE}") {
                fields.push((text.len(), FieldKind::PageNumber));
                rest = next;
            } else if let Some(next) = rest.strip_prefix("{{") {
                text.push('{');
                rest = next;
            } else if let Some(next) = rest.strip_prefix("}}") {
                text.push('}');
                rest = next;
            } else {
                let c = rest.chars().next().unwrap();
                text.push(c);
                rest = &rest[c.len_utf8()..];
            }
        }
        self.doc
            .transact(|s| {
                let old = if footer {
                    s.sections[section].footer.clone()
                } else {
                    s.sections[section].header.clone()
                };
                let previous = old.first();
                let prior_id = previous.map(|p| p.id);
                if let Some(id) = prior_id {
                    if s.comments
                        .iter()
                        .any(|a| a.start.paragraph == id || a.end.paragraph == id)
                        || s.revisions
                            .iter()
                            .any(|a| a.start.paragraph == id || a.end.paragraph == id)
                        || s.footnotes.iter().any(|a| a.at.paragraph == id)
                    {
                        return Err(doc42_core::rich::Error::Unsupported(
                            "header contains anchored review content".into(),
                        ));
                    }
                    s.fields.retain(|f| f.at.paragraph != id);
                }
                let mut result = Vec::new();
                if !text.is_empty() || !fields.is_empty() {
                    let id = if let Some(id) = prior_id {
                        id
                    } else {
                        let id = s.next_id;
                        s.next_id = s
                            .next_id
                            .checked_add(1)
                            .ok_or(doc42_core::rich::Error::Limit)?;
                        id
                    };
                    let style = previous.map(|p| p.style.clone()).unwrap_or_default();
                    let run_style = previous
                        .and_then(|p| p.runs.first())
                        .map(|r| r.style.clone())
                        .unwrap_or_default();
                    result.push(Paragraph {
                        id,
                        style,
                        runs: vec![Run {
                            text,
                            style: run_style,
                            link: None,
                        }],
                    });
                    for (offset, kind) in fields {
                        let field_id = s.next_id;
                        s.next_id = s
                            .next_id
                            .checked_add(1)
                            .ok_or(doc42_core::rich::Error::Limit)?;
                        s.fields.push(Field {
                            id: field_id,
                            at: Position {
                                paragraph: id,
                                offset,
                            },
                            kind,
                        });
                    }
                }
                if footer {
                    s.sections[section].footer = result
                } else {
                    s.sections[section].header = result
                }
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn print_decoration(&self, paragraphs: &[Paragraph]) -> Option<Vec<win42::PrintParagraph>> {
        fn font(style: &TextStyle) -> win42::SceneFont {
            win42::SceneFont {
                family: style.font.clone(),
                points: style.size_half_points as f64 / 2.0,
                bold: style.bold,
                italic: style.italic,
                underline: style.underline,
            }
        }
        fn fragment(text: win42::PrintText, style: &TextStyle) -> win42::PrintFragment {
            let c = style.color.unwrap_or(0);
            win42::PrintFragment {
                text,
                font: font(style),
                color: [(c >> 16) as u8, (c >> 8) as u8, c as u8],
            }
        }
        let mut output = Vec::new();
        for p in paragraphs {
            let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
            let mut fields = self
                .doc
                .state()
                .fields
                .iter()
                .filter(|f| f.at.paragraph == p.id)
                .collect::<Vec<_>>();
            fields.sort_by_key(|f| f.at.offset);
            let mut fragments = Vec::new();
            let mut cursor = 0usize;
            let add_range = |from: usize,
                             to: usize,
                             out: &mut Vec<win42::PrintFragment>|
             -> Option<()> {
                let mut base = 0;
                for run in &p.runs {
                    let end = base + run.text.len();
                    let a = from.max(base);
                    let b = to.min(end);
                    if a < b {
                        out.push(fragment(
                            win42::PrintText::Literal(run.text.get(a - base..b - base)?.to_owned()),
                            &run.style,
                        ));
                    }
                    base = end;
                }
                Some(())
            };
            for field in fields {
                if field.at.offset < cursor || field.at.offset > text.len() {
                    return None;
                }
                add_range(cursor, field.at.offset, &mut fragments)?;
                let mut n = 0;
                let style = p
                    .runs
                    .iter()
                    .find_map(|r| {
                        let end = n + r.text.len();
                        let found = field.at.offset <= end;
                        n = end;
                        found.then_some(&r.style)
                    })
                    .or_else(|| p.runs.last().map(|r| &r.style))
                    .cloned()
                    .unwrap_or_default();
                let value = match field.kind {
                    FieldKind::PageNumber => win42::PrintText::PageNumber,
                    FieldKind::PageCount => win42::PrintText::PageCount,
                    _ => return None,
                };
                fragments.push(fragment(value, &style));
                cursor = field.at.offset;
            }
            add_range(cursor, text.len(), &mut fragments)?;
            let align = match p.style.alignment {
                Alignment::Left => win42::TextAlign::Left,
                Alignment::Center => win42::TextAlign::Center,
                Alignment::Right => win42::TextAlign::Right,
                Alignment::Justify => return None,
            };
            output.push(win42::PrintParagraph { align, fragments });
        }
        Some(output)
    }
    fn note_layout_supported(&self) -> bool {
        self.doc
            .state()
            .footnotes
            .iter()
            .flat_map(|n| &n.paragraphs)
            .all(|p| {
                let s = &p.style;
                s.left_twips == 0
                    && s.right_twips == 0
                    && s.first_line_twips == 0
                    && s.before_twips == 0
                    && s.after_twips == 0
                    && s.line_twips == 0
                    && s.tabs_twips.is_empty()
                    && !s.keep_lines
                    && !s.keep_with_next
                    && !s.page_break_before
                    && s.list.is_none()
                    && s.outline_level.is_none()
                    && p.runs
                        .iter()
                        .all(|r| !r.style.strike && r.style.highlight.is_none())
            })
    }
    fn rich_print(&self) -> Option<win42::RichPrint> {
        let s = self.doc.state();
        if s.sections.len() != 1
            || (!s.footnotes.is_empty() && !self.note_layout_supported())
            || self.doc.rich_edit_projection().is_err()
        {
            return None;
        }
        let section = &s.sections[0];
        let p = &section.page;
        let geometry = win42::PrintGeometry {
            width_twips: p.width_twips.try_into().ok()?,
            height_twips: p.height_twips.try_into().ok()?,
            margin_left_twips: p.margin_left.try_into().ok()?,
            margin_top_twips: p.margin_top.try_into().ok()?,
            margin_right_twips: p.margin_right.try_into().ok()?,
            margin_bottom_twips: p.margin_bottom.try_into().ok()?,
        };
        if !geometry.valid() {
            return None;
        }
        let (body_rtf, notes) = if s.footnotes.is_empty() {
            (None, Vec::new())
        } else {
            let (rtf, projected) = self.doc.note_print_projection().ok()?;
            let mut notes = Vec::new();
            for note in projected {
                notes.push(win42::RichPrintNote {
                    marker: note.marker,
                    label: note.label,
                    endnote: note.endnote,
                    paragraphs: self.print_decoration(&note.paragraphs)?,
                });
            }
            (Some(rtf), notes)
        };
        Some(win42::RichPrint {
            geometry,
            header_distance_twips: p.header_distance_twips.try_into().ok()?,
            footer_distance_twips: p.footer_distance_twips.try_into().ok()?,
            header: self.print_decoration(&section.header)?,
            footer: self.print_decoration(&section.footer)?,
            body_rtf,
            notes,
        })
    }
    fn visible_text(&self) -> String {
        self.doc.editor_text().unwrap_or_default()
    }
}
impl App for Doc {
    fn supports_production(&self) -> bool {
        true
    }
    fn label_source(&self) -> Result<win42::LabelSource, String> {
        let (start, end) = self.selection;
        let text = if end > start {
            let units: Vec<u16> = self
                .visible_text()
                .encode_utf16()
                .skip(start)
                .take((end - start).min(4096))
                .collect();
            String::from_utf16(&units).map_err(|_| "Select complete text characters.")?
        } else {
            String::new()
        };
        Ok(win42::LabelSource::Text(text))
    }
    fn name(&self) -> &'static str {
        "DOC42"
    }
    fn extension(&self) -> &'static str {
        "doc42"
    }
    fn format_name(&self) -> &'static str {
        "DOC42 project"
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[]
    }
    fn mark_file_action(&self) -> Option<u16> {
        Some(310)
    }
    fn file_actions(&self) -> &'static [win42::FileAction] {
        &[win42::FileAction {
            id: 310,
            title: "Insert image…",
            save: false,
            extension: "png",
            format: "PNG or JPEG image",
            pattern: "*.png;*.jpg;*.jpeg",
        }]
    }
    fn file_action(&mut self, id: u16, path: &Path) -> Result<(), String> {
        if id != 310 {
            return Err("Unknown file action".into());
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 16 * 1024 * 1024 {
            return Err("Image exceeds size limit".into());
        }
        let mime = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else {
            return Err("Choose a PNG or JPEG image".into());
        };
        let reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 100_000_000 {
            return Err("Image dimensions exceed limit".into());
        }
        let scale = (8640.0 / (f64::from(width) * 15.0))
            .min(11520.0 / (f64::from(height) * 15.0))
            .min(1.0);
        let w = (f64::from(width) * 15.0 * scale).round().max(1.0) as u32;
        let h = (f64::from(height) * 15.0 * scale).round().max(1.0) as u32;
        let paragraph = self
            .doc
            .position_from_utf16(self.selection.1)
            .ok()
            .map(|p| p.paragraph);
        self.doc
            .transact(|s| {
                let index = s
                    .blocks
                    .iter()
                    .position(|b| matches!(b,Block::Paragraph(p) if Some(p.id)==paragraph))
                    .map_or(s.blocks.len(), |i| i + 1);
                let id = s.next_id;
                s.next_id = s
                    .next_id
                    .checked_add(1)
                    .ok_or(doc42_core::rich::Error::Limit)?;
                s.blocks.insert(
                    index,
                    Block::Image(Image {
                        id,
                        mime: mime.into(),
                        data: bytes,
                        width_twips: w,
                        height_twips: h,
                        alt: String::new(),
                    }),
                );
                for section in &mut s.sections {
                    if section.start_block >= index && section.start_block != 0 {
                        section.start_block += 1;
                    }
                }
                s.selection = Selection::default();
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn supports_new(&self) -> bool {
        true
    }
    fn new_document(&mut self) -> Result<(), String> {
        self.doc = RichDocument::new();
        self.selection = (0, 0);
        self.snapshots.clear();
        Ok(())
    }
    fn rich_ruler(&self) -> Option<win42::RichRuler> {
        let (section, context) = self.ruler_context();
        let p = &self.doc.state().sections.get(section)?.page;
        let page = win42::PrintGeometry {
            width_twips: p.width_twips.try_into().ok()?,
            height_twips: p.height_twips.try_into().ok()?,
            margin_left_twips: p.margin_left.try_into().ok()?,
            margin_top_twips: p.margin_top.try_into().ok()?,
            margin_right_twips: p.margin_right.try_into().ok()?,
            margin_bottom_twips: p.margin_bottom.try_into().ok()?,
        };
        let paragraph = context.map(|(_, p, _)| win42::ParagraphRuler {
            left_twips: p.left_twips,
            right_twips: p.right_twips,
            first_line_twips: p.first_line_twips,
            tabs_twips: p.tabs_twips,
        });
        Some(win42::RichRuler {
            page,
            zoom_percent: self.zoom_percent,
            paragraph,
        })
    }
    fn rich_ruler_action(&mut self, action: win42::RichRulerAction) -> Result<(), String> {
        use win42::RichRulerAction::*;
        let (section, context) = self.ruler_context();
        let page = self.doc.state().sections[section].page.clone();
        match action {
            LeftMargin(v) | RightMargin(v) | TopMargin(v) | BottomMargin(v) => {
                let value = u32::try_from(v).map_err(|_| "Margin cannot be negative")?;
                let mut next = page.clone();
                match action {
                    LeftMargin(_) => next.margin_left = value,
                    RightMargin(_) => next.margin_right = value,
                    TopMargin(_) => next.margin_top = value,
                    BottomMargin(_) => next.margin_bottom = value,
                    _ => unreachable!(),
                };
                let horizontal = u64::from(next.margin_left)
                    + u64::from(next.margin_right)
                    + u64::from(next.column_gap) * u64::from(next.columns.saturating_sub(1))
                    + 720 * u64::from(next.columns);
                if horizontal > u64::from(next.width_twips)
                    || u64::from(next.margin_top) + u64::from(next.margin_bottom) + 720
                        > u64::from(next.height_twips)
                {
                    return Err("Margins must leave at least 36 points of usable space".into());
                }
                let selection = context.map(|(_, _, s)| s).unwrap_or_default();
                self.doc
                    .transact(|s| {
                        s.sections[section].page = next;
                        s.selection = selection;
                        Ok(())
                    })
                    .map_err(|e| e.to_string())?;
            }
            LeftIndent(v)
            | RightIndent(v)
            | FirstIndent(v)
            | AddTab(v)
            | MoveTab { to: v, .. }
            | RemoveTab(v) => {
                let (id, mut style, selection) = context.ok_or("Place the caret in a paragraph")?;
                let body = i64::from(page.width_twips)
                    - i64::from(page.margin_left)
                    - i64::from(page.margin_right)
                    - i64::from(page.column_gap) * i64::from(page.columns.saturating_sub(1));
                let body = body / i64::from(page.columns.max(1));
                match action {
                    LeftIndent(_) => style.left_twips = v,
                    RightIndent(_) => style.right_twips = v,
                    FirstIndent(_) => style.first_line_twips = v,
                    AddTab(_) => {
                        if i64::from(v) < 0 || i64::from(v) > body {
                            return Err("Tab stop is outside the text area".into());
                        }
                        style.tabs_twips.push(v);
                    }
                    MoveTab { from, .. } => {
                        if i64::from(v) < 0 || i64::from(v) > body {
                            return Err("Tab stop is outside the text area".into());
                        }
                        let index = style
                            .tabs_twips
                            .iter()
                            .position(|n| *n == from)
                            .ok_or("Tab stop no longer exists")?;
                        style.tabs_twips[index] = v;
                    }
                    RemoveTab(_) => {
                        style.tabs_twips.retain(|n| *n != v);
                    }
                    _ => unreachable!(),
                }
                if i64::from(style.left_twips) < -i64::from(page.margin_left)
                    || i64::from(style.right_twips) < -i64::from(page.margin_right)
                    || body - i64::from(style.left_twips) - i64::from(style.right_twips) < 720
                    || i64::from(style.left_twips) + i64::from(style.first_line_twips)
                        < -i64::from(page.margin_left)
                    || i64::from(style.left_twips) + i64::from(style.first_line_twips)
                        > body - i64::from(style.right_twips) - 720
                {
                    return Err("Indents must leave at least 36 points of text width".into());
                }
                style.tabs_twips.sort_unstable();
                style.tabs_twips.dedup();
                if style.tabs_twips.len() > 64 {
                    return Err("Too many tab stops".into());
                }
                self.doc
                    .set_paragraph_style(id, style, selection)
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
    fn uses_rich_edit(&self) -> bool {
        true
    }
    fn rich_text(&self) -> Option<Vec<u8>> {
        self.doc.rich_edit_projection().ok()
    }
    fn edit_rich_text(&mut self, bytes: &[u8], selection: (usize, usize)) -> Result<(), String> {
        self.doc
            .apply_rich_edit_projection(bytes, selection)
            .map_err(|e| e.to_string())?;
        self.selection = selection;
        Ok(())
    }
    fn take_requested_rich_selection(&mut self) -> Option<(usize, usize)> {
        self.requested_selection.take()
    }
    fn rich_selection_to_restore(&self) -> Option<(usize, usize)> {
        let selection = &self.doc.state().selection;
        Some((
            self.doc
                .position_to_utf16(selection.anchor.as_ref()?)
                .ok()?,
            self.doc.position_to_utf16(selection.focus.as_ref()?).ok()?,
        ))
    }
    fn set_rich_selection(&mut self, s: (usize, usize)) {
        self.selection = s;
    }
    fn open_formats(&self) -> Vec<FileFormat> {
        self.save_formats()
    }
    fn save_formats(&self) -> Vec<FileFormat> {
        vec![
            FileFormat {
                id: "doc42",
                title: "DOC42 project",
                extensions: &["doc42"],
            },
            FileFormat {
                id: "docx",
                title: "Word document (supported subset)",
                extensions: &["docx"],
            },
            FileFormat {
                id: "rtf",
                title: "Rich Text Format (supported subset)",
                extensions: &["rtf"],
            },
            FileFormat {
                id: "odt",
                title: "OpenDocument text",
                extensions: &["odt"],
            },
            FileFormat {
                id: "html",
                title: "HTML",
                extensions: &["html", "htm"],
            },
            FileFormat {
                id: "md",
                title: "Markdown",
                extensions: &["md", "markdown"],
            },
            FileFormat {
                id: "txt",
                title: "UTF-8 text",
                extensions: &["txt"],
            },
        ]
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        let snapshot = self.doc.snapshot();
        let bytes = RichDocument::encode_snapshot(snapshot.clone(), Self::format(format)?)
            .map_err(|e| e.to_string())?;
        // The native host publishes synchronously; discard abandoned attempts.
        self.snapshots.clear();
        let revision = self.next_revision;
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or("Revision overflow")?;
        self.snapshots.insert(revision, snapshot);
        Ok(Some((revision, bytes)))
    }
    fn save_bundle_to(
        &mut self,
        format: &str,
        path: &Path,
        is_current: bool,
    ) -> Result<Option<(u64, suite42_core::session::SaveBundle)>, String> {
        if Self::format(format)? != Format::Markdown {
            return Ok(self
                .save_snapshot_to(format, path, is_current)?
                .map(|(revision, main)| {
                    (
                        revision,
                        suite42_core::session::SaveBundle {
                            main,
                            sidecars: Vec::new(),
                        },
                    )
                }));
        }
        let snapshot = self.doc.snapshot();
        let bundle = self.doc.markdown_bundle(path).map_err(|e| e.to_string())?;
        self.snapshots.clear();
        let revision = self.next_revision;
        self.next_revision = self
            .next_revision
            .checked_add(1)
            .ok_or("Revision overflow")?;
        self.snapshots.insert(revision, snapshot);
        Ok(Some((revision, bundle)))
    }
    fn saved_snapshot(&mut self, revision: u64) {
        if let Some(snapshot) = self.snapshots.remove(&revision) {
            let _ = self.doc.mark_saved_snapshot(snapshot);
        }
        self.snapshots.retain(|id, _| *id > revision);
    }
    fn command_specs(&self) -> Vec<CommandSpec> {
        [
            (300, "Bold", Some("Ctrl+B")),
            (301, "Italic", Some("Ctrl+I")),
            (302, "Underline", Some("Ctrl+U")),
            (303, "Font…", None),
            (304, "Text color…", None),
            (305, "Align left", Some("Ctrl+L")),
            (306, "Center", Some("Ctrl+E")),
            (307, "Align right", Some("Ctrl+R")),
            (308, "Justify", Some("Ctrl+J")),
            (309, "Page setup…", None),
            (311, "Bulleted list", None),
            (312, "Numbered list", None),
            (313, "Insert table…", None),
            (314, "Paragraph spacing…", None),
            (315, "Print…", Some("Ctrl+P")),
            (316, "Print layout preview…", None),
            (317, "Zoom in", None),
            (318, "Zoom out", None),
            (319, "Edit header…", None),
            (320, "Edit footer…", None),
            (321, "Add table row…", None),
            (322, "Add table column…", None),
            (323, "Image size and description…", None),
            (324, "Delete table row…", None),
            (325, "Delete table column…", None),
            (326, "Heading level…", None),
            (327, "Table cell alignment…", None),
            (328, "Delete table…", None),
            (329, "Merge cells across row…", None),
            (330, "Split merged cell…", None),
            (331, "Table cell background…", None),
            (332, "Table cell border…", None),
            (333, "Table column width…", None),
            (334, "Merge rectangular cells…", None),
            (335, "Add comment…", None),
            (336, "Edit comment…", None),
            (337, "Delete comment…", None),
            (338, "List comments…", None),
            (339, "Go to comment…", None),
            (340, "Insert footnote…", None),
            (341, "Insert endnote…", None),
            (342, "Edit note…", None),
            (343, "Delete note…", None),
            (344, "List notes…", None),
            (345, "Go to note…", None),
            (346, "Insert table of contents", None),
            (347, "Update table of contents", None),
            (348, "Remove table of contents", None),
            (349, "Go to TOC heading", None),
            (350, "Insert or edit link…", None),
            (351, "Remove link", None),
            (352, "Add bookmark…", None),
            (353, "Go to bookmark…", None),
            (354, "Follow link", None),
            (355, "Delete bookmark…", None),
        ]
        .into_iter()
        .map(|(id, title, accelerator)| CommandSpec {
            id,
            title,
            accelerator,
            group: "Format",
        })
        .collect()
    }
    fn command_state(&self, id: u16) -> CommandState {
        if (346..=349).contains(&id) {
            let at = self.doc.position_from_utf16(self.selection.0).ok();
            return CommandState {
                enabled: match id {
                    346 => self.doc.state().blocks.iter().any(
                        |b| matches!(b, Block::Paragraph(p) if p.style.outline_level.is_some()),
                    ),
                    349 => at
                        .as_ref()
                        .and_then(|p| self.doc.toc_target(p.paragraph))
                        .is_some(),
                    _ => !self.doc.state().tables_of_contents.is_empty(),
                },
                checked: false,
            };
        }
        if (340..=345).contains(&id) {
            return CommandState {
                enabled: id == 340 || id == 341 || !self.doc.state().footnotes.is_empty(),
                checked: false,
            };
        }
        if (335..=339).contains(&id) {
            return CommandState {
                enabled: id == 335 || !self.doc.state().comments.is_empty(),
                checked: false,
            };
        }
        if id == 319 || id == 320 {
            return CommandState {
                enabled: self.header_input(id == 320).is_ok(),
                checked: false,
            };
        }
        if id == 317 || id == 318 {
            return CommandState {
                enabled: if id == 317 {
                    self.zoom_percent < 200
                } else {
                    self.zoom_percent > 25
                },
                checked: false,
            };
        }
        if id == 315 || id == 316 {
            return CommandState {
                enabled: self.rich_print().is_some(),
                checked: false,
            };
        }
        if id == 309
            || id == 310
            || id == 313
            || (321..=325).contains(&id)
            || id == 327
            || id == 328
        {
            return CommandState {
                enabled: true,
                checked: false,
            };
        }
        let style = self.selection_style().ok().map(|x| x.2);
        let paragraph = self
            .doc
            .position_from_utf16(self.selection.0)
            .ok()
            .and_then(|pos| {
                self.doc.state().blocks.iter().find_map(|b| match b {
                    Block::Paragraph(p) if p.id == pos.paragraph => Some(p),
                    _ => None,
                })
            });
        if id == 314 || id == 326 {
            return CommandState {
                enabled: paragraph.is_some(),
                checked: false,
            };
        }
        if (311..=312).contains(&id) {
            return CommandState {
                enabled: paragraph.is_some(),
                checked: paragraph
                    .and_then(|p| p.style.list.as_ref())
                    .is_some_and(|l| l.numbered == (id == 312)),
            };
        }
        if (305..=308).contains(&id) {
            return CommandState {
                enabled: paragraph.is_some(),
                checked: paragraph.is_some_and(|p| {
                    p.style.alignment
                        == match id {
                            305 => Alignment::Left,
                            306 => Alignment::Center,
                            307 => Alignment::Right,
                            _ => Alignment::Justify,
                        }
                }),
            };
        }
        CommandState {
            enabled: style.is_some(),
            checked: style.is_some_and(|s| match id {
                300 => s.bold,
                301 => s.italic,
                302 => s.underline,
                _ => false,
            }),
        }
    }
    fn multiline_input(&self, id: u16) -> bool {
        matches!(id, 335 | 336 | 338 | 340 | 341 | 342 | 344)
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        if id == 350 {
            return Some(win42::InputRequest {
                title: "Insert or edit link",
                fields: vec![(
                    "Address (https://, http://, mailto:, or #bookmark)",
                    String::new(),
                )],
            });
        }
        if matches!(id, 352 | 353 | 355) {
            return Some(win42::InputRequest {
                title: match id {
                    352 => "Add bookmark",
                    353 => "Go to bookmark",
                    _ => "Delete bookmark",
                },
                fields: vec![("Bookmark name", String::new())],
            });
        }
        if id == 340 || id == 341 {
            return Some(win42::InputRequest {
                title: if id == 340 {
                    "Insert footnote"
                } else {
                    "Insert endnote"
                },
                fields: vec![("Note text", String::new())],
            });
        }
        if id == 342 {
            let first = self.doc.state().footnotes.first()?;
            let text = first
                .paragraphs
                .iter()
                .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                .collect::<Vec<_>>()
                .join("\r\n");
            return Some(win42::InputRequest {
                title: "Edit note",
                fields: vec![("Note text", text), ("Note number", "1".into())],
            });
        }
        if id == 343 || id == 345 {
            return Some(win42::InputRequest {
                title: if id == 343 {
                    "Delete note"
                } else {
                    "Go to note anchor"
                },
                fields: vec![("Note number", "1".into())],
            });
        }
        if id == 344 {
            let list = self
                .doc
                .state()
                .footnotes
                .iter()
                .enumerate()
                .map(|(i, n)| {
                    format!(
                        "{}: {} — {}",
                        i + 1,
                        if n.endnote { "Endnote" } else { "Footnote" },
                        n.paragraphs
                            .iter()
                            .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()))
                            .collect::<String>()
                    )
                })
                .collect::<Vec<_>>()
                .join("\r\n");
            return Some(win42::InputRequest {
                title: "Notes",
                fields: vec![("Notes", list)],
            });
        }
        if id == 335 {
            return Some(win42::InputRequest {
                title: "Add comment to selection",
                fields: vec![("Comment", String::new()), ("Author", String::new())],
            });
        }
        if id == 336 {
            let first = self.doc.state().comments.first()?;
            return Some(win42::InputRequest {
                title: "Edit comment",
                fields: vec![
                    ("Text", first.text.clone()),
                    ("Comment number", "1".into()),
                    (
                        "Resolved (yes/no)",
                        if first.resolved { "yes" } else { "no" }.into(),
                    ),
                ],
            });
        }
        if id == 337 || id == 339 {
            return Some(win42::InputRequest {
                title: if id == 337 {
                    "Delete comment"
                } else {
                    "Go to comment"
                },
                fields: vec![("Comment number", "1".into())],
            });
        }
        if id == 338 {
            let list = self
                .doc
                .state()
                .comments
                .iter()
                .enumerate()
                .map(|(i, c)| {
                    format!(
                        "{}: {} — {}{}",
                        i + 1,
                        c.author,
                        c.text.replace(['\r', '\n'], " "),
                        if c.resolved { " (resolved)" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join("\r\n");
            return Some(win42::InputRequest {
                title: "Comments",
                fields: vec![("Comments", list)],
            });
        }
        if id == 319 || id == 320 {
            Some(win42::InputRequest {
                title: if id == 319 {
                    "Header ({PAGE}, {PAGES})"
                } else {
                    "Footer ({PAGE}, {PAGES})"
                },
                fields: vec![("Text", self.header_input(id == 320).ok()?)],
            })
        } else if (321..=322).contains(&id) || (324..=325).contains(&id) {
            Some(win42::InputRequest {
                title: match id {
                    321 => "Add table row",
                    322 => "Add table column",
                    324 => "Delete table row",
                    _ => "Delete table column",
                },
                fields: vec![
                    ("Table number", "1".into()),
                    ("Position (starts at 1)", "1".into()),
                ],
            })
        } else if id == 323 {
            Some(win42::InputRequest {
                title: "Image size (points)",
                fields: vec![
                    ("Image number", "1".into()),
                    ("Width", "144".into()),
                    ("Height", "144".into()),
                    ("Description", String::new()),
                ],
            })
        } else if id == 313 {
            Some(win42::InputRequest {
                title: "Insert table",
                fields: vec![("Rows", "2".into()), ("Columns", "2".into())],
            })
        } else if id == 334 {
            Some(win42::InputRequest {
                title: "Merge cells (uses top-left cell style)",
                fields: vec![
                    ("Table number", "1".into()),
                    ("First row", "1".into()),
                    ("First column", "1".into()),
                    ("Last row", "2".into()),
                    ("Last column", "2".into()),
                ],
            })
        } else if id == 333 {
            Some(win42::InputRequest {
                title: "Table column width (points)",
                fields: vec![
                    ("Table number", "1".into()),
                    ("Column", "1".into()),
                    ("Width", "72".into()),
                ],
            })
        } else if id == 332 {
            Some(win42::InputRequest {
                title: "Cell border (0 removes; width in points)",
                fields: vec![
                    ("Table number", "1".into()),
                    ("Row", "1".into()),
                    ("Column", "1".into()),
                    ("Width (0.25 pt steps)", "1".into()),
                    ("Color (#RRGGBB)", "#000000".into()),
                ],
            })
        } else if id == 331 {
            Some(win42::InputRequest {
                title: "Cell background (#RRGGBB or none)",
                fields: vec![
                    ("Table number", "1".into()),
                    ("Row", "1".into()),
                    ("Column", "1".into()),
                    ("Color", "#ffffff".into()),
                ],
            })
        } else if id == 329 || id == 330 {
            let mut fields = vec![
                ("Table number", "1".into()),
                ("Row", "1".into()),
                (
                    if id == 329 {
                        "First column"
                    } else {
                        "Merged cell column"
                    },
                    "1".into(),
                ),
            ];
            if id == 329 {
                fields.push(("Last column", "2".into()));
            }
            Some(win42::InputRequest {
                title: if id == 329 {
                    "Merge cells (uses first cell's background and border)"
                } else {
                    "Split merged cell"
                },
                fields,
            })
        } else if id == 328 {
            Some(win42::InputRequest {
                title: "Delete table",
                fields: vec![("Table number", "1".into())],
            })
        } else if id == 327 {
            Some(win42::InputRequest {
                title: "Cell alignment (left, center, right, justify)",
                fields: vec![
                    ("Table number", "1".into()),
                    ("Row", "1".into()),
                    ("Column", "1".into()),
                    ("Alignment", "left".into()),
                ],
            })
        } else if id == 326 {
            let pos = self.doc.position_from_utf16(self.selection.0).ok()?;
            let p = self.doc.state().blocks.iter().find_map(|b| match b {
                Block::Paragraph(p) if p.id == pos.paragraph => Some(p),
                _ => None,
            })?;
            Some(win42::InputRequest {
                title: "Heading level (0 = body, 1–9 = heading)",
                fields: vec![(
                    "Level",
                    p.style.outline_level.map_or(0, |n| n + 1).to_string(),
                )],
            })
        } else if id == 314 {
            let pos = self.doc.position_from_utf16(self.selection.0).ok()?;
            let p = self.doc.state().blocks.iter().find_map(|b| match b {
                Block::Paragraph(p) if p.id == pos.paragraph => Some(p),
                _ => None,
            })?;
            Some(win42::InputRequest {
                title: "Paragraph spacing (points)",
                fields: vec![
                    ("Before", format!("{}", p.style.before_twips as f64 / 20.0)),
                    ("After", format!("{}", p.style.after_twips as f64 / 20.0)),
                    ("Line", format!("{}", p.style.line_twips as f64 / 20.0)),
                ],
            })
        } else {
            None
        }
    }
    fn input_action(&mut self, id: u16, values: &[String]) -> Result<(), String> {
        if id == 350 {
            let target = values.first().ok_or("Missing link target")?.trim();
            if !target.is_ascii() {
                return Err("Use an ASCII or percent-encoded link address".into());
            }
            if target.len() > 4096
                || target.chars().any(|c| c.is_control() || c.is_whitespace())
                || !(target.starts_with("https://")
                    || target.starts_with("http://")
                    || target.starts_with("mailto:")
                    || target.starts_with('#'))
            {
                return Err("Use an http, https, mailto, or #bookmark address".into());
            }
            let (p, range, _, selection) = self.selection_style()?;
            self.doc
                .set_link(p, range, Some(target.into()), selection)
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if matches!(id, 352 | 353 | 355) {
            let name = values.first().ok_or("Missing bookmark name")?.trim();
            if name.is_empty() {
                return Err("Enter a bookmark name".into());
            }
            if id == 352 {
                let start = self
                    .doc
                    .position_from_utf16(self.selection.0.min(self.selection.1))
                    .map_err(|e| e.to_string())?;
                let end = self
                    .doc
                    .position_from_utf16(self.selection.0.max(self.selection.1))
                    .map_err(|e| e.to_string())?;
                self.doc
                    .add_bookmark(name.into(), start, end)
                    .map_err(|e| e.to_string())?;
            } else if id == 355 {
                self.doc.remove_bookmark(name).map_err(|e| e.to_string())?;
            } else {
                let bookmark = self
                    .doc
                    .state()
                    .bookmarks
                    .iter()
                    .find(|b| b.name == name)
                    .ok_or("Bookmark not found")?;
                let offset = self
                    .doc
                    .position_to_utf16(&bookmark.start)
                    .map_err(|e| e.to_string())?;
                self.selection = (offset, offset);
                self.requested_selection = Some((offset, offset));
            }
            return Ok(());
        }
        if id == 340 || id == 341 {
            if values.len() != 1 {
                return Err("Invalid note".into());
            }
            let at = self
                .doc
                .position_from_utf16(self.selection.0.min(self.selection.1))
                .map_err(|e| e.to_string())?;
            self.doc
                .add_note(at, values[0].clone(), id == 341)
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 344 {
            return Ok(());
        }
        if matches!(id, 342 | 343 | 345) {
            let expected = if id == 342 { 2 } else { 1 };
            if values.len() != expected {
                return Err("Invalid note selection".into());
            }
            let index = values[if id == 342 { 1 } else { 0 }]
                .parse::<usize>()
                .map_err(|_| "Invalid note number")?
                .checked_sub(1)
                .ok_or("Numbers start at 1")?;
            let note = self
                .doc
                .state()
                .footnotes
                .get(index)
                .cloned()
                .ok_or("Note not found")?;
            match id {
                342 => {
                    self.doc
                        .edit_note(note.id, values[0].clone())
                        .map_err(|e| e.to_string())?;
                }
                343 => {
                    self.doc.delete_note(note.id).map_err(|e| e.to_string())?;
                }
                _ => {
                    let pos = self
                        .doc
                        .position_to_utf16(&note.at)
                        .map_err(|e| e.to_string())?;
                    self.selection = (pos, pos);
                    self.requested_selection = Some((pos, pos));
                }
            }
            return Ok(());
        }
        if id == 335 {
            if values.len() != 2 {
                return Err("Invalid comment".into());
            }
            let first = self.selection.0.min(self.selection.1);
            let last = self.selection.0.max(self.selection.1);
            let start = self
                .doc
                .position_from_utf16(first)
                .map_err(|e| e.to_string())?;
            let end = self
                .doc
                .position_from_utf16(last)
                .map_err(|e| e.to_string())?;
            self.doc
                .add_comment(start, end, values[1].clone(), values[0].clone())
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 338 {
            return Ok(());
        }
        if matches!(id, 336 | 337 | 339) {
            let required = if id == 336 { 3 } else { 1 };
            if values.len() != required {
                return Err("Invalid comment selection".into());
            }
            let index = values[if id == 336 { 1 } else { 0 }]
                .parse::<usize>()
                .map_err(|_| "Invalid comment number")?
                .checked_sub(1)
                .ok_or("Numbers start at 1")?;
            let comment = self
                .doc
                .state()
                .comments
                .get(index)
                .cloned()
                .ok_or("Comment not found")?;
            match id {
                336 => {
                    let resolved = match values[2].trim().to_ascii_lowercase().as_str() {
                        "yes" => true,
                        "no" => false,
                        _ => return Err("Enter yes or no".into()),
                    };
                    self.doc
                        .edit_comment(comment.id, values[0].clone(), resolved)
                        .map_err(|e| e.to_string())?;
                }
                337 => {
                    self.doc
                        .delete_comment(comment.id)
                        .map_err(|e| e.to_string())?;
                }
                _ => {
                    let start = self
                        .doc
                        .position_to_utf16(&comment.start)
                        .map_err(|e| e.to_string())?;
                    let end = self
                        .doc
                        .position_to_utf16(&comment.end)
                        .map_err(|e| e.to_string())?;
                    self.selection = (start, end);
                    self.requested_selection = Some((start, end));
                }
            }
            return Ok(());
        }
        if id == 319 || id == 320 {
            if values.len() != 1 {
                return Err("Invalid header/footer input".into());
            }
            return self.set_header_input(id == 320, &values[0]);
        }
        if id == 323 {
            if values.len() != 4 {
                return Err("Invalid image size".into());
            }
            let n = values[0]
                .parse::<usize>()
                .map_err(|_| "Invalid image number")?;
            let parse = |v: &str| -> Result<u32, String> {
                let x = v.parse::<f64>().map_err(|_| "Invalid image size")?;
                if !x.is_finite() || !(1.0..=10000.0).contains(&x) {
                    return Err("Image size must be 1–10000 points".into());
                }
                Ok((x * 20.0).round() as u32)
            };
            let (width, height) = (parse(&values[1])?, parse(&values[2])?);
            if n == 0 {
                return Err("Image numbers start at 1".into());
            }
            self.doc
                .transact(|s| {
                    let image = s
                        .blocks
                        .iter_mut()
                        .filter_map(|b| {
                            if let Block::Image(i) = b {
                                Some(i)
                            } else {
                                None
                            }
                        })
                        .nth(n - 1)
                        .ok_or(doc42_core::rich::Error::Invalid("Image not found".into()))?;
                    image.width_twips = width;
                    image.height_twips = height;
                    image.alt = values[3].clone();
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if (321..=322).contains(&id) || (324..=325).contains(&id) {
            if values.len() != 2 {
                return Err("Invalid table position".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let (table, position) = (parse(&values[0])?, parse(&values[1])?);
            let table_id = self
                .doc
                .state()
                .blocks
                .iter()
                .filter_map(|b| {
                    if let Block::Table(t) = b {
                        Some(t.id)
                    } else {
                        None
                    }
                })
                .nth(table)
                .ok_or("Table not found")?;
            self.doc
                .edit_table_structure(
                    table_id,
                    id == 321 || id == 324,
                    id == 321 || id == 322,
                    position,
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 334 {
            if values.len() != 5 {
                return Err("Invalid merge selection".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let index = parse(&values[0])?;
            let id = self
                .doc
                .state()
                .blocks
                .iter()
                .filter_map(|b| {
                    if let Block::Table(t) = b {
                        Some(t.id)
                    } else {
                        None
                    }
                })
                .nth(index)
                .ok_or("Table not found")?;
            self.doc
                .merge_table_rectangle(
                    id,
                    parse(&values[1])?,
                    parse(&values[2])?,
                    parse(&values[3])?,
                    parse(&values[4])?,
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 333 {
            if values.len() != 3 {
                return Err("Invalid column width".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let (table, col) = (parse(&values[0])?, parse(&values[1])?);
            let width = values[2]
                .parse::<f64>()
                .map_err(|_| "Invalid column width")?;
            if !width.is_finite() || !(1.0..=10000.0).contains(&width) {
                return Err("Width must be 1–10000 points".into());
            }
            self.doc
                .transact(|s| {
                    let t = s
                        .blocks
                        .iter_mut()
                        .filter_map(|b| {
                            if let Block::Table(t) = b {
                                Some(t)
                            } else {
                                None
                            }
                        })
                        .nth(table)
                        .ok_or(doc42_core::rich::Error::Invalid("Table not found".into()))?;
                    let count = t.rows.first().map_or(0, Vec::len);
                    if col >= count {
                        return Err(doc42_core::rich::Error::Invalid(
                            "Table column not found".into(),
                        ));
                    }
                    if t.column_widths_twips.is_empty() {
                        t.column_widths_twips = vec![1440; count];
                    }
                    t.column_widths_twips[col] = (width * 20.0).round() as u32;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 332 {
            if values.len() != 5 {
                return Err("Invalid cell border".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let (table, row, col) = (parse(&values[0])?, parse(&values[1])?, parse(&values[2])?);
            let width = values[3]
                .parse::<f64>()
                .map_err(|_| "Invalid border width")?;
            if !width.is_finite()
                || !(0.0..=3276.75).contains(&width)
                || (width * 4.0).fract().abs() > 0.000001
            {
                return Err("Use a width in 0.25 point steps".into());
            }
            let raw = values[4].trim().strip_prefix('#').ok_or("Use #RRGGBB")?;
            if raw.len() != 6 {
                return Err("Use #RRGGBB".into());
            }
            let color = u32::from_str_radix(raw, 16).map_err(|_| "Use #RRGGBB")?;
            let border = if width == 0.0 {
                None
            } else {
                Some(doc42_core::rich::Border {
                    width_twips: (width * 20.0).round() as u16,
                    color,
                })
            };
            let (table_id, row, col) = self.table_anchor(table, row, col)?;
            self.doc
                .transact(|s| {
                    let target = s
                        .blocks
                        .iter_mut()
                        .find_map(|b| match b {
                            Block::Table(t) if t.id == table_id => Some(&mut t.rows[row][col]),
                            _ => None,
                        })
                        .ok_or(doc42_core::rich::Error::Invalid("Table not found".into()))?;
                    target.border = border;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 331 {
            if values.len() != 4 {
                return Err("Invalid cell background".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let (table, row, col) = (parse(&values[0])?, parse(&values[1])?, parse(&values[2])?);
            let value = values[3].trim();
            let fill = if value.eq_ignore_ascii_case("none") {
                None
            } else {
                let hex = value.strip_prefix('#').ok_or("Use #RRGGBB or none")?;
                if hex.len() != 6 {
                    return Err("Use #RRGGBB or none".into());
                }
                Some(u32::from_str_radix(hex, 16).map_err(|_| "Use #RRGGBB or none")?)
            };
            let (table_id, row, col) = self.table_anchor(table, row, col)?;
            self.doc
                .transact(|s| {
                    let target = s
                        .blocks
                        .iter_mut()
                        .find_map(|b| match b {
                            Block::Table(t) if t.id == table_id => Some(&mut t.rows[row][col]),
                            _ => None,
                        })
                        .ok_or(doc42_core::rich::Error::Invalid("Table not found".into()))?;
                    target.fill = fill;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 329 || id == 330 {
            if values.len() != if id == 329 { 4 } else { 3 } {
                return Err("Invalid merge selection".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?
                    .checked_sub(1)
                    .ok_or_else(|| "Numbers start at 1".into())
            };
            let (index, row, col) = (parse(&values[0])?, parse(&values[1])?, parse(&values[2])?);
            let id_table = self
                .doc
                .state()
                .blocks
                .iter()
                .filter_map(|b| {
                    if let Block::Table(t) = b {
                        Some(t.id)
                    } else {
                        None
                    }
                })
                .nth(index)
                .ok_or("Table not found")?;
            if id == 329 {
                self.doc
                    .merge_table_cells(id_table, row, col, parse(&values[3])?)
                    .map_err(|e| e.to_string())?;
            } else {
                self.doc
                    .split_table_cell(id_table, row, col)
                    .map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        if id == 328 {
            if values.len() != 1 {
                return Err("Invalid table number".into());
            }
            let n = values[0]
                .parse::<usize>()
                .map_err(|_| "Invalid table number")?;
            if n == 0 {
                return Err("Table numbers start at 1".into());
            }
            self.doc
                .transact(|s| {
                    let index = s
                        .blocks
                        .iter()
                        .enumerate()
                        .filter(|(_, b)| matches!(b, Block::Table(_)))
                        .nth(n - 1)
                        .map(|(i, _)| i)
                        .ok_or(doc42_core::rich::Error::Invalid("Table not found".into()))?;
                    if s.blocks.len() == 1
                        || s.sections.iter().any(|section| {
                            section.start_block == index
                                && index != 0
                                && index + 1 == s.blocks.len()
                        })
                    {
                        return Err(doc42_core::rich::Error::Unsupported(
                            "Cannot delete the only block in a section".into(),
                        ));
                    }
                    s.blocks.remove(index);
                    for section in &mut s.sections {
                        if section.start_block > index {
                            section.start_block -= 1;
                        }
                    }
                    s.selection = Selection::default();
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 327 {
            if values.len() != 4 {
                return Err("Invalid cell alignment".into());
            }
            let parse = |value: &str| -> Result<usize, String> {
                let n = value
                    .parse::<usize>()
                    .map_err(|_| "Invalid table position")?;
                n.checked_sub(1).ok_or_else(|| "Numbers start at 1".into())
            };
            let (table, row, col) = (parse(&values[0])?, parse(&values[1])?, parse(&values[2])?);
            let alignment = match values[3].trim().to_ascii_lowercase().as_str() {
                "left" => Alignment::Left,
                "center" => Alignment::Center,
                "right" => Alignment::Right,
                "justify" => Alignment::Justify,
                _ => return Err("Choose left, center, right or justify".into()),
            };
            let (table_id, row, col) = self.table_anchor(table, row, col)?;
            self.doc
                .transact(|s| {
                    let cell = s
                        .blocks
                        .iter_mut()
                        .find_map(|b| match b {
                            Block::Table(t) if t.id == table_id => Some(&mut t.rows[row][col]),
                            _ => None,
                        })
                        .ok_or(doc42_core::rich::Error::Invalid("Table not found".into()))?;
                    for p in &mut cell.paragraphs {
                        p.style.alignment = alignment;
                    }
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 326 {
            if values.len() != 1 {
                return Err("Invalid heading level".into());
            }
            let level = values[0]
                .parse::<u8>()
                .map_err(|_| "Invalid heading level")?;
            if level > 9 {
                return Err("Choose a level from 0 to 9".into());
            }
            let anchor = self
                .doc
                .position_from_utf16(self.selection.0)
                .map_err(|e| e.to_string())?;
            let focus = self
                .doc
                .position_from_utf16(self.selection.1)
                .map_err(|e| e.to_string())?;
            let mut style = self
                .doc
                .state()
                .blocks
                .iter()
                .find_map(|b| match b {
                    Block::Paragraph(p) if p.id == anchor.paragraph => Some(p.style.clone()),
                    _ => None,
                })
                .ok_or("Paragraph not found")?;
            style.outline_level = level.checked_sub(1);
            self.doc
                .set_paragraph_style(
                    anchor.paragraph,
                    style,
                    Selection {
                        anchor: Some(anchor),
                        focus: Some(focus),
                    },
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id == 314 {
            if values.len() != 3 {
                return Err("Invalid spacing".into());
            }
            let mut converted = Vec::new();
            for value in values {
                let n = value.parse::<f64>().map_err(|_| "Invalid spacing")?;
                if !n.is_finite() || !(0.0..=10000.0).contains(&n) {
                    return Err("Spacing must be 0–10000 points".into());
                }
                converted.push((n * 20.0).round() as u32);
            }
            let pos = self
                .doc
                .position_from_utf16(self.selection.0)
                .map_err(|e| e.to_string())?;
            let focus = self
                .doc
                .position_from_utf16(self.selection.1)
                .map_err(|e| e.to_string())?;
            let mut style = self
                .doc
                .state()
                .blocks
                .iter()
                .find_map(|b| match b {
                    Block::Paragraph(p) if p.id == pos.paragraph => Some(p.style.clone()),
                    _ => None,
                })
                .ok_or("Paragraph not found")?;
            style.before_twips = converted[0];
            style.after_twips = converted[1];
            style.line_twips = converted[2] as i32;
            self.doc
                .set_paragraph_style(
                    pos.paragraph,
                    style,
                    Selection {
                        anchor: Some(pos),
                        focus: Some(focus),
                    },
                )
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        if id != 313 || values.len() != 2 {
            return Err("Invalid table input".into());
        }
        let rows = values[0].parse::<usize>().map_err(|_| "Invalid rows")?;
        let cols = values[1].parse::<usize>().map_err(|_| "Invalid columns")?;
        if !(1..=50).contains(&rows) || !(1..=20).contains(&cols) {
            return Err("Choose 1–50 rows and 1–20 columns".into());
        }
        let paragraph = self
            .doc
            .position_from_utf16(self.selection.1)
            .ok()
            .map(|p| p.paragraph);
        self.doc
            .transact(|s| {
                let index = s
                    .blocks
                    .iter()
                    .position(|b| matches!(b,Block::Paragraph(p) if Some(p.id)==paragraph))
                    .map_or(s.blocks.len(), |i| i + 1);
                let table_id = s.next_id;
                s.next_id = s
                    .next_id
                    .checked_add(1)
                    .ok_or(doc42_core::rich::Error::Limit)?;
                let mut table_rows = Vec::new();
                for _ in 0..rows {
                    let mut cells = Vec::new();
                    for _ in 0..cols {
                        let id = s.next_id;
                        s.next_id = s
                            .next_id
                            .checked_add(1)
                            .ok_or(doc42_core::rich::Error::Limit)?;
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
                    }
                    table_rows.push(cells);
                }
                s.blocks.insert(
                    index,
                    Block::Table(Table {
                        column_widths_twips: Vec::new(),
                        id: table_id,
                        rows: table_rows,
                    }),
                );
                for section in &mut s.sections {
                    if section.start_block >= index && section.start_block != 0 {
                        section.start_block += 1;
                    }
                }
                s.selection = Selection::default();
                Ok(())
            })
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn rich_preview(&self, id: u16) -> Option<win42::RichPrint> {
        if id == 316 {
            self.rich_print()
        } else {
            None
        }
    }
    fn print_job(&self, id: u16) -> Option<win42::PrintJob> {
        if id == 315 {
            self.rich_print().map(win42::PrintJob::RichText)
        } else {
            None
        }
    }
    fn dialog_request(&self, id: u16) -> Option<DialogRequest> {
        match id {
            303 => Some(DialogRequest::Font),
            304 => Some(DialogRequest::Color([0, 0, 0])),
            309 => {
                let p = &self.doc.state().sections[0].page;
                let hmm = |v: u32| -> i32 {
                    ((u64::from(v) * 127 + 36) / 72).min(i32::MAX as u64) as i32
                };
                Some(DialogRequest::PageSetup(win42::PageMargins {
                    left: hmm(p.margin_left),
                    top: hmm(p.margin_top),
                    right: hmm(p.margin_right),
                    bottom: hmm(p.margin_bottom),
                    paper_width: hmm(p.width_twips),
                    paper_height: hmm(p.height_twips),
                }))
            }
            _ => None,
        }
    }
    fn dialog_result(&mut self, id: u16, result: DialogResult) -> Result<(), String> {
        if let (309, DialogResult::PageSetup(p)) = (id, &result) {
            let twips = |v: i32| -> Result<u32, String> {
                if v < 0 {
                    return Err("Negative page setting".into());
                }
                Ok(((v as u64 * 72 + 63) / 127) as u32)
            };
            let (w, h, l, r, t, b) = (
                twips(p.paper_width)?,
                twips(p.paper_height)?,
                twips(p.left)?,
                twips(p.right)?,
                twips(p.top)?,
                twips(p.bottom)?,
            );
            self.doc
                .transact(|s| {
                    let page = &mut s.sections[0].page;
                    page.width_twips = w;
                    page.height_twips = h;
                    page.margin_left = l;
                    page.margin_right = r;
                    page.margin_top = t;
                    page.margin_bottom = b;
                    Ok(())
                })
                .map_err(|e| e.to_string())?;
            return Ok(());
        }
        let (_, _, mut style, _) = self.selection_style()?;
        match (id, result) {
            (303, DialogResult::Font(f)) => {
                if !f.points.is_finite() || !(1.0..=32767.0).contains(&(f.points * 2.0)) {
                    return Err("Invalid font size".into());
                }
                style.font = f.family;
                style.size_half_points = (f.points * 2.0).round() as u16;
                style.bold = f.bold;
                style.italic = f.italic;
                style.underline = f.underline;
                style.theme_color = None;
                style.theme_tint = None;
                style.theme_shade = None;
                style.color = Some(
                    (u32::from(f.color[0]) << 16)
                        | (u32::from(f.color[1]) << 8)
                        | u32::from(f.color[2]),
                );
            }
            (304, DialogResult::Color(c)) => {
                style.theme_color = None;
                style.theme_tint = None;
                style.theme_shade = None;
                style.color =
                    Some((u32::from(c[0]) << 16) | (u32::from(c[1]) << 8) | u32::from(c[2]))
            }
            _ => return Err("Invalid dialog response".into()),
        };
        self.apply_style(style)
    }
    fn items(&self) -> Vec<String> {
        vec![]
    }
    fn list_visible(&self) -> bool {
        false
    }
    fn text(&self) -> String {
        win42::to_native_text(&self.visible_text())
    }
    fn status(&self) -> String {
        let headers = self
            .doc
            .state()
            .sections
            .iter()
            .any(|s| !s.header.is_empty() || !s.footer.is_empty());
        format!(
            "{} words · {}%{}{}{}",
            self.visible_text().split_whitespace().count(),
            self.zoom_percent,
            if headers {
                " · header/footer stored; body view"
            } else {
                ""
            },
            if self.doc.state().footnotes.is_empty() {
                String::new()
            } else if !self.note_layout_supported() {
                format!(
                    " · {} notes (printing unavailable for this layout)",
                    self.doc.state().footnotes.len()
                )
            } else {
                format!(
                    " · {} notes (markers shown in print)",
                    self.doc.state().footnotes.len()
                )
            },
            if self
                .doc
                .state()
                .tables_of_contents
                .iter()
                .any(|t| t.docx_cached.is_some()
                    || t.odt_cached
                        .as_ref()
                        .is_some_and(|c| c.has_pages && !c.needs_refresh))
            {
                " · TOC page numbers are imported cache"
            } else {
                ""
            }
        )
    }
    fn dirty(&self) -> bool {
        self.doc.is_dirty()
    }
    fn can_undo(&self) -> bool {
        self.doc.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.doc.can_redo()
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mut data = Vec::new();
        std::fs::File::open(p)
            .map_err(|e| e.to_string())?
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        let base = p.parent().unwrap_or_else(|| Path::new("."));
        let resolve = |reference: &str| -> Result<Vec<u8>, doc42_core::rich::Error> {
            if reference.contains(':')
                || reference.starts_with('/')
                || reference.starts_with('\\')
                || reference.contains(['?', '#'])
            {
                return Err(doc42_core::rich::Error::Unsupported(
                    "Only local relative images are supported".into(),
                ));
            }
            let input = reference.as_bytes();
            let mut decoded = Vec::new();
            let mut i = 0;
            while i < input.len() {
                if input[i] == b'%' {
                    if i + 2 >= input.len() {
                        return Err(doc42_core::rich::Error::Invalid(
                            "Invalid image path encoding".into(),
                        ));
                    }
                    let hex = std::str::from_utf8(&input[i + 1..i + 3])
                        .map_err(|e| doc42_core::rich::Error::Invalid(e.to_string()))?;
                    decoded.push(
                        u8::from_str_radix(hex, 16)
                            .map_err(|e| doc42_core::rich::Error::Invalid(e.to_string()))?,
                    );
                    i += 3
                } else {
                    decoded.push(input[i]);
                    i += 1
                }
            }
            let name = std::str::from_utf8(&decoded)
                .map_err(|e| doc42_core::rich::Error::Invalid(e.to_string()))?;
            if name.contains('\0') || name.contains(':') || Path::new(name).is_absolute() {
                return Err(doc42_core::rich::Error::Invalid(
                    "Invalid image path".into(),
                ));
            }
            let mut image = Vec::new();
            std::fs::File::open(base.join(name))
                .map_err(|e| doc42_core::rich::Error::Invalid(e.to_string()))?
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut image)
                .map_err(|e| doc42_core::rich::Error::Invalid(e.to_string()))?;
            if image.len() > 8 * 1024 * 1024 {
                return Err(doc42_core::rich::Error::Limit);
            }
            Ok(image)
        };
        let doc = RichDocument::decode_with_resources(&data, Self::format(&ext)?, &resolve)
            .map_err(|e| e.to_string())?;
        doc.rich_edit_projection()
            .map_err(|e| format!("Cannot safely display this document: {e}"))?;
        self.doc = doc;
        self.selection = (0, 0);
        self.snapshots.clear();
        Ok(())
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        let ext = p
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let bytes = self
            .doc
            .encode(Self::format(&ext)?)
            .map_err(|e| e.to_string())?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(p)
            .map_err(|e| e.to_string())?;
        f.write_all(&bytes).map_err(|e| e.to_string())?;
        self.doc.mark_saved();
        Ok(())
    }
    fn edit(&mut self, s: String) -> Result<(), String> {
        self.doc
            .set_plain_editor_text(&win42::from_native_text(&s))
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn select(&mut self, _: usize) {}
    fn take_external_url(&mut self) -> Option<String> {
        self.external_url.take()
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        match id {
            351 => {
                let (p, range, _, selection) = self.selection_style()?;
                self.doc
                    .set_link(p, range, None, selection)
                    .map_err(|e| e.to_string())?;
            }
            354 => {
                let position = self
                    .doc
                    .position_from_utf16(self.selection.0)
                    .map_err(|e| e.to_string())?;
                let paragraph = self
                    .doc
                    .state()
                    .blocks
                    .iter()
                    .find_map(|b| match b {
                        Block::Paragraph(p) if p.id == position.paragraph => Some(p),
                        _ => None,
                    })
                    .ok_or("Place the caret in a link")?;
                let mut offset = 0;
                let target = paragraph
                    .runs
                    .iter()
                    .find_map(|r| {
                        let start = offset;
                        offset += r.text.len();
                        (position.offset >= start && position.offset < offset)
                            .then_some(r.link.as_deref())
                            .flatten()
                    })
                    .ok_or("Place the caret in a link")?
                    .to_owned();
                if let Some(name) = target.strip_prefix('#') {
                    let bookmark = self
                        .doc
                        .state()
                        .bookmarks
                        .iter()
                        .find(|b| b.name == name)
                        .map(|b| b.start.clone())
                        .or_else(|| self.doc.toc_target(position.paragraph))
                        .ok_or("Bookmark not found")?;
                    let at = self
                        .doc
                        .position_to_utf16(&bookmark)
                        .map_err(|e| e.to_string())?;
                    self.selection = (at, at);
                    self.requested_selection = Some((at, at));
                } else if target.starts_with("https://")
                    || target.starts_with("http://")
                    || target.starts_with("mailto:")
                {
                    self.external_url = Some(target);
                } else {
                    return Err("This link uses an unsupported scheme".into());
                }
            }
            346 => {
                let at = self
                    .doc
                    .position_from_utf16(self.selection.0)
                    .map_err(|e| e.to_string())?;
                self.doc
                    .insert_toc(at.paragraph)
                    .map_err(|e| e.to_string())?;
            }
            347 | 348 => {
                let at = self.doc.position_from_utf16(self.selection.0).ok();
                let toc = self
                    .doc
                    .state()
                    .tables_of_contents
                    .iter()
                    .find(|t| {
                        at.as_ref()
                            .is_some_and(|p| t.entries.iter().any(|e| e.paragraph == p.paragraph))
                    })
                    .or_else(|| self.doc.state().tables_of_contents.first())
                    .ok_or("No table of contents")?
                    .id;
                if id == 347 {
                    self.doc.update_toc(toc)
                } else {
                    self.doc.remove_toc(toc)
                }
                .map_err(|e| e.to_string())?;
            }
            349 => {
                let at = self
                    .doc
                    .position_from_utf16(self.selection.0)
                    .map_err(|e| e.to_string())?;
                let target = self
                    .doc
                    .toc_target(at.paragraph)
                    .ok_or("Place the caret in a table-of-contents entry")?;
                let offset = self
                    .doc
                    .position_to_utf16(&target)
                    .map_err(|e| e.to_string())?;
                self.selection = (offset, offset);
                self.requested_selection = Some((offset, offset));
            }
            4 => {
                self.doc.undo();
            }
            5 => {
                self.doc.redo();
            }
            300..=302 => {
                let (_, _, mut s, _) = self.selection_style()?;
                match id {
                    300 => s.bold = !s.bold,
                    301 => s.italic = !s.italic,
                    _ => s.underline = !s.underline,
                };
                self.apply_style(s)?;
            }
            317 | 318 => {
                let levels = [25u16, 50, 75, 100, 125, 150, 200];
                let index = levels
                    .iter()
                    .position(|v| *v == self.zoom_percent)
                    .unwrap_or(3);
                self.zoom_percent = if id == 317 {
                    levels[(index + 1).min(levels.len() - 1)]
                } else {
                    levels[index.saturating_sub(1)]
                };
            }
            311..=312 => {
                let pos = self
                    .doc
                    .position_from_utf16(self.selection.0)
                    .map_err(|e| e.to_string())?;
                let focus = self
                    .doc
                    .position_from_utf16(self.selection.1)
                    .map_err(|e| e.to_string())?;
                let numbered = id == 312;
                self.doc
                    .transact(|s| {
                        let index = s
                            .blocks
                            .iter()
                            .position(|b| matches!(b,Block::Paragraph(p) if p.id==pos.paragraph))
                            .ok_or_else(|| {
                                doc42_core::rich::Error::Invalid("paragraph not found".into())
                            })?;
                        let prior = index
                            .checked_sub(1)
                            .and_then(|i| match &s.blocks[i] {
                                Block::Paragraph(p) => p.style.list.clone(),
                                _ => None,
                            })
                            .filter(|l| l.numbered == numbered);
                        let Block::Paragraph(p) = &mut s.blocks[index] else {
                            unreachable!()
                        };
                        if p.style
                            .list
                            .as_ref()
                            .is_some_and(|l| l.numbered == numbered)
                        {
                            p.style.list = None;
                        } else if let Some(list) = prior {
                            p.style.list = Some(list);
                        } else {
                            let id = s.next_id;
                            s.next_id = s
                                .next_id
                                .checked_add(1)
                                .ok_or(doc42_core::rich::Error::Limit)?;
                            p.style.list = Some(ListMarker {
                                id,
                                level: 0,
                                numbered,
                            });
                        }
                        s.selection = Selection {
                            anchor: Some(pos),
                            focus: Some(focus),
                        };
                        Ok(())
                    })
                    .map_err(|e| e.to_string())?;
            }
            305..=308 => {
                let pos = self
                    .doc
                    .position_from_utf16(self.selection.0)
                    .map_err(|e| e.to_string())?;
                let mut style = self
                    .doc
                    .state()
                    .blocks
                    .iter()
                    .find_map(|b| match b {
                        Block::Paragraph(p) if p.id == pos.paragraph => Some(p.style.clone()),
                        _ => None,
                    })
                    .ok_or("Paragraph not found")?;
                style.alignment = match id {
                    305 => Alignment::Left,
                    306 => Alignment::Center,
                    307 => Alignment::Right,
                    _ => Alignment::Justify,
                };
                let focus = self
                    .doc
                    .position_from_utf16(self.selection.1)
                    .map_err(|e| e.to_string())?;
                self.doc
                    .set_paragraph_style(
                        pos.paragraph,
                        style,
                        Selection {
                            anchor: Some(pos),
                            focus: Some(focus),
                        },
                    )
                    .map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        Ok(())
    }
    fn supports_find_replace(&self) -> bool {
        true
    }
    fn find_next(&self, query: &str, start_utf16: usize) -> Result<Option<(usize, usize)>, String> {
        let text = self.text();
        let start = text
            .char_indices()
            .scan(0usize, |n, (i, c)| {
                let old = *n;
                *n += c.len_utf16();
                Some((i, old))
            })
            .find(|(_, n)| *n >= start_utf16)
            .map(|(i, _)| i)
            .unwrap_or(text.len());
        if query.is_empty() {
            return Ok(None);
        }
        let found = text[start..]
            .find(query)
            .map(|i| start + i)
            .or_else(|| text.find(query));
        Ok(found.map(|i| {
            let n = text[..i].encode_utf16().count();
            (n, n + query.encode_utf16().count())
        }))
    }
    fn replace_all(&mut self, query: &str, replacement: &str) -> Result<usize, String> {
        self.doc
            .replace_all_text(query, replacement)
            .map_err(|e| e.to_string())
    }
}
fn main() {
    win42::run(Box::new(Doc::new()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn link_and_bookmark_commands_follow_only_on_explicit_action() {
        let mut app = Doc::new();
        app.doc.set_plain_editor_text("Hello target").unwrap();
        app.selection = (6, 12);
        app.input_action(352, &["target".into()]).unwrap();
        app.selection = (0, 5);
        app.input_action(350, &["#target".into()]).unwrap();
        app.selection = (1, 1);
        app.action(354).unwrap();
        assert_eq!(app.selection, (6, 6));
        app.selection = (0, 5);
        app.input_action(350, &["https://example.com".into()])
            .unwrap();
        assert!(app.take_external_url().is_none());
        app.selection = (1, 1);
        app.action(354).unwrap();
        assert_eq!(
            app.take_external_url().as_deref(),
            Some("https://example.com")
        );
        assert!(app
            .input_action(350, &["file:///tmp/example".into()])
            .is_err());
    }
    #[test]
    fn toc_commands_insert_navigate_refresh_remove_and_undo() {
        let mut app = Doc::new();
        app.doc.set_plain_editor_text("Heading\nBody").unwrap();
        app.doc
            .transact(|s| {
                let Block::Paragraph(p) = &mut s.blocks[0] else {
                    panic!()
                };
                p.style.outline_level = Some(0);
                Ok(())
            })
            .unwrap();
        app.selection = (9, 9);
        app.action(346).unwrap();
        let entry = app.doc.state().tables_of_contents[0].entries[0].clone();
        let position = app
            .doc
            .position_to_utf16(&doc42_core::rich::Position {
                paragraph: entry.paragraph,
                offset: 0,
            })
            .unwrap();
        app.selection = (position, position);
        app.action(349).unwrap();
        assert_eq!(
            app.selection.0,
            app.doc
                .position_to_utf16(&doc42_core::rich::Position {
                    paragraph: entry.heading,
                    offset: 0
                })
                .unwrap()
        );
        app.action(347).unwrap();
        app.action(348).unwrap();
        assert!(app.doc.state().tables_of_contents.is_empty());
        app.action(4).unwrap();
        assert_eq!(app.doc.state().tables_of_contents.len(), 1);
    }
    #[test]
    fn header_footer_page_fields_roundtrip_and_undo() {
        let mut app = Doc::new();
        app.set_header_input(false, "Page {PAGE} of {PAGES}")
            .unwrap();
        app.set_header_input(true, "Confidential").unwrap();
        assert_eq!(app.header_input(false).unwrap(), "Page {PAGE} of {PAGES}");
        let snapshot = app.doc.snapshot();
        let bytes = app.doc.encode(Format::Project).unwrap();
        assert_eq!(
            RichDocument::decode(&bytes, Format::Project)
                .unwrap()
                .state(),
            &snapshot
        );
        let rtf = app.doc.encode(Format::Rtf).unwrap();
        let reopened = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        assert_eq!(reopened.state().sections[0].header.len(), 1);
        assert_eq!(reopened.state().fields.len(), 2);
        let docx = app.doc.encode(Format::Docx).unwrap();
        let reopened = RichDocument::decode(&docx, Format::Docx).unwrap();
        assert_eq!(reopened.state().sections[0].header.len(), 1);
        assert_eq!(reopened.state().sections[0].footer.len(), 1);
        assert_eq!(reopened.state().fields.len(), 2);
        let odt = app.doc.encode(Format::Odt).unwrap();
        let reopened = RichDocument::decode(&odt, Format::Odt).unwrap();
        assert_eq!(reopened.state().sections[0].header.len(), 1);
        assert_eq!(reopened.state().fields.len(), 2);
        let view = app.doc.rich_edit_projection().unwrap();
        app.doc.apply_rich_edit_projection(&view, (0, 0)).unwrap();
        assert_eq!(app.doc.state().fields.len(), 2);
        app.action(4).unwrap();
        assert!(app.doc.state().sections[0].footer.is_empty());
        app.action(5).unwrap();
        assert_eq!(app.doc.state(), &snapshot);
        let print = app.rich_print().unwrap();
        assert_eq!(print.header.len(), 1);
        assert!(print.header[0]
            .fragments
            .iter()
            .any(|f| matches!(f.text, win42::PrintText::PageCount)));
    }
    #[test]
    fn heading_level_survives_formats_and_undo() {
        let mut app = Doc::new();
        app.edit("Heading".into()).unwrap();
        app.input_action(326, &["2".into()]).unwrap();
        for format in [Format::Project, Format::Rtf, Format::Docx, Format::Odt] {
            let bytes = app.doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let Block::Paragraph(p) = &reopened.state().blocks[0] else {
                panic!()
            };
            assert_eq!(p.style.outline_level, Some(1), "{format:?}");
        }
        app.action(4).unwrap();
        let Block::Paragraph(p) = &app.doc.state().blocks[0] else {
            panic!()
        };
        assert_eq!(p.style.outline_level, None);
    }
    #[test]
    fn imported_odt_page_and_headers_can_be_edited() {
        let mut original = Doc::new();
        original.edit("body".into()).unwrap();
        original.set_header_input(false, "Old {PAGE}").unwrap();
        let bytes = original.doc.encode(Format::Odt).unwrap();
        let mut imported = Doc::new();
        imported.doc = RichDocument::decode(&bytes, Format::Odt).unwrap();
        imported.set_header_input(false, "New {PAGES}").unwrap();
        imported
            .rich_ruler_action(win42::RichRulerAction::LeftMargin(1200))
            .unwrap();
        let updated = imported.doc.encode(Format::Odt).unwrap();
        let mut reopened = Doc::new();
        reopened.doc = RichDocument::decode(&updated, Format::Odt).unwrap();
        assert_eq!(reopened.header_input(false).unwrap(), "New {PAGES}");
        assert_eq!(reopened.doc.state().sections[0].page.margin_left, 1200);
    }
    #[test]
    fn notes_commands_save_project_office_and_create_print_plan() {
        let mut app = Doc::new();
        app.edit("Hello world".into()).unwrap();
        app.set_rich_selection((5, 5));
        app.input_action(340, &["Footnote text".into()]).unwrap();
        app.set_rich_selection((11, 11));
        app.input_action(341, &["Endnote text".into()]).unwrap();
        let print = app.rich_print().unwrap();
        assert!(print.body_rtf.is_some());
        assert_eq!(print.notes.len(), 2);
        assert!(!print.notes[0].endnote);
        assert!(print.notes[1].endnote);
        for format in [Format::Project, Format::Docx, Format::Odt] {
            let reopened = RichDocument::decode(&app.doc.encode(format).unwrap(), format).unwrap();
            assert_eq!(reopened.state().footnotes.len(), 2, "{format:?}");
            assert_eq!(
                (
                    reopened.state().footnotes[0].at.offset,
                    reopened.state().footnotes[1].at.offset
                ),
                (5, 11)
            );
        }
        app.input_action(342, &["Changed".into(), "1".into()])
            .unwrap();
        app.input_action(343, &["1".into()]).unwrap();
        assert_eq!(app.doc.state().footnotes.len(), 1);
        app.action(4).unwrap();
        assert_eq!(
            app.doc.state().footnotes[0].paragraphs[0].runs[0].text,
            "Changed"
        );
        app.action(4).unwrap();
        assert_eq!(
            app.doc.state().footnotes[0].paragraphs[0].runs[0].text,
            "Footnote text"
        );
    }
    #[test]
    fn comment_commands_project_and_docx_roundtrip() {
        let mut app = Doc::new();
        app.edit("Hello world".into()).unwrap();
        app.set_rich_selection((6, 11));
        app.input_action(335, &["Review this".into(), "Editor".into()])
            .unwrap();
        assert_eq!(app.doc.state().comments.len(), 1);
        let view = app.rich_text().unwrap();
        app.edit_rich_text(&view, (0, 0)).unwrap();
        app.input_action(339, &["1".into()]).unwrap();
        assert_eq!(app.take_requested_rich_selection(), Some((6, 11)));
        for format in [Format::Project, Format::Docx, Format::Odt] {
            let reopened = RichDocument::decode(&app.doc.encode(format).unwrap(), format).unwrap();
            let saved_again =
                RichDocument::decode(&reopened.encode(format).unwrap(), format).unwrap();
            assert_eq!(saved_again.state().comments[0].text, "Review this");
            assert_eq!(reopened.state().comments[0].text, "Review this");
            assert_eq!(
                (
                    reopened.state().comments[0].start.offset,
                    reopened.state().comments[0].end.offset
                ),
                (6, 11)
            );
        }
        app.input_action(336, &["Revised".into(), "1".into(), "no".into()])
            .unwrap();
        app.input_action(337, &["1".into()]).unwrap();
        assert!(app.doc.state().comments.is_empty());
        app.action(4).unwrap();
        assert_eq!(app.doc.state().comments[0].text, "Revised");
        app.action(4).unwrap();
        assert_eq!(app.doc.state().comments[0].text, "Review this");
    }
    #[test]
    fn insert_delete_around_and_through_merged_cells_preserves_content_and_undo() {
        let mut app = Doc::new();
        app.input_action(313, &["3".into(), "3".into()]).unwrap();
        app.doc
            .transact(|s| {
                let Block::Table(t) = &mut s.blocks[1] else {
                    panic!()
                };
                for (cell, text) in t
                    .rows
                    .iter_mut()
                    .flatten()
                    .zip(["a", "b", "c", "d", "e", "f", "g", "h", "i"])
                {
                    cell.paragraphs[0].runs[0].text = text.into();
                }
                Ok(())
            })
            .unwrap();
        app.input_action(
            334,
            &["1".into(), "1".into(), "1".into(), "2".into(), "2".into()],
        )
        .unwrap();
        app.input_action(331, &["1".into(), "1".into(), "1".into(), "#12ABEF".into()])
            .unwrap();
        app.input_action(
            332,
            &[
                "1".into(),
                "1".into(),
                "1".into(),
                "1".into(),
                "#334455".into(),
            ],
        )
        .unwrap();
        app.input_action(333, &["1".into(), "2".into(), "100.25".into()])
            .unwrap();
        let before = app.doc.snapshot();
        app.input_action(321, &["1".into(), "3".into()]).unwrap(); // disjoint below merged header
        app.input_action(321, &["1".into(), "2".into()]).unwrap(); // expand vertical span
        app.input_action(322, &["1".into(), "2".into()]).unwrap(); // expand horizontal span
        app.input_action(324, &["1".into(), "1".into()]).unwrap(); // promote deleted anchor row
        app.input_action(325, &["1".into(), "1".into()]).unwrap(); // promote deleted anchor column
        let view = app.rich_text().unwrap();
        app.edit_rich_text(&view, (0, 0)).unwrap();
        for format in [
            Format::Project,
            Format::Rtf,
            Format::Docx,
            Format::Odt,
            Format::Html,
            Format::Markdown,
        ] {
            let reopened = RichDocument::decode(&app.doc.encode(format).unwrap(), format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(
                (t.rows[0][0].row_span, t.rows[0][0].col_span),
                (2, 2),
                "{format:?}"
            );
            assert_eq!(t.rows[0][0].fill, Some(0x12abef), "{format:?}");
            assert_eq!(
                t.rows[0][0].border,
                Some(doc42_core::rich::Border {
                    width_twips: 20,
                    color: 0x334455
                }),
                "{format:?}"
            );
            assert_eq!(
                t.rows[0][0]
                    .paragraphs
                    .iter()
                    .map(|p| p.runs[0].text.as_str())
                    .collect::<Vec<_>>(),
                ["a", "b", "d", "e"]
            );
            assert_eq!(t.column_widths_twips, [1440, 2005, 1440], "{format:?}");
        }
        for _ in 0..5 {
            app.action(4).unwrap();
        }
        assert_eq!(app.doc.state(), &before);
    }
    #[test]
    fn rectangular_merge_keeps_content_styles_widths_and_undo() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        app.doc
            .transact(|s| {
                let Block::Table(t) = &mut s.blocks[1] else {
                    panic!()
                };
                for (cell, text) in t
                    .rows
                    .iter_mut()
                    .flatten()
                    .zip(["one", "two", "three", "four"])
                {
                    cell.paragraphs[0].runs[0].text = text.into();
                }
                Ok(())
            })
            .unwrap();
        app.input_action(
            334,
            &["1".into(), "1".into(), "1".into(), "2".into(), "2".into()],
        )
        .unwrap();
        app.input_action(331, &["1".into(), "2".into(), "2".into(), "#12ABEF".into()])
            .unwrap();
        app.input_action(
            332,
            &[
                "1".into(),
                "2".into(),
                "2".into(),
                "1".into(),
                "#334455".into(),
            ],
        )
        .unwrap();
        app.input_action(333, &["1".into(), "2".into(), "100.25".into()])
            .unwrap();
        let view = app.rich_text().unwrap();
        app.edit_rich_text(&view, (0, 0)).unwrap();
        for format in [
            Format::Project,
            Format::Rtf,
            Format::Docx,
            Format::Odt,
            Format::Html,
            Format::Markdown,
        ] {
            let reopened = RichDocument::decode(&app.doc.encode(format).unwrap(), format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(
                (t.rows[0][0].row_span, t.rows[0][0].col_span),
                (2, 2),
                "{format:?}"
            );
            assert_eq!(t.column_widths_twips, vec![1440, 2005], "{format:?}");
            assert_eq!(t.rows[0][0].fill, Some(0x12abef), "{format:?}");
            assert_eq!(
                t.rows[0][0].border,
                Some(doc42_core::rich::Border {
                    width_twips: 20,
                    color: 0x334455
                }),
                "{format:?}"
            );
            assert_eq!(
                t.rows[0][0]
                    .paragraphs
                    .iter()
                    .map(|p| p.runs[0].text.as_str())
                    .collect::<Vec<_>>(),
                ["one", "two", "three", "four"]
            );
        }
        app.input_action(330, &["1".into(), "2".into(), "2".into()])
            .unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert!(t
            .rows
            .iter()
            .flatten()
            .all(|c| c.fill == Some(0x12abef) && c.border.is_some()));
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[0][0].row_span, 2);
    }
    #[test]
    fn merge_and_split_table_cells_preserve_text_and_history() {
        let mut app = Doc::new();
        app.input_action(313, &["1".into(), "3".into()]).unwrap();
        app.doc
            .transact(|s| {
                let Block::Table(t) = &mut s.blocks[1] else {
                    panic!()
                };
                for (cell, text) in t.rows[0].iter_mut().zip(["one", "two", "three"]) {
                    cell.paragraphs[0].runs[0].text = text.into();
                }
                Ok(())
            })
            .unwrap();
        app.input_action(329, &["1".into(), "1".into(), "1".into(), "3".into()])
            .unwrap();
        app.input_action(331, &["1".into(), "1".into(), "2".into(), "#12ABEF".into()])
            .unwrap();
        app.input_action(
            332,
            &[
                "1".into(),
                "1".into(),
                "3".into(),
                "1".into(),
                "#334455".into(),
            ],
        )
        .unwrap();
        app.input_action(333, &["1".into(), "2".into(), "100.25".into()])
            .unwrap();
        let projection = app.rich_text().unwrap();
        app.edit_rich_text(&projection, (0, 0)).unwrap();
        for format in [
            Format::Project,
            Format::Rtf,
            Format::Docx,
            Format::Odt,
            Format::Html,
            Format::Markdown,
        ] {
            let bytes = app.doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(t.rows[0][0].col_span, 3, "{format:?}");
            assert_eq!(t.column_widths_twips, vec![1440, 2005, 1440], "{format:?}");
            assert_eq!(t.rows[0][0].fill, Some(0x12abef), "{format:?}");
            assert_eq!(
                t.rows[0][0].border,
                Some(doc42_core::rich::Border {
                    width_twips: 20,
                    color: 0x334455
                }),
                "{format:?}"
            );
            assert_eq!(
                t.rows[0][0]
                    .paragraphs
                    .iter()
                    .map(|p| p.runs[0].text.as_str())
                    .collect::<Vec<_>>(),
                ["one", "two", "three"]
            );
        }
        app.input_action(330, &["1".into(), "1".into(), "1".into()])
            .unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert!(t.rows[0].iter().all(|cell| cell.fill == Some(0x12abef)));
        assert!(t.rows[0].iter().all(|cell| cell.border
            == Some(doc42_core::rich::Border {
                width_twips: 20,
                color: 0x334455
            })));
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[0][0].col_span, 3);
    }
    #[test]
    fn deleting_annotated_table_cells_is_rejected_without_losing_data() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        app.doc
            .transact(|s| {
                let Block::Table(t) = &mut s.blocks[1] else {
                    panic!()
                };
                t.rows[1][0].paragraphs[0].runs[0].text = "protected".into();
                let paragraph = t.rows[1][0].paragraphs[0].id;
                let id = s.next_id;
                s.next_id += 1;
                s.comments.push(doc42_core::rich::Annotation {
                    id,
                    start: doc42_core::rich::Position {
                        paragraph,
                        offset: 0,
                    },
                    end: doc42_core::rich::Position {
                        paragraph,
                        offset: 9,
                    },
                    author: "author".into(),
                    text: "comment".into(),
                    resolved: false,
                    date: None,
                    initials: None,
                });
                Ok(())
            })
            .unwrap();
        let before = app.doc.snapshot();
        assert!(app.input_action(324, &["1".into(), "2".into()]).is_err());
        assert!(app.input_action(328, &["1".into()]).is_err());
        assert_eq!(app.doc.state(), &before);
    }
    #[test]
    fn table_cell_background_is_undoable_and_roundtrips() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        app.input_action(331, &["1".into(), "2".into(), "1".into(), "#12ABEF".into()])
            .unwrap();
        let projection = app.rich_text().unwrap();
        app.edit_rich_text(&projection, (0, 0)).unwrap();
        for format in [Format::Project, Format::Rtf, Format::Docx, Format::Odt] {
            let bytes = app.doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(t.rows[1][0].fill, Some(0x12abef), "{format:?}");
        }
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[1][0].fill, None);
    }
    #[test]
    fn table_cell_alignment_survives_formats_and_undo() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        app.input_action(327, &["1".into(), "2".into(), "1".into(), "center".into()])
            .unwrap();
        for format in [Format::Project, Format::Rtf, Format::Docx, Format::Odt] {
            let bytes = app.doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let Block::Table(t) = &reopened.state().blocks[1] else {
                panic!()
            };
            assert_eq!(
                t.rows[1][0].paragraphs[0].style.alignment,
                Alignment::Center,
                "{format:?}"
            );
        }
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[1][0].paragraphs[0].style.alignment, Alignment::Left);
    }
    #[test]
    fn table_growth_is_undoable() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        app.input_action(321, &["1".into(), "2".into()]).unwrap();
        app.input_action(322, &["1".into(), "3".into()]).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!((t.rows.len(), t.rows[0].len()), (3, 3));
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[0].len(), 2);
        app.action(5).unwrap();
        app.doc
            .transact(|s| {
                let Block::Table(t) = &mut s.blocks[1] else {
                    panic!()
                };
                t.rows[1][1].paragraphs[0].runs[0].text = "restored cell".into();
                Ok(())
            })
            .unwrap();
        app.input_action(324, &["1".into(), "2".into()]).unwrap();
        app.input_action(325, &["1".into(), "2".into()]).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!((t.rows.len(), t.rows[0].len()), (2, 2));
        app.action(4).unwrap();
        app.action(4).unwrap();
        let bytes = app.doc.encode(Format::Docx).unwrap();
        let reopened = RichDocument::decode(&bytes, Format::Docx).unwrap();
        let Block::Table(t) = &reopened.state().blocks[1] else {
            panic!()
        };
        assert_eq!((t.rows.len(), t.rows[0].len()), (3, 3));
        assert_eq!(t.rows[1][1].paragraphs[0].runs[0].text, "restored cell");
        app.input_action(328, &["1".into()]).unwrap();
        assert!(!app
            .doc
            .state()
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Table(_))));
        app.action(4).unwrap();
        let Block::Table(t) = &app.doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[1][1].paragraphs[0].runs[0].text, "restored cell");
    }
    #[test]
    fn ruler_geometry_indents_and_tabs_survive_history_and_formats() {
        use win42::RichRulerAction::*;
        let mut app = Doc::new();
        app.edit("one\ttwo".into()).unwrap();
        app.rich_ruler_action(LeftMargin(1200)).unwrap();
        app.rich_ruler_action(LeftIndent(300)).unwrap();
        app.rich_ruler_action(FirstIndent(-140)).unwrap();
        app.rich_ruler_action(AddTab(1800)).unwrap();
        let snapshot = app.doc.snapshot();
        app.action(4).unwrap();
        let Block::Paragraph(p) = &app.doc.state().blocks[0] else {
            panic!()
        };
        assert!(p.style.tabs_twips.is_empty());
        app.action(5).unwrap();
        assert_eq!(app.doc.state(), &snapshot);
        for format in [Format::Project, Format::Rtf, Format::Docx, Format::Odt] {
            let bytes = app.doc.encode(format).unwrap();
            let copy = RichDocument::decode(&bytes, format).unwrap();
            let Block::Paragraph(p) = &copy.state().blocks[0] else {
                panic!()
            };
            assert_eq!(copy.state().sections[0].page.margin_left, 1200);
            assert_eq!((p.style.left_twips, p.style.first_line_twips), (300, -140));
            assert_eq!(p.style.tabs_twips, vec![1800]);
        }
        assert!(app.rich_ruler_action(RightMargin(99999)).is_err());
        assert_eq!(app.doc.state(), &snapshot);
    }
    #[test]
    fn markdown_opens_local_relative_image() {
        let dir = std::env::temp_dir().join(format!("doc42-mdimage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ];
        std::fs::write(dir.join("picture 1.png"), png).unwrap();
        let md = dir.join("x.md");
        std::fs::write(&md, b"![alt](picture%201.png)\n").unwrap();
        let mut app = Doc::new();
        app.open(&md).unwrap();
        assert!(matches!(&app.doc.state().blocks[0],Block::Image(i) if i.alt=="alt"));
        assert!(app.rich_text().is_some());
        assert!(app.save_snapshot("md").is_err()); // A path is required to name sidecars.
        let destination = dir.join("result file.md");
        let (revision, bundle) = app
            .save_bundle_to("md", &destination, false)
            .unwrap()
            .unwrap();
        assert_eq!(bundle.sidecars.len(), 1);
        let identity = suite42_core::session::publish_bundle(&destination, &bundle, None).unwrap();
        app.saved_snapshot(revision);
        assert!(!app.dirty());
        let (older_revision, older_bundle) = app
            .save_bundle_to("md", &destination, true)
            .unwrap()
            .unwrap();
        app.doc
            .transact(|s| {
                let Block::Image(image) = &mut s.blocks[0] else {
                    panic!()
                };
                image.alt = "newer".into();
                Ok(())
            })
            .unwrap();
        suite42_core::session::publish_bundle(&destination, &older_bundle, Some(&identity))
            .unwrap();
        app.saved_snapshot(older_revision);
        assert!(app.dirty());
        let mut reopened = Doc::new();
        reopened.open(&destination).unwrap();
        assert!(
            matches!(&reopened.doc.state().blocks[0], Block::Image(i) if i.alt == "alt" && i.data == png)
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn html_markdown_and_odt_open_as_rich_documents() {
        let dir = std::env::temp_dir().join(format!("doc42-formats-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let html = dir.join("x.html");
        std::fs::write(
            &html,
            b"<p>Hello <a href=\"https://example.com\">world</a></p>",
        )
        .unwrap();
        let mut app = Doc::new();
        app.open(&html).unwrap();
        assert!(app.rich_text().is_some());
        assert!(app.save_snapshot("html").unwrap().is_some());
        let md = dir.join("x.md");
        std::fs::write(&md, b"Hello **world**.\n").unwrap();
        app.open(&md).unwrap();
        assert!(app.save_snapshot("md").unwrap().is_some());
        let mut state = RichDocument::new();
        state.set_plain_editor_text("ODT text").unwrap();
        let odt = dir.join("x.odt");
        std::fs::write(&odt, state.encode(Format::Odt).unwrap()).unwrap();
        app.open(&odt).unwrap();
        assert!(app.save_snapshot("odt").unwrap().is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn insert_table_projects_and_undoes() {
        let mut app = Doc::new();
        app.input_action(313, &["2".into(), "2".into()]).unwrap();
        let data = app.rich_text().unwrap();
        let decoded = RichDocument::decode(&data, Format::Rtf).unwrap();
        let Block::Table(t) = &decoded.state().blocks[1] else {
            panic!()
        };
        assert_eq!((t.rows.len(), t.rows[0].len()), (2, 2));
        app.edit_rich_text(&data, (0, 0)).unwrap();
        app.action(4).unwrap();
        assert_eq!(app.doc.state().blocks.len(), 1);
    }
    #[test]
    fn list_command_projects_and_undoes() {
        let mut app = Doc::new();
        app.edit("item".into()).unwrap();
        app.action(312).unwrap();
        let rtf = app.rich_text().unwrap();
        let reopened = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        assert!(
            matches!(&reopened.state().blocks[0],Block::Paragraph(p) if p.style.list.as_ref().is_some_and(|l|l.numbered))
        );
        app.action(4).unwrap();
        assert!(matches!(&app.doc.state().blocks[0],Block::Paragraph(p) if p.style.list.is_none()));
    }
    #[test]
    fn insert_png_creates_picture_and_undoes() {
        let bytes = [
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ];
        let path = std::env::temp_dir().join(format!("doc42-image-{}.png", std::process::id()));
        std::fs::write(&path, bytes).unwrap();
        let mut app = Doc::new();
        app.file_action(310, &path).unwrap();
        assert!(matches!(app.doc.state().blocks[1], Block::Image(_)));
        assert!(app.rich_text().is_some());
        app.action(4).unwrap();
        assert_eq!(app.doc.state().blocks.len(), 1);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn windows_stream_preserves_project_metadata() {
        let mut app = Doc::new();
        app.doc
            .transact(|s| {
                s.sections[0].page.margin_top = 1200;
                Ok(())
            })
            .unwrap();
        let rtf=br"{\rtf1\ansi\ansicpg1252\deff0\deflang1033{\fonttbl{\f0\fnil\fcharset0 Arial;}}{\colortbl;\red255\green0\blue0;}{\*\generator Test;}\viewkind4\uc1\pard\lang1033\cf1\f0\fs24 Caf\'e9 \u1046?\par}";
        app.edit_rich_text(rtf, (0, 2)).unwrap();
        assert_eq!(app.doc.state().sections[0].page.margin_top, 1200);
        assert_eq!(app.visible_text(), "Café Ж");
        assert!(app.doc.can_undo());
    }
    #[test]
    fn native_project_and_rich_edit_roundtrip() {
        let mut app = Doc::new();
        app.edit("one\r\ntwo".into()).unwrap();
        let bytes = app.rich_text().unwrap();
        app.edit_rich_text(&bytes, (0, 3)).unwrap();
        app.selection = (0, 3);
        app.action(300).unwrap();
        assert!(app
            .doc
            .state()
            .blocks
            .iter()
            .any(|b| matches!(b,Block::Paragraph(p) if p.runs.iter().any(|r|r.style.bold))));
        let (_, bytes) = app.save_snapshot("doc42").unwrap().unwrap();
        assert!(RichDocument::decode(&bytes, Format::Project).is_ok());
    }
}

#[cfg(test)]
mod original_tests {
    use super::*;
    use win42::App;
    #[test]
    fn editor_preserves_crlf_during_edit_and_save() {
        let d = RichDocument::decode(b"one\r\ntwo", Format::Text).unwrap();
        let mut app = Doc {
            doc: d,
            ..Doc::new()
        };
        assert_eq!(app.text(), "one\r\ntwo");
        app.edit("one".into()).unwrap();
        app.edit("one\r\nthree".into()).unwrap();
        let path = std::env::temp_dir().join(format!("doc42-ui-{}.txt", std::process::id()));
        let _ = std::fs::remove_file(&path);
        app.save(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"one\r\nthree");
        assert_eq!(
            RichDocument::decode(&std::fs::read(&path).unwrap(), Format::Text)
                .unwrap()
                .editor_text()
                .unwrap(),
            "one\r\nthree"
        );
        std::fs::remove_file(path).unwrap();
    }
}
#[cfg(test)]
mod policy_regression {
    use super::*;
    use win42::App;
    #[test]
    fn rejection_undo_and_branch_keep_core_policy() {
        let d = RichDocument::decode(b"a\r\nb", Format::Text).unwrap();
        let mut a = Doc {
            doc: d,
            ..Doc::new()
        };
        a.edit("single".into()).unwrap();
        assert!(a.edit("x".repeat(4 * 1024 * 1024 + 1)).is_err());
        a.action(4).unwrap();
        assert_eq!(a.doc.editor_text().unwrap(), "a\r\nb");
        a.edit("branch".into()).unwrap();
        assert!(!a.can_redo());
        a.edit("branch\r\nend".into()).unwrap();
        assert_eq!(a.doc.editor_text().unwrap(), "branch\r\nend");
    }
}

#[cfg(test)]
mod find_adapter_regression {
    use super::*;
    use win42::App;
    #[test]
    fn native_caret_inside_match_wraps_across_emoji_and_crlf() {
        let d = RichDocument::decode("x😀z\r\nend".as_bytes(), Format::Text).unwrap();
        let a = Doc {
            doc: d,
            ..Doc::new()
        };
        assert_eq!(a.find_next("😀z\r\n", 3).unwrap(), Some((1, 6)));
        assert_eq!(a.find_next("😀z\r\n", 4).unwrap(), Some((1, 6)));
    }
    #[test]
    fn native_offsets_wrap_across_unicode_and_crlf() {
        let d = RichDocument::decode("😀 a\n😀 a".as_bytes(), Format::Text).unwrap();
        let a = Doc {
            doc: d,
            ..Doc::new()
        };
        assert_eq!(a.find_next("😀", 0).unwrap(), Some((0, 2)));
        assert_eq!(a.find_next("😀", 2).unwrap(), Some((6, 8)));
        assert_eq!(a.find_next("😀", 8).unwrap(), Some((0, 2)));
        assert_eq!(a.find_next("missing", 0).unwrap(), None);
    }
}
