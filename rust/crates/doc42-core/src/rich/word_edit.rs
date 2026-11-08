//! Small text corrections against an immutable native projection, without importing its font substitutions.
use super::*;
pub struct PreparedWordEdit {
    source: State,
    source_id: u64,
    after: State,
    projection: Vec<u8>,
    paragraph: u64,
    runs: Vec<(usize, String)>,
}
impl PreparedWordEdit {
    pub fn paragraph(&self) -> u64 {
        self.paragraph
    }
    /// Source run indices and replacement text; adapters retain their own richer formatting metadata.
    pub fn run_texts(&self) -> &[(usize, String)] {
        &self.runs
    }
    pub fn projection(&self) -> &[u8] {
        &self.projection
    }
}
fn structural(c: u16) -> bool {
    matches!(c, 7 | 9 | 10 | 11 | 12 | 13 | 0xFFFC | 0xFDD0..=0xFDEF)
}
fn paragraphs(state: &State) -> impl Iterator<Item = &Paragraph> {
    state.blocks.iter().flat_map(|block| match block {
        Block::Paragraph(p) => vec![p],
        Block::Table(t) => t
            .rows
            .iter()
            .flatten()
            .flat_map(|cell| &cell.paragraphs)
            .collect(),
        _ => vec![],
    })
}
fn byte_at(text: &str, offset: usize) -> Option<usize> {
    let mut units = 0;
    for (i, c) in text.char_indices() {
        if units == offset {
            return Some(i);
        }
        units += c.len_utf16();
        if units > offset {
            return None;
        }
    }
    (units == offset).then_some(text.len())
}
fn locate(
    state: &State,
    native: &[u16],
    range: (usize, usize),
) -> Result<(u64, std::ops::Range<usize>), Error> {
    let invalid = || Error::Invalid("native word projection no longer matches the document".into());
    let mut cursor = 0usize;
    let mut target = None;
    let mut work = 0usize;
    for p in paragraphs(state) {
        let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        if text.is_empty() {
            continue;
        }
        let projected: Vec<_> = text
            .encode_utf16()
            .map(|c| if c == 10 { 11 } else { c })
            .collect();
        loop {
            work = work
                .checked_add(projected.len())
                .filter(|n| *n <= 32_000_000)
                .ok_or(Error::Limit)?;
            if native.get(cursor..cursor.saturating_add(projected.len()))
                == Some(projected.as_slice())
            {
                break;
            }
            if native.get(cursor).copied().is_some_and(structural) {
                cursor += 1
            } else {
                return Err(invalid());
            }
        }
        let end = cursor + projected.len();
        if range.0 >= cursor && range.1 <= end {
            if target.is_some() {
                return Err(invalid());
            }
            let start = byte_at(&text, range.0 - cursor).ok_or_else(invalid)?;
            let end = byte_at(&text, range.1 - cursor).ok_or_else(invalid)?;
            target = Some((p.id, start..end));
        }
        cursor = end;
    }
    if native[cursor..].iter().any(|&c| !structural(c)) {
        return Err(invalid());
    }
    target.ok_or_else(invalid)
}
impl RichDocument {
    pub fn prepare_word_edit(
        &self,
        native: &[u16],
        range: (usize, usize),
        word: &str,
        replacement: &str,
    ) -> Result<PreparedWordEdit, Error> {
        if native.len() > 1_000_000 || replacement.len() > 32768 {
            return Err(Error::Limit);
        }
        if range.0 >= range.1
            || range.1 > native.len()
            || word.is_empty()
            || word
                .chars()
                .chain(replacement.chars())
                .any(|c| c.is_control() || matches!(c, '\u{FFFC}' | '\u{FDD0}'..='\u{FDEF}'))
        {
            return Err(Error::Invalid("invalid word correction".into()));
        }
        if String::from_utf16(&native[range.0..range.1])
            .ok()
            .as_deref()
            != Some(word)
        {
            return Err(Error::Invalid(
                "word correction splits or changes the original text".into(),
            ));
        }
        let (paragraph, range) = locate(self.state(), native, range)?;
        let source = self.state().clone();
        let mut after = source.clone();
        if cached_toc_paragraph(&after, paragraph)
            || after.fields.iter().any(|f| f.at.paragraph == paragraph)
            || after
                .revisions
                .iter()
                .any(|r| r.start.paragraph == paragraph || r.end.paragraph == paragraph)
        {
            return Err(Error::Unsupported(
                "word correction at fields, revisions or cached TOC entries".into(),
            ));
        }
        if after
            .footnotes
            .iter()
            .any(|n| n.at.paragraph == paragraph && range.contains(&n.at.offset))
        {
            return Err(Error::Unsupported(
                "word correction includes a note marker".into(),
            ));
        }
        let p = paragraph_mut(&mut after, paragraph)
            .ok_or_else(|| Error::Invalid("word paragraph missing".into()))?;
        let all: String = p.runs.iter().map(|r| r.text.as_str()).collect();
        if all.get(range.clone()) != Some(word) {
            return Err(Error::Invalid("word source mismatch".into()));
        }
        let mut ends = Vec::with_capacity(p.runs.len());
        let mut end = 0;
        for run in &p.runs {
            end += run.text.len();
            ends.push(end)
        }
        let first = ends.partition_point(|&end| end <= range.start);
        let last = ends.partition_point(|&end| end < range.end);
        let mut kept = vec![];
        if first == last {
            kept.extend(0..p.runs.len());
            let base = if first == 0 { 0 } else { ends[first - 1] };
            p.runs[first]
                .text
                .replace_range(range.start - base..range.end - base, replacement);
        } else {
            let mut out = vec![];
            let mut cursor = 0;
            for (i, run) in p.runs.iter().enumerate() {
                let end = ends[i];
                if i < first || i > last {
                    out.push(run.clone());
                    kept.push(i);
                } else if i == first {
                    let mut run = run.clone();
                    run.text.truncate(range.start - cursor);
                    run.text.push_str(replacement);
                    if !run.text.is_empty() {
                        out.push(run);
                        kept.push(i);
                    }
                } else if i == last {
                    let mut run = run.clone();
                    run.text = run.text[range.end - cursor..].into();
                    if !run.text.is_empty() {
                        out.push(run);
                        kept.push(i);
                    }
                }
                cursor = end;
            }
            p.runs = out;
        }
        let runs = kept
            .into_iter()
            .zip(p.runs.iter().map(|r| r.text.clone()))
            .collect();
        annotations::remap_edit(&mut after, paragraph, range.clone(), replacement.len());
        for p in [&mut after.selection.anchor, &mut after.selection.focus]
            .into_iter()
            .flatten()
        {
            if p.paragraph == paragraph {
                p.offset = if p.offset < range.start {
                    p.offset
                } else if p.offset >= range.end {
                    p.offset - (range.end - range.start) + replacement.len()
                } else {
                    range.start + replacement.len()
                };
            }
        }
        validate(&after)?;
        let preview = RichDocument {
            history: SnapshotHistory::new(after.clone(), 0),
        };
        let projection = preview.rich_edit_projection()?;
        Ok(PreparedWordEdit {
            source,
            source_id: self.history.current_id(),
            after,
            projection,
            paragraph,
            runs,
        })
    }
    pub fn commit_word_edit(&mut self, edit: PreparedWordEdit) -> Result<bool, Error> {
        if self.history.current_id() != edit.source_id || self.state() != &edit.source {
            return Err(Error::Invalid(
                "document changed before word correction".into(),
            ));
        }
        Ok(self.history.apply(edit.after))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (RichDocument, Vec<u16>) {
        let mut doc = RichDocument::decode(
            include_bytes!("../../fixtures/native-rich-edit-picture.doc42"),
            Format::Project,
        )
        .unwrap();
        doc.transact(|s| {
            let p = paragraph_mut(s, 1).unwrap();
            p.runs[0].text = "😀 speling world".into();
            p.runs[0].style.bold = true;
            p.runs[0].link = Some("https://example.com".into());
            paragraph_mut(s, 2).unwrap().runs[0].text = "hello speling".into();
            Ok(())
        })
        .unwrap();
        let native = "😀 speling world\rhello speling\u{7}Other\u{7}\r\u{FFFC}\rAfter\rTail\r"
            .encode_utf16()
            .collect();
        (doc, native)
    }
    #[test]
    fn precise_word_edits_preserve_every_unrelated_style_link_picture_and_history() {
        let (mut doc, mut native) = fixture();
        let original = doc.state().clone();
        let edit = doc
            .prepare_word_edit(&native, (3, 10), "speling", "spelling")
            .unwrap();
        assert_eq!(doc.state(), &original);
        assert!(!edit.projection().is_empty());
        assert_eq!(edit.run_texts(), &[(0, "😀 spelling world".into())]);
        doc.commit_word_edit(edit).unwrap();
        let mut expected = original.clone();
        paragraph_mut(&mut expected, 1).unwrap().runs[0].text = "😀 spelling world".into();
        assert_eq!(doc.state(), &expected);
        doc.undo();
        assert_eq!(doc.state(), &original);
        doc.redo();
        assert_eq!(doc.state(), &expected);
        native.splice(3..10, "spelling".encode_utf16());
        let start = String::from_utf16(&native).unwrap().find("hello ").unwrap();
        let start = String::from_utf16(&native).unwrap()[..start]
            .encode_utf16()
            .count()
            + 6;
        let edit = doc
            .prepare_word_edit(&native, (start, start + 7), "speling", "spelling")
            .unwrap();
        doc.commit_word_edit(edit).unwrap();
        paragraph_mut(&mut expected, 2).unwrap().runs[0].text = "hello spelling".into();
        assert_eq!(doc.state(), &expected);
        let reopened =
            RichDocument::decode(&doc.encode(Format::Project).unwrap(), Format::Project).unwrap();
        assert_eq!(reopened.state(), &expected);
    }
    #[test]
    fn stale_wrong_or_structural_ranges_refuse_without_mutating_model() {
        let (mut doc, native) = fixture();
        let before = doc.state().clone();
        assert!(doc.prepare_word_edit(&native, (1, 2), "x", "y").is_err());
        assert!(doc
            .prepare_word_edit(&native, (3, 10), "wrong", "y")
            .is_err());
        let mut mismatch = native.clone();
        mismatch[0] = b'X' as u16;
        assert!(doc
            .prepare_word_edit(&mismatch, (3, 10), "speling", "y")
            .is_err());
        assert!(doc
            .prepare_word_edit(&native, (3, 10), "speling", "\u{FFFC}")
            .is_err());
        assert_eq!(doc.state(), &before);
        let edit = doc
            .prepare_word_edit(&native, (3, 10), "speling", "spelling")
            .unwrap();
        doc.transact(|s| {
            s.sections[0].page.margin_left += 1;
            Ok(())
        })
        .unwrap();
        let after = doc.state().clone();
        assert!(doc.commit_word_edit(edit).is_err());
        assert_eq!(doc.state(), &after);
    }
}
