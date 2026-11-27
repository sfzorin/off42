//! Document commands over doc42-core. Positions are `{id, offset}`: a body paragraph id and an offset in Unicode
//! characters (code points) within its text; the core works in UTF-8 bytes and the conversion happens here.

use crate::Result;
use doc42_core::rich::{
    ops, Alignment, Block, Error as DocError, Format, ListMarker, Paragraph, ParagraphStyle, Position, RichDocument,
    Run, Selection, State, TextStyle,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub(crate) fn format(name: &str) -> Result<Format> {
    Ok(match name.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
        "docx" => Format::Docx,
        "odt" => Format::Odt,
        "rtf" => Format::Rtf,
        "md" | "markdown" => Format::Markdown,
        "html" | "htm" => Format::Html,
        "txt" | "text" => Format::Text,
        other => return Err(format!("Unsupported document format: {other}")),
    })
}

/// A position in a body paragraph: id and character offset.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pos {
    pub id: u64,
    pub offset: usize,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Marks {
    #[serde(default)]
    bold: Option<bool>,
    #[serde(default)]
    italic: Option<bool>,
    #[serde(default)]
    underline: Option<bool>,
}

impl Marks {
    fn apply(self, style: &TextStyle) -> TextStyle {
        let mut s = style.clone();
        if let Some(v) = self.bold {
            s.bold = v;
        }
        if let Some(v) = self.italic {
            s.italic = v;
        }
        if let Some(v) = self.underline {
            s.underline = v;
        }
        s
    }
    fn is_empty(self) -> bool {
        self.bold.is_none() && self.italic.is_none() && self.underline.is_none()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum StyleName {
    Normal,
    Heading1,
    Heading2,
    Heading3,
    BulletList,
    NumberedList,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Align {
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct NewParagraph {
    text: String,
    style: StyleName,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    /// Replace the range `at`..`to` (or insert at `at`) with text; line breaks start new paragraphs. `marks`
    /// overrides bold/italic/underline of the inserted text; `typing` joins consecutive typing into one Undo step.
    InsertText {
        at: Pos,
        #[serde(default)]
        to: Option<Pos>,
        text: String,
        #[serde(default)]
        marks: Option<Marks>,
        #[serde(default)]
        typing: bool,
    },
    /// Delete from one position to another (across paragraphs: the rest joins the first paragraph).
    Delete { from: Pos, to: Pos },
    /// Enter: split the paragraph at `at` (replacing `at`..`to`). In an empty list item it ends the list instead.
    SplitParagraph {
        at: Pos,
        #[serde(default)]
        to: Option<Pos>,
    },
    SetParagraphStyle { ids: Vec<u64>, style: StyleName },
    SetAlignment { ids: Vec<u64>, alignment: Align },
    /// Bold/italic/underline on (true) or off (false): over `from`..`to`, or occurrences of `find` (in `ids` or
    /// everywhere), or the whole text of `ids`.
    ApplyTextStyle {
        #[serde(default)]
        from: Option<Pos>,
        #[serde(default)]
        to: Option<Pos>,
        #[serde(default)]
        ids: Option<Vec<u64>>,
        #[serde(default)]
        find: Option<String>,
        #[serde(default)]
        match_case: bool,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        underline: Option<bool>,
    },
    /// With id and find: every occurrence in that paragraph; find only: everywhere; id only: the whole paragraph.
    ReplaceText {
        #[serde(default)]
        id: Option<u64>,
        #[serde(default)]
        find: Option<String>,
        replace: String,
    },
    /// New paragraphs after `after_id` (0: at the start).
    InsertParagraphs { after_id: u64, paragraphs: Vec<NewParagraph> },
    DeleteParagraphs { ids: Vec<u64> },
    DocumentContext { start: Option<Pos>, end: Option<Pos> },
    GetOutline { offset: Option<usize>, limit: Option<usize> },
    ReadRange { ids: Vec<u64>, start: Option<usize>, length: Option<usize> },
    Find {
        text: String,
        #[serde(default)]
        match_case: bool,
    },
}

impl Command {
    fn reads_only(&self) -> bool {
        matches!(self, Self::DocumentContext { .. } | Self::GetOutline { .. } | Self::ReadRange { .. } | Self::Find { .. })
    }
}

pub struct DocumentSession {
    pub(crate) doc: RichDocument,
    /// Where consecutive typing would continue (the caret after the last typed text).
    typing: Option<Pos>,
}

fn failed(e: DocError) -> String {
    e.to_string()
}

impl DocumentSession {
    pub(crate) fn editing_copy(&self) -> Self {
        Self { doc: self.doc.editing_copy(), typing: None }
    }

    pub fn blank() -> Self {
        let mut doc = RichDocument::new();
        doc.set_history_limit(crate::HISTORY_DEPTH);
        Self { doc, typing: None }
    }

    pub fn open(bytes: &[u8], from: &str) -> Result<Self> {
        let mut doc = RichDocument::decode(bytes, format(from)?).map_err(failed)?;
        doc.set_history_limit(crate::HISTORY_DEPTH);
        Ok(Self { doc, typing: None })
    }

    pub fn document(&self) -> &RichDocument {
        &self.doc
    }

    /// Why the document cannot be edited here, if it cannot.
    pub fn read_only_reason(&self) -> Option<&'static str> {
        let s = self.doc.state();
        if s.track_changes || !s.revisions.is_empty() {
            Some("This document has tracked changes; editing them is not available in the browser yet.")
        } else {
            None
        }
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.typing = None;
        self.doc.undo()
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.typing = None;
        self.doc.redo()
    }

    pub(crate) fn apply(&mut self, commands: Vec<Value>) -> Result<Vec<Value>> {
        let commands = commands
            .into_iter()
            .map(|c| serde_json::from_value::<Command>(c).map_err(|e| format!("Invalid command: {e}")))
            .collect::<Result<Vec<_>>>()?;
        if commands.iter().all(Command::reads_only) {
            return commands.iter().map(|c| read(self.doc.state(), c)).collect();
        }
        if let Some(reason) = self.read_only_reason() {
            return Err(reason.into());
        }
        // Consecutive typing at the caret the previous typing left joins its Undo step.
        let amend = matches!(&commands[..], [Command::InsertText { at, to: None, typing: true, text, .. }]
            if Some(at) == self.typing.as_ref() && !text.contains('\n'));
        let typing = matches!(&commands[..], [Command::InsertText { typing: true, .. }]);
        let mut results = Vec::new();
        let mut caret = None;
        let mut error = None;
        let operation = |state: &mut State| -> std::result::Result<(), DocError> {
            state.selection = Selection::default();
            for command in &commands {
                match run(state, command) {
                    Ok((value, at)) => {
                        results.push(value);
                        if at.is_some() {
                            caret = at;
                        }
                    }
                    Err(e) => {
                        error = Some(e);
                        return Err(DocError::Invalid(String::new()));
                    }
                }
            }
            if let Some(at) = &caret {
                let position = to_position(state, at).map_err(DocError::Invalid)?;
                state.selection = Selection { anchor: Some(position.clone()), focus: Some(position) };
            }
            Ok(())
        };
        let outcome = if amend { self.doc.transact_amend(operation) } else { self.doc.transact(operation) };
        if let Some(e) = error {
            return Err(e);
        }
        outcome.map_err(failed)?;
        self.typing = if typing { caret } else { None };
        Ok(results)
    }

    /// The document as JSON for drawing: blocks (paragraphs with runs, tables with cell text, images without their
    /// bytes), the selection, statistics and whether it can be edited.
    pub(crate) fn snapshot(&self) -> Value {
        let s = self.doc.state();
        let blocks: Vec<Value> = s.blocks.iter().map(block_json).collect();
        let selection = match (&s.selection.anchor, &s.selection.focus) {
            (Some(a), Some(f)) => json!({ "anchor": from_position(s, a), "focus": from_position(s, f) }),
            _ => Value::Null,
        };
        let stats = self.doc.statistics();
        let reason = self.read_only_reason();
        json!({
            "kind": "document",
            "editable": reason.is_none(),
            "reason": reason,
            "blocks": blocks,
            "selection": selection,
            "stats": {
                "words": stats.words,
                "characters": stats.characters,
                "charactersWithoutSpaces": stats.characters_without_spaces,
                "paragraphs": stats.paragraphs,
            },
        })
    }

    /// The bytes of an image block.
    pub fn image(&self, id: u64) -> Result<(String, Vec<u8>)> {
        self.doc
            .state()
            .blocks
            .iter()
            .find_map(|b| match b {
                Block::Image(i) if i.id == id => Some((i.mime.clone(), i.data.clone())),
                _ => None,
            })
            .ok_or_else(|| "No such image".into())
    }

    pub(crate) fn save(&self, to: &str, drop_package_parts: bool) -> Result<Vec<u8>> {
        let to = format(to)?;
        if !drop_package_parts {
            return self.doc.encode(to).map_err(failed);
        }
        let mut state = self.doc.snapshot();
        if to != Format::Docx {
            state.docx_parts.clear();
        }
        if to != Format::Odt {
            state.odt_parts.clear();
        }
        RichDocument::encode_snapshot(state, to).map_err(failed)
    }
}

// MARK: - Positions

fn byte_offset(text: &str, chars: usize) -> Result<usize> {
    if chars == 0 {
        return Ok(0);
    }
    match text.char_indices().nth(chars) {
        Some((b, _)) => Ok(b),
        None if text.chars().count() == chars => Ok(text.len()),
        None => Err(format!("Offset {chars} is beyond the end of the paragraph")),
    }
}

fn char_offset(text: &str, bytes: usize) -> usize {
    text.get(..bytes).map_or_else(|| text.chars().count(), |t| t.chars().count())
}

fn paragraph_text(s: &State, id: u64) -> Result<String> {
    ops::paragraph(s, id).map(ops::text).ok_or_else(|| format!("No paragraph {id}"))
}

fn to_position(s: &State, p: &Pos) -> std::result::Result<Position, String> {
    let text = paragraph_text(s, p.id)?;
    Ok(Position { paragraph: p.id, offset: byte_offset(&text, p.offset)? })
}

fn from_position(s: &State, p: &Position) -> Value {
    let offset = ops::paragraph(s, p.paragraph).map_or(p.offset, |para| char_offset(&ops::text(para), p.offset));
    json!({ "id": p.paragraph, "offset": offset })
}

/// Document order of a body position (block index, offset).
fn order(s: &State, p: &Pos) -> Result<(usize, usize)> {
    let index = ops::body_index(s, p.id).ok_or_else(|| format!("No paragraph {} in the document body", p.id))?;
    Ok((index, p.offset))
}

fn ordered(s: &State, a: &Pos, b: &Pos) -> Result<(Pos, Pos)> {
    if a.id == b.id {
        return Ok(if a.offset <= b.offset { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) });
    }
    Ok(if order(s, a)? <= order(s, b)? { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) })
}

fn delete_between(s: &mut State, from: &Pos, to: &Pos) -> Result<Pos> {
    let (from, to) = ordered(s, from, to)?;
    if from == to {
        return Ok(from);
    }
    let (a, b) = (to_position(s, &from)?, to_position(s, &to)?);
    ops::delete_range(s, &a, &b).map_err(failed)?;
    Ok(from)
}

// MARK: - Styles

fn style_name(p: &Paragraph) -> String {
    if let Some(level) = p.style.outline_level {
        return format!("heading{}", level + 1);
    }
    match &p.style.list {
        Some(l) if l.numbered => "numbered_list".into(),
        Some(_) => "bullet_list".into(),
        None => "normal".into(),
    }
}

fn body_paragraph(s: &State, index: usize) -> Option<&Paragraph> {
    match s.blocks.get(index) {
        Some(Block::Paragraph(p)) => Some(p),
        _ => None,
    }
}

/// Applies a named style to body paragraphs; list paragraphs join an adjacent list of the same kind.
fn set_style(s: &mut State, ids: &[u64], name: StyleName) -> Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let mut indexes = ids
        .iter()
        .map(|id| ops::body_index(s, *id).ok_or_else(|| format!("No paragraph {id} in the document body")))
        .collect::<Result<Vec<_>>>()?;
    indexes.sort_unstable();
    let list_id = match name {
        StyleName::BulletList | StyleName::NumberedList => {
            let numbered = name == StyleName::NumberedList;
            let same = |p: Option<&Paragraph>| p.and_then(|p| p.style.list.clone()).filter(|l| l.numbered == numbered);
            let before = indexes[0].checked_sub(1).and_then(|i| same(body_paragraph(s, i)));
            let after = same(body_paragraph(s, indexes[indexes.len() - 1] + 1));
            let inside = indexes.iter().find_map(|i| same(body_paragraph(s, *i)));
            match before.or(inside).or(after) {
                Some(l) => Some((l.id, numbered)),
                None => Some((ops::allocate_id(s).map_err(failed)?, numbered)),
            }
        }
        _ => None,
    };
    for index in indexes {
        let Some(Block::Paragraph(p)) = s.blocks.get(index) else { continue };
        let id = p.id;
        let mut style = p.style.clone();
        let level = match name {
            StyleName::Heading1 => Some(0),
            StyleName::Heading2 => Some(1),
            StyleName::Heading3 => Some(2),
            _ => None,
        };
        let list = list_id.map(|(id, numbered)| ListMarker {
            id,
            level: style.list.as_ref().map_or(0, |l| l.level),
            numbered,
        });
        if style.outline_level != level || style.list != list {
            // A named Word/OpenDocument paragraph style would keep the old look; the new style replaces it.
            style.source_style = None;
        }
        style.outline_level = level;
        style.list = list;
        ops::set_paragraph_style(s, id, style).map_err(failed)?;
    }
    Ok(())
}

fn set_alignment(s: &mut State, ids: &[u64], align: Align) -> Result<()> {
    for id in ids {
        let mut style = ops::paragraph(s, *id).ok_or_else(|| format!("No paragraph {id}"))?.style.clone();
        style.alignment = match align {
            Align::Left => Alignment::Left,
            Align::Center => Alignment::Center,
            Align::Right => Alignment::Right,
            Align::Justify => Alignment::Justify,
        };
        ops::set_paragraph_style(s, *id, style).map_err(failed)?;
    }
    Ok(())
}

/// Byte ranges of `needle` in `text` (case-insensitive unless `match_case`).
fn matches(text: &str, needle: &str, match_case: bool) -> Vec<std::ops::Range<usize>> {
    if needle.is_empty() {
        return vec![];
    }
    if match_case {
        return text.match_indices(needle).map(|(i, m)| i..i + m.len()).collect();
    }
    // Compare character by character so byte offsets stay those of the original text.
    let needle: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut k = 0;
        let mut j = i;
        while j < chars.len() && k < needle.len() {
            let lower: Vec<char> = chars[j].1.to_lowercase().collect();
            if needle[k..].starts_with(&lower) {
                k += lower.len();
                j += 1;
            } else {
                break;
            }
        }
        if k == needle.len() {
            let end = chars.get(j).map_or(text.len(), |c| c.0);
            found.push(chars[i].0..end);
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    found
}

fn body_ids(s: &State) -> Vec<u64> {
    s.blocks
        .iter()
        .filter_map(|b| match b {
            Block::Paragraph(p) => Some(p.id),
            _ => None,
        })
        .collect()
}

// MARK: - Commands

/// Runs one command; returns its result and the caret it leaves (if any).
fn run(s: &mut State, command: &Command) -> Result<(Value, Option<Pos>)> {
    match command {
        Command::InsertText { at, to, text, marks, .. } => {
            let mut caret = match to {
                Some(to) => delete_between(s, at, to)?,
                None => at.clone(),
            };
            let text = text.replace("\r\n", "\n").replace('\r', "\n");
            if text.contains('\0') {
                return Err("Text cannot contain NUL characters".into());
            }
            let marks = marks.unwrap_or_default();
            let start = to_position(s, &caret)?;
            let base = ops::paragraph(s, caret.id).map(|p| ops::style_at(p, start.offset)).unwrap_or_default();
            let style = marks.apply(&base);
            for (i, line) in text.split('\n').enumerate() {
                if i > 0 {
                    let at = to_position(s, &caret)?;
                    let id = ops::split_paragraph(s, caret.id, at.offset).map_err(failed)?;
                    caret = Pos { id, offset: 0 };
                }
                if !line.is_empty() {
                    let at = to_position(s, &caret)?;
                    ops::replace_text(s, caret.id, at.offset..at.offset, line, style.clone()).map_err(failed)?;
                    caret.offset += line.chars().count();
                }
            }
            Ok((json!({ "caret": caret }), Some(caret)))
        }
        Command::Delete { from, to } => {
            let caret = delete_between(s, from, to)?;
            Ok((json!({ "caret": caret }), Some(caret)))
        }
        Command::SplitParagraph { at, to } => {
            let caret = match to {
                Some(to) => delete_between(s, at, to)?,
                None => at.clone(),
            };
            let p = ops::paragraph(s, caret.id).ok_or_else(|| format!("No paragraph {}", caret.id))?;
            if p.style.list.is_some() && ops::text(p).is_empty() {
                // Enter in an empty list item ends the list.
                set_style(s, &[caret.id], StyleName::Normal)?;
                return Ok((json!({ "caret": caret }), Some(caret)));
            }
            let length = ops::text(p).chars().count();
            let heading = p.style.outline_level.is_some();
            let at = to_position(s, &caret)?;
            let id = ops::split_paragraph(s, caret.id, at.offset).map_err(failed)?;
            if heading && caret.offset == length {
                // After a heading the next paragraph is body text.
                set_style(s, &[id], StyleName::Normal)?;
            }
            let caret = Pos { id, offset: 0 };
            Ok((json!({ "caret": caret }), Some(caret)))
        }
        Command::SetParagraphStyle { ids, style } => {
            set_style(s, ids, *style)?;
            Ok((json!({ "changed": ids.len() }), None))
        }
        Command::SetAlignment { ids, alignment } => {
            set_alignment(s, ids, *alignment)?;
            Ok((json!({ "changed": ids.len() }), None))
        }
        Command::ApplyTextStyle { from, to, ids, find, match_case, bold, italic, underline } => {
            let marks = Marks { bold: *bold, italic: *italic, underline: *underline };
            if marks.is_empty() {
                return Err("Choose bold, italic or underline".into());
            }
            let patch = |style: &TextStyle| marks.apply(style);
            let mut spans: Vec<(u64, std::ops::Range<usize>)> = Vec::new();
            match (from, to) {
                (Some(from), Some(to)) => {
                    let (from, to) = ordered(s, from, to)?;
                    let (a, b) = (order(s, &from)?.0, order(s, &to)?.0);
                    for index in a..=b {
                        let Some(p) = body_paragraph(s, index) else { continue };
                        let text = ops::text(p);
                        let start = if index == a { byte_offset(&text, from.offset)? } else { 0 };
                        let end = if index == b { byte_offset(&text, to.offset)? } else { text.len() };
                        if start < end {
                            spans.push((p.id, start..end));
                        }
                    }
                }
                (None, None) => {
                    let targets = ids.clone().unwrap_or_else(|| body_ids(s));
                    for id in targets {
                        let text = paragraph_text(s, id)?;
                        match find {
                            Some(needle) => {
                                spans.extend(matches(&text, needle, *match_case).into_iter().map(|r| (id, r)))
                            }
                            None if ids.is_some() => spans.push((id, 0..text.len())),
                            None => return Err("Give a range, paragraph ids or text to find".into()),
                        }
                    }
                }
                _ => return Err("Give both ends of the range".into()),
            }
            let count = spans.len();
            for (id, range) in spans {
                ops::style_text(s, id, range, &patch).map_err(failed)?;
            }
            Ok((json!({ "spans": count }), None))
        }
        Command::ReplaceText { id, find, replace } => {
            if replace.contains(['\n', '\r']) {
                return Err("Use insert_paragraphs for new paragraphs".into());
            }
            if find.is_none() && id.is_none() {
                return Err("Give a paragraph id or text to find".into());
            }
            let targets = match id {
                Some(id) => vec![*id],
                None => body_ids(s),
            };
            let mut count = 0;
            for id in targets {
                let text = paragraph_text(s, id)?;
                let ranges = match find {
                    Some(needle) => matches(&text, needle, true),
                    None => vec![0..text.len()],
                };
                // From the end, so earlier offsets stay valid. The replacement takes the style of the first
                // replaced character.
                for range in ranges.into_iter().rev() {
                    let p = ops::paragraph(s, id).ok_or_else(|| format!("No paragraph {id}"))?;
                    let style = ops::style_at(p, range.start + usize::from(range.start < range.end));
                    ops::replace_text(s, id, range, replace, style).map_err(failed)?;
                    count += 1;
                }
            }
            Ok((json!({ "replaced": count }), None))
        }
        Command::InsertParagraphs { after_id, paragraphs } => {
            if paragraphs.is_empty() || paragraphs.len() > 1000 {
                return Err("Insert 1 to 1000 paragraphs".into());
            }
            let mut index = if *after_id == 0 {
                0
            } else {
                ops::body_index(s, *after_id).ok_or_else(|| format!("No paragraph {after_id} in the document body"))? + 1
            };
            let base = match body_paragraph(s, index.saturating_sub(1)).or_else(|| body_paragraph(s, index)) {
                Some(p) => p.runs.first().map(|r| r.style.clone()).unwrap_or_default(),
                None => TextStyle::default(),
            };
            let mut base = base;
            base.bold = false;
            base.italic = false;
            base.underline = false;
            let mut ids = Vec::new();
            for new in paragraphs {
                if new.text.contains(['\n', '\r']) {
                    return Err("Paragraph text cannot contain line breaks".into());
                }
                let style = ParagraphStyle::default();
                let run = Run { text: new.text.clone(), style: base.clone(), link: None };
                let id = ops::insert_paragraph(s, index, style, vec![run]).map_err(failed)?;
                ids.push(id);
                index += 1;
            }
            // Styles in runs of the same name, so consecutive list items share one list.
            let mut i = 0;
            while i < ids.len() {
                let mut j = i + 1;
                while j < ids.len() && paragraphs[j].style == paragraphs[i].style {
                    j += 1;
                }
                set_style(s, &ids[i..j], paragraphs[i].style)?;
                i = j;
            }
            let caret = Pos { id: *ids.last().unwrap(), offset: paragraphs.last().unwrap().text.chars().count() };
            Ok((json!({ "ids": ids }), Some(caret)))
        }
        Command::DeleteParagraphs { ids } => {
            for id in ids {
                let index = ops::body_index(s, *id).ok_or_else(|| format!("No paragraph {id} in the document body"))?;
                if s.blocks.len() == 1 {
                    // The last paragraph is emptied instead.
                    let text = paragraph_text(s, *id)?;
                    let style = ops::paragraph(s, *id).map(|p| ops::style_at(p, 0)).unwrap_or_default();
                    ops::replace_text(s, *id, 0..text.len(), "", style).map_err(failed)?;
                    continue;
                }
                let neighbour = (0..index)
                    .rev()
                    .chain(index + 1..s.blocks.len())
                    .find_map(|i| body_paragraph(s, i).filter(|p| !ids.contains(&p.id)).map(|p| (i, p.id, ops::text(p).len())));
                let to = neighbour.map(|(i, id, len)| Position { paragraph: id, offset: if i < index { len } else { 0 } });
                ops::remove_block(s, index, to).map_err(failed)?;
            }
            Ok((json!({ "deleted": ids.len() }), None))
        }
        command => Ok((read(s, command)?, None)),
    }
}

fn block_preview(index: usize, block: &Block) -> Value {
    match block {
        Block::Paragraph(p) => {
            let mut chars = p.runs.iter().flat_map(|r| r.text.chars());
            let text: String = chars.by_ref().take(100).collect();
            let length = text.chars().count() + chars.count();
            json!({"index":index,"id":p.id,"style":style_name(p),"length":length,"text":text})
        }
        Block::Table(t) => {
            json!({"index":index,"id":t.id,"style":"table","rows":t.rows.len(),"columns":t.rows.first().map_or(0,Vec::len)})
        }
        Block::Image(i) => {
            json!({"index":index,"id":i.id,"style":"image","alt":i.alt.chars().take(80).collect::<String>()})
        }
        Block::SectionBreak => json!({"index":index,"style":"section_break"}),
    }
}

/// Offsets in this protocol count Unicode characters, including in split formatted runs.
fn document_context(s: &State, start: Option<&Pos>, end: Option<&Pos>) -> Result<Value> {
    let total = s.blocks.len();
    let headings: Vec<_> = s
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, block)| matches!(block, Block::Paragraph(p) if p.style.outline_level.is_some()))
        .collect();
    // Sample the heading map across the whole file, including its last section.
    let count = headings.len().min(6);
    let map: Vec<_> = (0..count)
        .map(|n| {
            let (i, b) = headings[n * (headings.len() - 1) / count.saturating_sub(1).max(1)];
            block_preview(i, b)
        })
        .collect();
    let mut focus = 0;
    let mut selection = Value::Null;
    if let (Some(a), Some(b)) = (start, end) {
        let (a, b) = ordered(s, a, b)?;
        focus = order(s, &a)?.0;
        let last = order(s, &b)?.0;
        let mut selected = s.blocks[focus..=last]
            .iter()
            .filter_map(|block| match block {
                Block::Paragraph(p) => Some(p),
                _ => None,
            })
            .enumerate()
            .flat_map(|(n, p)| {
                let from = if p.id == a.id { a.offset } else { 0 };
                let until = if p.id == b.id { b.offset } else { usize::MAX };
                (n != 0)
                    .then_some('\n')
                    .into_iter()
                    .chain(p.runs.iter().flat_map(|r| r.text.chars()).skip(from).take(until.saturating_sub(from)))
            });
        let text: String = selected.by_ref().take(320).collect();
        selection = json!({"start":a,"end":b,"text":text,"truncated":selected.next().is_some()});
        // The block preview starts at the top. A caret deep in a long paragraph needs the words at the caret.
        if a.offset > 100 {
            if let Block::Paragraph(paragraph) = &s.blocks[focus] {
                let from = a.offset.saturating_sub(64);
                let excerpt: String = paragraph.runs.iter().flat_map(|r| r.text.chars()).skip(from).take(128).collect();
                selection["near_start"] = json!({"offset":from,"text":excerpt});
            }
        }
    }
    let near: Vec<_> = s
        .blocks
        .iter()
        .enumerate()
        .skip(focus.saturating_sub(1))
        .take(if focus == 0 { 2 } else { 3 })
        .map(|(i, b)| block_preview(i, b))
        .collect();
    let section = headings.iter().rev().find(|(i, _)| *i <= focus).map(|(i, b)| block_preview(*i, b));
    Ok(json!({"kind":"document","block_count":total,"selection":selection,"section":section,
        "near_selection":near,"headings":map,"headings_omitted":headings.len()-count}))
}

fn read(s: &State, command: &Command) -> Result<Value> {
    match command {
        Command::DocumentContext { start, end } => document_context(s, start.as_ref(), end.as_ref()),
        Command::GetOutline { offset, limit } => {
            let offset = offset.unwrap_or(0);
            let limit = limit.unwrap_or(s.blocks.len());
            let paragraphs: Vec<_> =
                s.blocks.iter().enumerate().skip(offset).take(limit).map(|(i, b)| block_preview(i, b)).collect();
            let end = offset.saturating_add(paragraphs.len());
            Ok(
                json!({"paragraphs":paragraphs,"block_count":s.blocks.len(),"next_offset":(end<s.blocks.len()).then_some(end)}),
            )
        }
        Command::ReadRange { ids, start, length } => {
            if ids.len() > 50 {
                return Err("Read at most 50 paragraphs at a time".into());
            }
            let from = start.unwrap_or(0);
            let to = from.saturating_add(length.unwrap_or(usize::MAX));
            let items = ids.iter().map(|id| {
                let p = ops::paragraph(s,*id).ok_or_else(|| format!("No paragraph {id}"))?;
                let mut spans = Vec::new();
                let mut text = String::new();
                let mut pos = 0;
                for run in &p.runs {
                    let len = run.text.chars().count();
                    let left = from.max(pos);
                    let right = to.min(pos+len);
                    if left < right {
                        text.extend(run.text.chars().skip(left-pos).take(right-left));
                        let st = &run.style;
                        if st.bold || st.italic || st.underline {
                            spans.push(json!({"start":left,"end":right,"bold":st.bold,"italic":st.italic,"underline":st.underline}));
                        }
                    }
                    pos += len;
                }
                Ok(json!({"id":id,"style":style_name(p),"text":text,"spans":spans,"start":from,"length":pos,
                    "next_start":(to<pos).then_some(to)}))
            }).collect::<Result<Vec<_>>>()?;
            Ok(json!({"paragraphs":items}))
        }
        Command::Find { text, match_case } => {
            if text.is_empty() {
                return Err("Give text to find".into());
            }
            let mut found = Vec::new();
            'outer: for block in &s.blocks {
                let Block::Paragraph(p) = block else { continue };
                let id = p.id;
                let all = ops::text(p);
                for range in matches(&all, text, *match_case) {
                    if found.len() == 50 {
                        break 'outer;
                    }
                    let offset = char_offset(&all, range.start);
                    let before: String =
                        all[..range.start].chars().rev().take(30).collect::<Vec<_>>().into_iter().rev().collect();
                    let after: String = all[range.end..].chars().take(30).collect();
                    found.push(json!({ "id": id, "offset": offset, "context": format!("{before}{}{after}", &all[range.clone()]) }));
                }
            }
            Ok(json!({ "matches": found }))
        }
        _ => Err("Not a document read command".into()),
    }
}

// MARK: - Snapshot

fn color(value: Option<u32>) -> Value {
    value.map_or(Value::Null, |c| format!("#{:06x}", c & 0xff_ffff).into())
}

fn paragraph_json(p: &Paragraph) -> Value {
    let runs: Vec<Value> = p
        .runs
        .iter()
        .filter(|r| !r.text.is_empty())
        .map(|r| {
            let st = &r.style;
            let mut run = serde_json::Map::new();
            run.insert("text".into(), r.text.clone().into());
            for (key, on) in [("bold", st.bold), ("italic", st.italic), ("underline", st.underline), ("strike", st.strike)] {
                if on {
                    run.insert(key.into(), true.into());
                }
            }
            if st.size_half_points != 24 {
                run.insert("size".into(), (f64::from(st.size_half_points) / 2.0).into());
            }
            if !st.font.is_empty() {
                run.insert("font".into(), st.font.clone().into());
            }
            if st.color.is_some() {
                run.insert("color".into(), color(st.color));
            }
            if st.highlight.is_some() {
                run.insert("highlight".into(), color(st.highlight));
            }
            if let Some(link) = &r.link {
                run.insert("link".into(), link.clone().into());
            }
            Value::Object(run)
        })
        .collect();
    let align = match p.style.alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "justify",
    };
    json!({
        "type": "paragraph",
        "id": p.id,
        "style": style_name(p),
        "level": p.style.outline_level,
        "list": p.style.list.as_ref().map(|l| json!({ "id": l.id, "numbered": l.numbered, "level": l.level })),
        "align": align,
        "indent": p.style.left_twips.max(0) / 20,
        "runs": runs,
    })
}

fn block_json(block: &Block) -> Value {
    match block {
        Block::Paragraph(p) => paragraph_json(p),
        Block::Table(t) => json!({
            "type": "table",
            "id": t.id,
            "rows": t.rows.iter().map(|row| row.iter().map(|cell| json!({
                "paragraphs": cell.paragraphs.iter().map(paragraph_json).collect::<Vec<_>>(),
                "rowSpan": cell.row_span,
                "colSpan": cell.col_span,
                "fill": color(cell.fill),
            })).collect::<Vec<_>>()).collect::<Vec<_>>(),
        }),
        Block::Image(i) => json!({
            "type": "image",
            "id": i.id,
            "mime": i.mime,
            "alt": i.alt,
            "width": f64::from(i.width_twips) / 15.0,
            "height": f64::from(i.height_twips) / 15.0,
        }),
        Block::SectionBreak => json!({ "type": "break" }),
    }
}
