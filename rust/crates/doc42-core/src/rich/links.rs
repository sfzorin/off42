//! Links and named text bookmarks share the document's transaction history.
use super::*;
pub(super) fn apply_link(
    p: &mut Paragraph,
    range: std::ops::Range<usize>,
    target: Option<String>,
) -> Result<(), Error> {
    let text: String = p.runs.iter().map(|r| r.text.as_str()).collect();
    if range.is_empty()
        || range.start > range.end
        || range.end > text.len()
        || !text.is_char_boundary(range.start)
        || !text.is_char_boundary(range.end)
    {
        return Err(Error::Invalid("link selection".into()));
    }
    if target
        .as_ref()
        .is_some_and(|t| t.is_empty() || t.chars().any(char::is_control))
    {
        return Err(Error::Invalid("link target".into()));
    }
    let mut runs = Vec::new();
    let mut pos = 0;
    for run in &p.runs {
        let end = pos + run.text.len();
        let left = range.start.saturating_sub(pos).min(run.text.len());
        let right = range.end.saturating_sub(pos).min(run.text.len());
        if left > 0 {
            let mut part = run.clone();
            part.text = run.text[..left].into();
            runs.push(part);
        }
        if right > left {
            let mut part = run.clone();
            part.text = run.text[left..right].into();
            part.link = target.clone();
            runs.push(part);
        }
        if right < run.text.len() {
            let mut part = run.clone();
            part.text = run.text[right..].into();
            runs.push(part);
        }
        pos = end;
    }
    p.runs = runs;
    Ok(())
}
impl RichDocument {
    pub fn set_link(
        &mut self,
        paragraph: u64,
        range: std::ops::Range<usize>,
        target: Option<String>,
        selection: Selection,
    ) -> Result<bool, Error> {
        self.transact(|s| {
            if cached_toc_paragraph(s, paragraph) {
                return Err(Error::Unsupported(
                    "update the imported TOC before editing its cached entries".into(),
                ));
            }
            if s.tables_of_contents
                .iter()
                .flat_map(|toc| &toc.entries)
                .any(|entry| entry.paragraph == paragraph)
            {
                return Err(Error::Unsupported(
                    "edit generated table-of-contents links by updating the TOC".into(),
                ));
            }
            let p = paragraph_mut(s, paragraph)
                .ok_or_else(|| Error::Invalid("link paragraph".into()))?;
            apply_link(p, range, target)?;
            s.selection = selection;
            Ok(())
        })
    }
    pub fn add_bookmark(
        &mut self,
        name: String,
        start: Position,
        end: Position,
    ) -> Result<bool, Error> {
        self.transact(|s| {
            let id = s.next_id;
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            s.bookmarks.push(Bookmark {
                id,
                name,
                start,
                end,
            });
            Ok(())
        })
    }
    pub fn remove_bookmark(&mut self, name: &str) -> Result<bool, Error> {
        self.transact(|s| {
            let escaped = name
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
                .replace('\"', "&quot;")
                .replace('\'', "&apos;");
            if s.tables_of_contents.iter().any(|toc| {
                toc.docx_cached
                    .as_ref()
                    .is_some_and(|p| p.iter().any(|xml| xml.contains(&escaped)))
                    || toc.odt_cached.as_ref().is_some_and(|cache| {
                        !cache.needs_refresh && cache.index_xml.contains(&escaped)
                    })
            }) {
                return Err(Error::Unsupported(
                    "update the imported TOC before deleting its bookmark target".into(),
                ));
            }
            let i = s
                .bookmarks
                .iter()
                .position(|b| b.name == name)
                .ok_or_else(|| Error::Invalid("bookmark not found".into()))?;
            s.bookmarks.remove(i);
            Ok(())
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bookmarks_and_links_survive_odt_and_projection() {
        let mut d = RichDocument::new();
        d.set_plain_editor_text("Hello target").unwrap();
        let start = Position {
            paragraph: 1,
            offset: 6,
        };
        let end = Position {
            paragraph: 1,
            offset: 12,
        };
        d.add_bookmark("target".into(), start.clone(), end.clone())
            .unwrap();
        d.set_link(1, 0..5, Some("#target".into()), Selection::default())
            .unwrap();
        let reopened = RichDocument::decode(&d.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        assert_eq!(reopened.state().bookmarks.len(), 1);
        assert_eq!(reopened.state().bookmarks[0].name, "target");
        assert_eq!(
            (
                reopened.state().bookmarks[0].start.offset,
                reopened.state().bookmarks[0].end.offset
            ),
            (6, 12)
        );
        let Block::Paragraph(p) = &reopened.state().blocks[0] else {
            panic!()
        };
        assert!(p.runs.iter().any(|r| r.link.as_deref() == Some("#target")));
        let word = RichDocument::decode(&d.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        assert_eq!(word.state().bookmarks.len(), 1);
        assert_eq!(
            (
                word.state().bookmarks[0].start.offset,
                word.state().bookmarks[0].end.offset
            ),
            (6, 12)
        );
        let Block::Paragraph(p) = &word.state().blocks[0] else {
            panic!()
        };
        assert!(p.runs.iter().any(|r| r.link.as_deref() == Some("#target")));
        assert!(word.encode(Format::Docx).is_ok());
        d.set_link(
            1,
            6..12,
            Some("https://example.com/path".into()),
            Selection::default(),
        )
        .unwrap();
        let word = RichDocument::decode(&d.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        let Block::Paragraph(p) = &word.state().blocks[0] else {
            panic!()
        };
        assert!(p
            .runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://example.com/path")));
        let rtf = d.rich_edit_projection().unwrap();
        d.apply_rich_edit_projection(&rtf, (0, 0)).unwrap();
        assert_eq!(d.state().bookmarks[0].start, start);
        assert_eq!(d.state().bookmarks[0].end, end);
        let rtf = String::from_utf8(d.rich_edit_projection().unwrap())
            .unwrap()
            .replacen("Hello", "XHello", 1);
        d.apply_rich_edit_projection(rtf.as_bytes(), (1, 1))
            .unwrap();
        assert_eq!(
            (
                d.state().bookmarks[0].start.offset,
                d.state().bookmarks[0].end.offset
            ),
            (7, 13)
        );
        assert!(d.undo());
        assert_eq!(
            (
                d.state().bookmarks[0].start.offset,
                d.state().bookmarks[0].end.offset
            ),
            (6, 12)
        );
        assert!(d.redo());
        assert_eq!(
            (
                d.state().bookmarks[0].start.offset,
                d.state().bookmarks[0].end.offset
            ),
            (7, 13)
        );
    }
}
