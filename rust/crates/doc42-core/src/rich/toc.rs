//! Transactional table of contents backed by document paragraph identities.
use super::*;

impl RichDocument {
    /// Insert a generated TOC immediately before the selected body paragraph.
    pub fn insert_toc(&mut self, before: u64) -> Result<bool, Error> {
        self.transact(|s| {
            let index = s
                .blocks
                .iter()
                .position(|b| matches!(b, Block::Paragraph(p) if p.id == before))
                .ok_or_else(|| Error::Invalid("TOC insertion paragraph".into()))?;
            let headings: Vec<_> = s
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Paragraph(p)
                        if p.style.outline_level.is_some_and(|l| l <= 8)
                            && !s
                                .tables_of_contents
                                .iter()
                                .filter_map(|t| t.odt_cached.as_ref())
                                .any(|c| c.title_paragraphs.contains(&p.id)) =>
                    {
                        Some(p.clone())
                    }
                    _ => None,
                })
                .collect();
            if headings.is_empty() {
                return Err(Error::Invalid("document has no headings".into()));
            }
            let id = s.next_id;
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            let mut entries = Vec::new();
            for (i, heading) in headings.iter().enumerate() {
                let pid = s.next_id;
                s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                let mut style = ParagraphStyle::default();
                style.left_twips = i32::from(heading.style.outline_level.unwrap_or(0)) * 360;
                let text = heading
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>();
                s.blocks.insert(
                    index + i,
                    Block::Paragraph(Paragraph {
                        id: pid,
                        style,
                        runs: vec![Run {
                            text,
                            style: TextStyle::default(),
                            link: Some(format!("#DOC42_H{}", heading.id)),
                        }],
                    }),
                );
                entries.push(TocEntry {
                    paragraph: pid,
                    heading: heading.id,
                });
            }
            s.tables_of_contents.push(TableOfContents {
                id,
                entries,
                min_level: 0,
                max_level: 8,
                docx_cached: None,
                odt_cached: None,
            });
            Ok(())
        })
    }
}

impl RichDocument {
    /// Resolve a displayed TOC paragraph to its heading target.
    pub fn toc_target(&self, paragraph: u64) -> Option<Position> {
        self.state()
            .tables_of_contents
            .iter()
            .flat_map(|toc| &toc.entries)
            .find(|entry| entry.paragraph == paragraph)
            .map(|entry| Position {
                paragraph: entry.heading,
                offset: 0,
            })
    }
    /// Remove the generated paragraphs and their index in one undoable operation.
    pub fn remove_toc(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| {
            let index = s.tables_of_contents.iter().position(|t| t.id == id)
                .ok_or_else(|| Error::Invalid("table of contents".into()))?;
            let entries = &s.tables_of_contents[index].entries;
            let titles = s.tables_of_contents[index].odt_cached.as_ref().map(|c| c.title_paragraphs.clone()).unwrap_or_default();
            if entries.iter().any(|e| s.comments.iter().any(|c| c.start.paragraph == e.paragraph || c.end.paragraph == e.paragraph)
                || s.footnotes.iter().any(|n| n.at.paragraph == e.paragraph)
                || s.fields.iter().any(|f| f.at.paragraph == e.paragraph)
                || s.bookmarks.iter().any(|b| b.start.paragraph == e.paragraph || b.end.paragraph == e.paragraph)
                || s.revisions.iter().any(|r| r.start.paragraph == e.paragraph || r.end.paragraph == e.paragraph)) {
                return Err(Error::Unsupported("annotations on generated TOC entries".into()));
            }
            if titles.iter().any(|id| s.comments.iter().any(|c| c.start.paragraph == *id || c.end.paragraph == *id) || s.footnotes.iter().any(|n| n.at.paragraph == *id) || s.bookmarks.iter().any(|b| b.start.paragraph == *id || b.end.paragraph == *id) || s.fields.iter().any(|f| f.at.paragraph == *id) || s.revisions.iter().any(|r| r.start.paragraph == *id || r.end.paragraph == *id)) { return Err(Error::Unsupported("annotations on TOC title".into())); }
            s.blocks.retain(|b| !matches!(b, Block::Paragraph(p) if entries.iter().any(|e| e.paragraph == p.id) || titles.contains(&p.id)));
            s.tables_of_contents.remove(index);
            s.selection = Selection::default();
            Ok(())
        })
    }
    /// Refresh labels and membership using current heading styles in document order.
    pub fn update_toc(&mut self, id: u64) -> Result<bool, Error> {
        self.transact(|s| {
            let toc = s
                .tables_of_contents
                .iter()
                .position(|t| t.id == id)
                .ok_or_else(|| Error::Invalid("table of contents".into()))?;
            let old = s.tables_of_contents[toc].entries.clone();
            let titles = s.tables_of_contents[toc]
                .odt_cached
                .as_ref()
                .map(|c| c.title_paragraphs.clone())
                .unwrap_or_default();
            let index = s
                .blocks
                .iter()
                .position(
                    |b| matches!(b, Block::Paragraph(p) if old.iter().any(|e| e.paragraph == p.id)),
                )
                .ok_or_else(|| Error::Invalid("table of contents entries".into()))?;
            if old.iter().any(|e| {
                s.comments
                    .iter()
                    .any(|c| c.start.paragraph == e.paragraph || c.end.paragraph == e.paragraph)
                    || s.footnotes.iter().any(|n| n.at.paragraph == e.paragraph)
                    || s.fields.iter().any(|f| f.at.paragraph == e.paragraph)
                    || s.bookmarks
                        .iter()
                        .any(|b| b.start.paragraph == e.paragraph || b.end.paragraph == e.paragraph)
                    || s.revisions
                        .iter()
                        .any(|r| r.start.paragraph == e.paragraph || r.end.paragraph == e.paragraph)
            }) {
                return Err(Error::Unsupported(
                    "annotations on generated TOC entries".into(),
                ));
            }
            let headings: Vec<_> = s
                .blocks
                .iter()
                .filter_map(|b| match b {
                    Block::Paragraph(p)
                        if !titles.contains(&p.id)
                            && !s
                                .tables_of_contents
                                .iter()
                                .filter_map(|t| t.odt_cached.as_ref())
                                .any(|c| c.title_paragraphs.contains(&p.id))
                            && p.style.outline_level.is_some_and(|l| {
                                l >= s.tables_of_contents[toc].min_level
                                    && l <= s.tables_of_contents[toc].max_level
                            }) =>
                    {
                        Some(p.clone())
                    }
                    _ => None,
                })
                .collect();
            s.blocks.retain(
                |b| !matches!(b, Block::Paragraph(p) if old.iter().any(|e| e.paragraph == p.id)),
            );
            let mut entries = Vec::new();
            for (i, heading) in headings.iter().enumerate() {
                let pid = if let Some(e) = old.iter().find(|e| e.heading == heading.id) {
                    e.paragraph
                } else {
                    let id = s.next_id;
                    s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                    id
                };
                let mut style = ParagraphStyle::default();
                style.left_twips = i32::from(heading.style.outline_level.unwrap_or(0)) * 360;
                let text = heading
                    .runs
                    .iter()
                    .map(|r| r.text.as_str())
                    .collect::<String>();
                s.blocks.insert(
                    index + i,
                    Block::Paragraph(Paragraph {
                        id: pid,
                        style,
                        runs: vec![Run {
                            text,
                            style: TextStyle::default(),
                            link: Some(format!("#DOC42_H{}", heading.id)),
                        }],
                    }),
                );
                entries.push(TocEntry {
                    paragraph: pid,
                    heading: heading.id,
                });
            }
            s.tables_of_contents[toc].entries = entries;
            s.tables_of_contents[toc].docx_cached = None;
            if let Some(cache) = &mut s.tables_of_contents[toc].odt_cached {
                cache.needs_refresh = true;
                cache.has_pages = false;
            }
            s.selection = Selection::default();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generated_toc_tracks_heading_ids_through_project_history() {
        let mut doc = RichDocument::new();
        doc.set_plain_editor_text("Title\nBody").unwrap();
        doc.transact(|s| {
            let Block::Paragraph(p) = &mut s.blocks[0] else {
                panic!()
            };
            p.style.outline_level = Some(0);
            Ok(())
        })
        .unwrap();
        let heading = match &doc.state().blocks[0] {
            Block::Paragraph(p) => p.id,
            _ => panic!(),
        };
        let before = match &doc.state().blocks[1] {
            Block::Paragraph(p) => p.id,
            _ => panic!(),
        };
        doc.insert_toc(before).unwrap();
        let toc = doc.state().tables_of_contents[0].clone();
        assert_eq!(toc.entries[0].heading, heading);
        let saved =
            RichDocument::decode(&doc.encode(Format::Project).unwrap(), Format::Project).unwrap();
        assert_eq!(saved.state().tables_of_contents[0], toc);
        let odt = RichDocument::decode(&doc.encode(Format::Odt).unwrap(), Format::Odt).unwrap();
        assert_eq!(odt.state().tables_of_contents.len(), 1);
        let entry = &odt.state().tables_of_contents[0].entries[0];
        assert_eq!(
            odt.toc_target(entry.paragraph).unwrap().paragraph,
            entry.heading
        );
        assert!(odt.encode(Format::Odt).is_ok());
        let docx = RichDocument::decode(&doc.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        assert_eq!(docx.state().tables_of_contents.len(), 1);
        let entry = &docx.state().tables_of_contents[0].entries[0];
        assert_eq!(
            docx.toc_target(entry.paragraph).unwrap().paragraph,
            entry.heading
        );
        assert!(docx.encode(Format::Docx).is_ok());
        assert!(doc.undo());
        assert!(doc.state().tables_of_contents.is_empty());
        assert!(doc.redo());
        assert_eq!(doc.state().tables_of_contents[0], toc);
        let rtf = String::from_utf8(doc.rich_edit_projection().unwrap()).unwrap();
        let edited = rtf.replacen("Title", "Renamed Title", 1);
        doc.apply_rich_edit_projection(edited.as_bytes(), (0, 0))
            .unwrap();
        let entry = &doc.state().tables_of_contents[0].entries[0];
        assert_eq!(
            doc.toc_target(entry.paragraph).unwrap().paragraph,
            entry.heading
        );
        let toc_id = doc.state().tables_of_contents[0].id;
        let rtf = String::from_utf8(doc.rich_edit_projection().unwrap()).unwrap();
        let edited = rtf.replacen("Renamed Title", "prefix\\par Renamed Title", 1);
        doc.apply_rich_edit_projection(edited.as_bytes(), (0, 0))
            .unwrap();
        doc.update_toc(toc_id).unwrap();
        let labels = doc.state().tables_of_contents[0]
            .entries
            .iter()
            .map(|entry| {
                doc.state()
                    .blocks
                    .iter()
                    .find_map(|b| match b {
                        Block::Paragraph(p) if p.id == entry.paragraph => {
                            Some(p.runs[0].text.as_str())
                        }
                        _ => None,
                    })
                    .unwrap()
            })
            .collect::<Vec<_>>();
        assert_eq!(labels, ["prefix", "Renamed Title"]);
        doc.remove_toc(toc_id).unwrap();
        assert!(doc.state().tables_of_contents.is_empty());
        assert!(doc.undo());
        assert_eq!(doc.state().tables_of_contents.len(), 1);
    }
}
