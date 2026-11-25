//! Stable anchors for comments and notes in the authoritative rich document.
use super::*;
use std::collections::HashMap;

fn paragraph_texts(state: &State) -> HashMap<u64, String> {
    let mut out = HashMap::new();
    let mut add = |p: &Paragraph| {
        out.insert(p.id, p.runs.iter().map(|r| r.text.as_str()).collect());
    };
    for block in &state.blocks {
        match block {
            Block::Paragraph(p) => add(p),
            Block::Table(t) => {
                for p in t.rows.iter().flatten().flat_map(|c| &c.paragraphs) {
                    add(p);
                }
            }
            _ => {}
        }
    }
    for section in &state.sections {
        for p in section.header.iter().chain(&section.footer) {
            add(p);
        }
    }
    for note in &state.footnotes {
        for p in &note.paragraphs {
            add(p);
        }
    }
    out
}
fn same_topology(old: &State, new: &State) -> bool {
    old.blocks.len() == new.blocks.len()
        && old
            .blocks
            .iter()
            .zip(&new.blocks)
            .all(|(a, b)| match (a, b) {
                (Block::Paragraph(_), Block::Paragraph(_)) | (Block::Image(_), Block::Image(_)) => {
                    true
                }
                (Block::Table(a), Block::Table(b)) => {
                    a.rows.len() == b.rows.len()
                        && a.rows.iter().zip(&b.rows).all(|(a, b)| {
                            a.len() == b.len()
                                && a.iter()
                                    .zip(b)
                                    .all(|(a, b)| a.paragraphs.len() == b.paragraphs.len())
                        })
                }
                _ => false,
            })
}
fn changed_range(old: &str, new: &str) -> (usize, usize, usize) {
    let prefix: usize = old
        .chars()
        .zip(new.chars())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum();
    let old_tail = &old[prefix..];
    let new_tail = &new[prefix..];
    let suffix: usize = old_tail
        .chars()
        .rev()
        .zip(new_tail.chars().rev())
        .take_while(|(a, b)| a == b)
        .map(|(c, _)| c.len_utf8())
        .sum();
    (prefix, old.len() - suffix, new.len() - prefix - suffix)
}
fn remap_offset(offset: usize, start: usize, end: usize, inserted: usize, right: bool) -> usize {
    if offset < start {
        offset
    } else if offset > end {
        start + inserted + offset - end
    } else if offset == end && end != start {
        start + inserted
    } else if right {
        start + inserted
    } else {
        start
    }
}
fn remap_position(
    position: &mut Position,
    old: &HashMap<u64, String>,
    new: &HashMap<u64, String>,
    right: bool,
) -> Result<(), Error> {
    let before = old
        .get(&position.paragraph)
        .ok_or_else(|| Error::Invalid("missing annotation anchor".into()))?;
    let after = new
        .get(&position.paragraph)
        .ok_or_else(|| Error::Unsupported("editing would remove an annotation anchor".into()))?;
    let (start, end, inserted) = changed_range(before, after);
    position.offset = remap_offset(position.offset, start, end, inserted, right);
    Ok(())
}
struct LinearBody {
    text: String,
    spans: Vec<(u64, usize, usize)>,
    objects: Vec<(u8, u64)>,
}
fn linear_body(state: &State) -> Option<LinearBody> {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut objects = Vec::new();
    for (i, block) in state.blocks.iter().enumerate() {
        if i != 0 {
            text.push('\n');
        }
        match block {
            Block::Paragraph(paragraph) => {
                let start = text.len();
                for run in &paragraph.runs {
                    text.push_str(&run.text);
                }
                spans.push((paragraph.id, start, text.len()));
            }
            Block::Table(table) => {
                text.push('\u{fffc}');
                objects.push((0, table.id));
            }
            Block::Image(image) => {
                text.push('\u{fffc}');
                objects.push((1, image.id));
            }
            _ => return None,
        }
    }
    Some(LinearBody {
        text,
        spans,
        objects,
    })
}
fn global_position(body: &LinearBody, position: &Position) -> Option<usize> {
    body.spans
        .iter()
        .find(|(id, _, _)| *id == position.paragraph)
        .and_then(|(_, start, end)| {
            (position.offset <= end - start).then_some(start + position.offset)
        })
}
fn local_position(body: &LinearBody, offset: usize) -> Result<Position, Error> {
    for &(id, start, end) in &body.spans {
        if offset >= start && offset <= end {
            return Ok(Position {
                paragraph: id,
                offset: offset - start,
            });
        }
    }
    if let Some(&(id, start, end)) = body.spans.iter().rev().find(|(_, _, end)| *end < offset) {
        return Ok(Position {
            paragraph: id,
            offset: end - start,
        });
    }
    if let Some(&(id, _, _)) = body.spans.iter().find(|(_, start, _)| *start > offset) {
        return Ok(Position {
            paragraph: id,
            offset: 0,
        });
    }
    Err(Error::Unsupported("no text remains for annotation".into()))
}
/// Re-anchor a single contiguous edit in linear text, including paragraph
/// splits and joins. For structured blocks, retain only unambiguous identities.
pub(super) fn remap_projection(old: &State, parsed: &mut State) -> Result<(), Error> {
    parsed.comments = old.comments.clone();
    parsed.footnotes = old.footnotes.clone();
    let before = paragraph_texts(old);
    let after = paragraph_texts(parsed);
    if let (Some(from), Some(to)) = (linear_body(old), linear_body(parsed)) {
        if from.objects != to.objects {
            return Err(Error::Unsupported(
                "structural edits around annotated objects".into(),
            ));
        }
        let (start, end, inserted) = changed_range(&from.text, &to.text);
        for comment in &mut parsed.comments {
            let collapsed = comment.start == comment.end;
            let insertion = start == end;
            for (position, right) in [
                (&mut comment.start, insertion || collapsed),
                (&mut comment.end, !insertion || collapsed),
            ] {
                if let Some(offset) = global_position(&from, position) {
                    *position =
                        local_position(&to, remap_offset(offset, start, end, inserted, right))?;
                } else {
                    remap_position(position, &before, &after, right)?;
                }
            }
        }
        let mut retained = Vec::new();
        for mut note in std::mem::take(&mut parsed.footnotes) {
            if let Some(offset) = global_position(&from, &note.at) {
                // Deleting the marker's position removes the note; Undo restores it.
                if start < end && offset >= start && offset < end {
                    continue;
                }
                note.at = local_position(&to, remap_offset(offset, start, end, inserted, true))?;
            } else {
                remap_position(&mut note.at, &before, &after, true)?;
            }
            retained.push(note);
        }
        parsed.footnotes = retained;
        return Ok(());
    }
    if (!old.comments.is_empty() || !old.footnotes.is_empty()) && !same_topology(old, parsed) {
        return Err(Error::Unsupported(
            "structural edits around annotated objects".into(),
        ));
    }
    for comment in &mut parsed.comments {
        let collapsed = comment.start == comment.end;
        let insertion = before
            .get(&comment.start.paragraph)
            .zip(after.get(&comment.start.paragraph))
            .is_some_and(|(a, b)| {
                let (start, end, _) = changed_range(a, b);
                start == end
            });
        remap_position(&mut comment.start, &before, &after, insertion || collapsed)?;
        remap_position(&mut comment.end, &before, &after, !insertion || collapsed)?;
    }
    for note in &mut parsed.footnotes {
        remap_position(&mut note.at, &before, &after, true)?;
    }
    Ok(())
}
/// Remap a known edit, with start and end gravity for comment ranges.
pub(super) fn remap_edit(
    state: &mut State,
    paragraph: u64,
    range: std::ops::Range<usize>,
    inserted: usize,
) {
    let apply = |position: &mut Position, right| {
        if position.paragraph == paragraph {
            position.offset =
                remap_offset(position.offset, range.start, range.end, inserted, right);
        }
    };
    for bookmark in &mut state.bookmarks {
        let collapsed = bookmark.start == bookmark.end;
        let insertion = range.start == range.end;
        apply(&mut bookmark.start, insertion || collapsed);
        apply(&mut bookmark.end, !insertion || collapsed);
    }
    for comment in &mut state.comments {
        let collapsed = comment.start == comment.end;
        let insertion = range.start == range.end;
        apply(&mut comment.start, insertion || collapsed);
        apply(&mut comment.end, !insertion || collapsed);
    }
    state.footnotes.retain(|note| {
        !(note.at.paragraph == paragraph
            && range.start < range.end
            && note.at.offset >= range.start
            && note.at.offset < range.end)
    });
    for note in &mut state.footnotes {
        apply(&mut note.at, true);
    }
}
#[derive(Clone, Debug)]
pub struct ProjectedNote {
    pub marker: String,
    pub label: String,
    pub endnote: bool,
    pub paragraphs: Vec<Paragraph>,
}

fn rewrite_note_paragraph(old: &Paragraph, text: &str) -> Result<Paragraph, Error> {
    let before: String = old.runs.iter().map(|r| r.text.as_str()).collect();
    let (start, end, inserted) = changed_range(&before, text);
    let mut output = Vec::new();
    let mut cursor = 0usize;
    let mut inherited = None;
    for run in &old.runs {
        let next = cursor + run.text.len();
        if inherited.is_none() && start <= next {
            inherited = Some((run.style.clone(), run.link.clone()));
        }
        if cursor < start {
            let cut = start.min(next) - cursor;
            if cut > 0 {
                let mut part = run.clone();
                part.text = run
                    .text
                    .get(..cut)
                    .ok_or_else(|| Error::Invalid("note text boundary".into()))?
                    .into();
                output.push(part);
            }
        }
        cursor = next;
    }
    if inserted > 0 {
        let (style, link) = inherited.unwrap_or_default();
        output.push(Run {
            text: text
                .get(start..start + inserted)
                .ok_or_else(|| Error::Invalid("note text boundary".into()))?
                .into(),
            style,
            link,
        });
    }
    cursor = 0;
    for run in &old.runs {
        let next = cursor + run.text.len();
        if next > end {
            let cut = end.saturating_sub(cursor);
            let mut part = run.clone();
            part.text = run
                .text
                .get(cut..)
                .ok_or_else(|| Error::Invalid("note text boundary".into()))?
                .into();
            if !part.text.is_empty() {
                output.push(part);
            }
        }
        cursor = next;
    }
    if output.is_empty() {
        output.push(Run {
            text: String::new(),
            style: old
                .runs
                .first()
                .map(|r| r.style.clone())
                .unwrap_or_default(),
            link: None,
        });
    }
    Ok(Paragraph {
        id: old.id,
        style: old.style.clone(),
        runs: output,
    })
}

impl RichDocument {
    /// Produce a print-only RTF view with visible note markers without mutating
    /// document text. Unique invisible tags let the native host locate markers
    /// in RichEdit even when tables or pictures precede the anchor.
    pub fn note_print_projection(&self) -> Result<(Vec<u8>, Vec<ProjectedNote>), Error> {
        let mut paragraphs = Vec::new();
        for block in &self.state().blocks {
            match block {
                Block::Paragraph(p) => paragraphs.push(p),
                Block::Table(t) => {
                    for p in t.rows.iter().flatten().flat_map(|c| &c.paragraphs) {
                        paragraphs.push(p);
                    }
                }
                Block::Image(_) => {}
                _ => {
                    return Err(Error::Unsupported(
                        "printing notes in section breaks".into(),
                    ))
                }
            }
        }
        let source = paragraphs
            .iter()
            .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join("\0");
        let mut raw = Vec::new();
        for (index, note) in self.state().footnotes.iter().enumerate() {
            let order = paragraphs
                .iter()
                .position(|p| p.id == note.at.paragraph)
                .ok_or_else(|| Error::Unsupported("printing a note outside body text".into()))?;
            let text: String = paragraphs[order]
                .runs
                .iter()
                .map(|r| r.text.as_str())
                .collect();
            if note.at.offset > text.len() || !text.is_char_boundary(note.at.offset) {
                return Err(Error::Invalid("note anchor".into()));
            }
            raw.push((order, note.at.offset, index, note.clone()));
        }
        raw.sort_by_key(|(order, offset, index, _)| (*order, *offset, *index));
        let mut foot = 0usize;
        let mut end = 0usize;
        let mut entries = Vec::new();
        for (order, offset, index, note) in raw {
            let number = if note.endnote {
                end += 1;
                end
            } else {
                foot += 1;
                foot
            };
            let label = note.citation.clone().unwrap_or_else(|| number.to_string());
            let visible = label
                .chars()
                .map(|c| match c {
                    '0' => '⁰',
                    '1' => '¹',
                    '2' => '²',
                    '3' => '³',
                    '4' => '⁴',
                    '5' => '⁵',
                    '6' => '⁶',
                    '7' => '⁷',
                    '8' => '⁸',
                    '9' => '⁹',
                    _ => c,
                })
                .collect::<String>();
            if visible.is_empty() || visible.chars().any(char::is_control) {
                return Err(Error::Unsupported("note citation in print".into()));
            }
            let code = format!("{index:b}")
                .chars()
                .map(|c| if c == '0' { '\u{2060}' } else { '\u{2064}' })
                .collect::<String>();
            let mut length = 8usize;
            let marker = loop {
                if length > 128 {
                    return Err(Error::Unsupported(
                        "cannot construct unique note marker".into(),
                    ));
                }
                let candidate = format!(
                    "{}{code}\u{2063}{visible}\u{2063}",
                    "\u{2063}".repeat(length)
                );
                if !source.contains(&candidate) {
                    break candidate;
                }
                length += 1;
            };
            entries.push((order, offset, index, note, label, marker));
        }
        let projected = entries
            .iter()
            .map(|(_, _, _, note, label, marker)| ProjectedNote {
                marker: marker.clone(),
                label: label.clone(),
                endnote: note.endnote,
                paragraphs: note.paragraphs.clone(),
            })
            .collect();
        let mut snapshot = self.snapshot();
        snapshot.comments.clear();
        snapshot.footnotes.clear();
        snapshot.revisions.clear();
        let mut view = RichDocument {
            history: SnapshotHistory::new(snapshot, 0),
        };
        for (_, _, _, note, _, marker) in entries.into_iter().rev() {
            let style = view
                .state()
                .blocks
                .iter()
                .find_map(|b| match b {
                    Block::Paragraph(p) if p.id == note.at.paragraph => {
                        p.runs.first().map(|r| r.style.clone())
                    }
                    Block::Table(t) => t
                        .rows
                        .iter()
                        .flatten()
                        .flat_map(|c| &c.paragraphs)
                        .find(|p| p.id == note.at.paragraph)
                        .and_then(|p| p.runs.first().map(|r| r.style.clone())),
                    _ => None,
                })
                .unwrap_or_default();
            view.replace_text(
                note.at.paragraph,
                note.at.offset..note.at.offset,
                &marker,
                style,
                Selection::default(),
            )?;
        }
        Ok((view.rich_edit_projection()?, projected))
    }
    pub fn add_note(&mut self, at: Position, text: String, endnote: bool) -> Result<u64, Error> {
        if text.trim().is_empty() {
            return Err(Error::Invalid("empty note".into()));
        }
        let id = self.state().next_id;
        self.transact(|s| {
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            let mut paragraphs = Vec::new();
            for line in text.replace("\r\n", "\n").split('\n') {
                let paragraph = s.next_id;
                s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                paragraphs.push(Paragraph {
                    id: paragraph,
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: line.into(),
                        style: TextStyle::default(),
                        link: None,
                    }],
                });
            }
            s.footnotes.push(Footnote {
                id,
                at,
                paragraphs,
                endnote,
                citation: None,
            });
            Ok(())
        })?;
        Ok(id)
    }
    pub fn edit_note(&mut self, id: u64, text: String) -> Result<bool, Error> {
        if text.trim().is_empty() {
            return Err(Error::Invalid("empty note".into()));
        }
        self.transact(|s| {
            let note = s
                .footnotes
                .iter_mut()
                .find(|n| n.id == id)
                .ok_or_else(|| Error::Invalid("note not found".into()))?;
            let lines = text.replace("\r\n", "\n");
            let mut paragraphs = Vec::new();
            let mut edits = Vec::new();
            for (i, line) in lines.split('\n').enumerate() {
                if let Some(old) = note.paragraphs.get(i) {
                    let before: String = old.runs.iter().map(|r| r.text.as_str()).collect();
                    let (start, end, inserted) = changed_range(&before, line);
                    edits.push((old.id, start..end, inserted));
                    paragraphs.push(rewrite_note_paragraph(old, line)?);
                } else {
                    let id = s.next_id;
                    s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                    paragraphs.push(Paragraph {
                        id,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: line.into(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    });
                }
            }
            note.paragraphs = paragraphs;
            for (paragraph, range, inserted) in edits {
                remap_edit(s, paragraph, range, inserted);
            }
            Ok(())
        })
    }
    pub fn delete_note(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| {
            let index = s
                .footnotes
                .iter()
                .position(|n| n.id == id)
                .ok_or_else(|| Error::Invalid("note not found".into()))?;
            s.footnotes.remove(index);
            Ok(())
        })
    }
    pub fn add_comment(
        &mut self,
        start: Position,
        end: Position,
        author: String,
        text: String,
    ) -> Result<u64, Error> {
        if start.paragraph != end.paragraph || start.offset > end.offset || text.trim().is_empty() {
            return Err(Error::Invalid(
                "select one paragraph and enter comment text".into(),
            ));
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let id = self.state().next_id;
        self.transact(|s| {
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            s.comments.push(Annotation {
                id,
                start,
                end,
                author,
                text,
                resolved: false,
                date: None,
                initials: None,
            });
            Ok(())
        })?;
        Ok(id)
    }
    pub fn edit_comment(&mut self, id: u64, text: String, resolved: bool) -> Result<bool, Error> {
        if text.trim().is_empty() {
            return Err(Error::Invalid("empty comment".into()));
        }
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.transact(|s| {
            let comment = s
                .comments
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or_else(|| Error::Invalid("comment not found".into()))?;
            comment.text = text;
            comment.resolved = resolved;
            Ok(())
        })
    }
    pub fn delete_comment(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| {
            let index = s
                .comments
                .iter()
                .position(|c| c.id == id)
                .ok_or_else(|| Error::Invalid("comment not found".into()))?;
            s.comments.remove(index);
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn projected(text: &str) -> Vec<u8> {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text(text).unwrap();
        doc.rich_edit_projection().unwrap()
    }
    #[test]
    fn print_projection_inserts_ordered_markers_without_mutating_document() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("A🙂\nB").unwrap();
        let Block::Paragraph(second) = &doc.state().blocks[1] else {
            panic!()
        };
        let second_id = second.id;
        doc.add_note(
            Position {
                paragraph: second_id,
                offset: 0,
            },
            "later".into(),
            false,
        )
        .unwrap();
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 5,
            },
            "first".into(),
            false,
        )
        .unwrap();
        let before = doc.snapshot();
        let (rtf, notes) = doc.note_print_projection().unwrap();
        assert_eq!(
            notes.iter().map(|n| n.label.as_str()).collect::<Vec<_>>(),
            ["1", "2"]
        );
        let projected = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        assert_eq!(
            projected
                .editor_text()
                .unwrap()
                .replace("\r\n", "\n")
                .replace(['\u{2060}', '\u{2063}', '\u{2064}'], ""),
            "A🙂¹\n²B"
        );
        assert_eq!(doc.state(), &before);
    }
    #[test]
    fn note_projection_keeps_table_and_image_and_anchors_in_cells() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("body").unwrap();
        doc.transact(|s| {
            let table_id = s.next_id;
            s.next_id += 1;
            let cell_id = s.next_id;
            s.next_id += 1;
            s.blocks.push(Block::Table(Table {
                id: table_id,
                column_widths_twips: Vec::new(),
                rows: vec![vec![Cell {
                    paragraphs: vec![Paragraph {
                        id: cell_id,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: "cell".into(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    }],
                    row_span: 1,
                    col_span: 1,
                    border: None,
                    fill: None,
                }]],
            }));
            let image_id = s.next_id;
            s.next_id += 1;
            s.blocks.push(Block::Image(Image {
                id: image_id,
                mime: "image/png".into(),
                data: vec![137, 80, 78, 71, 13, 10, 26, 10, 0, 0],
                width_twips: 1440,
                height_twips: 720,
                alt: String::new(),
            }));
            Ok(())
        })
        .unwrap();
        let Block::Table(t) = &doc.state().blocks[1] else {
            panic!()
        };
        let cell_id = t.rows[0][0].paragraphs[0].id;
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 4,
            },
            "body note".into(),
            false,
        )
        .unwrap();
        doc.add_note(
            Position {
                paragraph: cell_id,
                offset: 2,
            },
            "cell note".into(),
            false,
        )
        .unwrap();
        let original = doc.snapshot();
        let (rtf, notes) = doc.note_print_projection().unwrap();
        let projected = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        let body: String = projected
            .state()
            .blocks
            .iter()
            .flat_map(|b| match b {
                Block::Paragraph(p) => vec![p],
                Block::Table(t) => t
                    .rows
                    .iter()
                    .flatten()
                    .flat_map(|c| c.paragraphs.iter())
                    .collect(),
                _ => Vec::new(),
            })
            .flat_map(|p| p.runs.iter().map(|r| r.text.as_str()))
            .collect();
        for note in &notes {
            assert!(body.contains(&note.marker));
        }
        assert!(projected
            .state()
            .blocks
            .iter()
            .any(|b| matches!(b, Block::Image(_))));
        assert_eq!(doc.state(), &original);
    }
    #[test]
    fn footnotes_and_endnotes_roundtrip_odt_with_anchors() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("hello world").unwrap();
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 5,
            },
            "First note\nSecond line".into(),
            false,
        )
        .unwrap();
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 11,
            },
            "End note".into(),
            true,
        )
        .unwrap();
        for format in [Format::Project, Format::Docx, Format::Odt] {
            let bytes = doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            assert_eq!(reopened.state().footnotes.len(), 2);
            assert_eq!(
                (
                    reopened.state().footnotes[0].at.offset,
                    reopened.state().footnotes[0].endnote
                ),
                (5, false)
            );
            assert_eq!(
                (
                    reopened.state().footnotes[1].at.offset,
                    reopened.state().footnotes[1].endnote
                ),
                (11, true)
            );
            assert_eq!(
                reopened.state().footnotes[0]
                    .paragraphs
                    .iter()
                    .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
                    .collect::<Vec<_>>(),
                ["First note", "Second line"]
            );
            let saved = RichDocument::decode(&reopened.encode(format).unwrap(), format).unwrap();
            assert_eq!(saved.state().footnotes.len(), 2);
        }
    }
    #[test]
    fn splitting_text_before_table_preserves_body_and_cell_comment_anchors() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("one two").unwrap();
        doc.transact(|s| {
            let table = s.next_id;
            s.next_id += 1;
            let paragraph = s.next_id;
            s.next_id += 1;
            s.blocks.push(Block::Table(Table {
                id: table,
                column_widths_twips: Vec::new(),
                rows: vec![vec![Cell {
                    paragraphs: vec![Paragraph {
                        id: paragraph,
                        style: ParagraphStyle::default(),
                        runs: vec![Run {
                            text: "cell".into(),
                            style: TextStyle::default(),
                            link: None,
                        }],
                    }],
                    row_span: 1,
                    col_span: 1,
                    border: None,
                    fill: None,
                }]],
            }));
            Ok(())
        })
        .unwrap();
        doc.add_comment(
            Position {
                paragraph: 1,
                offset: 4,
            },
            Position {
                paragraph: 1,
                offset: 7,
            },
            "A".into(),
            "body".into(),
        )
        .unwrap();
        let Block::Table(table) = &doc.state().blocks[1] else {
            panic!()
        };
        let table_id = table.id;
        let cell_id = table.rows[0][0].paragraphs[0].id;
        doc.add_comment(
            Position {
                paragraph: cell_id,
                offset: 0,
            },
            Position {
                paragraph: cell_id,
                offset: 4,
            },
            "A".into(),
            "cell".into(),
        )
        .unwrap();
        let original = doc.snapshot();
        let mut view =
            RichDocument::decode(&doc.encode(Format::Project).unwrap(), Format::Project).unwrap();
        view.transact(|s| {
            let Block::Paragraph(p) = &mut s.blocks[0] else {
                panic!()
            };
            p.runs[0].text = "one ".into();
            let id = s.next_id;
            s.next_id += 1;
            s.blocks.insert(
                1,
                Block::Paragraph(Paragraph {
                    id,
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "two".into(),
                        style: TextStyle::default(),
                        link: None,
                    }],
                }),
            );
            s.comments.clear();
            Ok(())
        })
        .unwrap();
        doc.apply_rich_edit_projection(&view.rich_edit_projection().unwrap(), (0, 0))
            .unwrap();
        let Block::Paragraph(second) = &doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(
            doc.state().comments[0].start,
            Position {
                paragraph: second.id,
                offset: 0
            }
        );
        let Block::Table(table) = &doc.state().blocks[2] else {
            panic!()
        };
        assert_eq!(table.id, table_id);
        assert_eq!(doc.state().comments[1].start.paragraph, cell_id);
        assert!(doc.undo());
        assert_eq!(doc.state(), &original);
    }
    #[test]
    fn comments_and_notes_follow_splits_joins_deletion_and_undo() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("one two").unwrap();
        doc.add_comment(
            Position {
                paragraph: 1,
                offset: 4,
            },
            Position {
                paragraph: 1,
                offset: 7,
            },
            "Author".into(),
            "Review".into(),
        )
        .unwrap();
        doc.transact(|s| {
            let note = s.next_id;
            s.next_id += 1;
            let paragraph = s.next_id;
            s.next_id += 1;
            s.footnotes.push(Footnote {
                id: note,
                endnote: false,
                citation: None,
                at: Position {
                    paragraph: 1,
                    offset: 5,
                },
                paragraphs: vec![Paragraph {
                    id: paragraph,
                    style: ParagraphStyle::default(),
                    runs: vec![Run {
                        text: "note".into(),
                        style: TextStyle::default(),
                        link: None,
                    }],
                }],
            });
            Ok(())
        })
        .unwrap();
        let original = doc.snapshot();
        doc.apply_rich_edit_projection(&projected("one \ntwo"), (0, 0))
            .unwrap();
        let Block::Paragraph(second) = &doc.state().blocks[1] else {
            panic!()
        };
        assert_eq!(
            doc.state().comments[0].start,
            Position {
                paragraph: second.id,
                offset: 0
            }
        );
        assert_eq!(
            doc.state().comments[0].end,
            Position {
                paragraph: second.id,
                offset: 3
            }
        );
        assert_eq!(
            doc.state().footnotes[0].at,
            Position {
                paragraph: second.id,
                offset: 1
            }
        );
        doc.apply_rich_edit_projection(&projected("one two"), (0, 0))
            .unwrap();
        assert_eq!(
            doc.state().comments[0].start,
            Position {
                paragraph: 1,
                offset: 4
            }
        );
        assert_eq!(
            doc.state().footnotes[0].at,
            Position {
                paragraph: 1,
                offset: 5
            }
        );
        doc.apply_rich_edit_projection(&projected("one "), (0, 0))
            .unwrap();
        assert!(doc.state().footnotes.is_empty());
        assert_eq!(doc.state().comments[0].start.offset, 4);
        assert_eq!(doc.state().comments[0].end.offset, 4);
        assert!(doc.undo());
        assert_eq!(doc.state().footnotes.len(), 1);
        assert!(doc.undo());
        assert!(doc.undo());
        assert_eq!(doc.state(), &original);
    }
}
