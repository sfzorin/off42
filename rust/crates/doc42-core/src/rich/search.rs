//! Body replacement with formatting and anchor preservation.
use super::*;
use suite42_core::text_search::{Budget, Replacement, Search};

fn search_error(error: suite42_core::text_search::Error) -> Error {
    match error {
        suite42_core::text_search::Error::Invalid(message) => Error::Invalid(message),
        suite42_core::text_search::Error::Limit(_) => Error::Limit,
    }
}

impl RichDocument {
    /// Replace literal occurrences across paragraphs as one transaction, retaining run formatting.
    pub fn replace_all_text(&mut self, query: &str, replacement: &str) -> Result<usize, Error> {
        if query.is_empty() {
            return Ok(0);
        }
        if query.contains(['\r', '\n']) || replacement.contains(['\r', '\n']) {
            if !self.can_edit_plain() {
                return Err(Error::Unsupported(
                    "replacement across rich paragraphs".into(),
                ));
            }
            let text = self.editor_text()?.replace("\r\n", "\n");
            let query = query.replace("\r\n", "\n");
            let replacement = replacement.replace("\r\n", "\n");
            let count = text.matches(&query).count();
            if count > 0 {
                self.set_plain_editor_text(&text.replace(&query, &replacement))?;
            }
            return Ok(count);
        }
        self.replace_paragraph_matches(|text, limit| {
            let ranges = text
                .match_indices(query)
                .take(limit)
                .map(|(start, _)| start..start + query.len())
                .collect::<Vec<_>>();
            if ranges.len().saturating_mul(replacement.len()) > MAX_BYTES {
                return Err(Error::Limit);
            }
            Ok(ranges
                .into_iter()
                .map(|range| Replacement {
                    range,
                    text: replacement.into(),
                })
                .collect())
        })
    }

    /// Replace nonempty matches. Rich edits stay inside body and table-cell paragraphs.
    pub fn replace_matches(&mut self, search: &Search, template: &str) -> Result<usize, Error> {
        let mut budget = Budget::default();
        let template = template.replace("\r\n", "\n");
        if self.can_edit_plain() {
            let text = self.editor_text()?.replace("\r\n", "\n");
            let edits = search
                .replacements(&text, &template, &mut budget)
                .map_err(search_error)?;
            if edits.is_empty() {
                return Ok(0);
            }
            let mut result = String::with_capacity(text.len());
            let mut cursor = 0;
            for edit in &edits {
                result.push_str(&text[cursor..edit.range.start]);
                result.push_str(&edit.text);
                cursor = edit.range.end;
            }
            result.push_str(&text[cursor..]);
            if result != text {
                self.set_plain_editor_text(&result)?;
            }
            return Ok(edits.len());
        }
        self.replace_paragraph_matches(|text, _| {
            let edits = search
                .replacements(text, &template, &mut budget)
                .map_err(search_error)?;
            if edits
                .iter()
                .any(|edit| edit.text.contains(['\r', '\n', '\0']))
            {
                return Err(Error::Unsupported(
                    "replacement across rich paragraphs".into(),
                ));
            }
            Ok(edits)
        })
    }

    fn replace_paragraph_matches(
        &mut self,
        mut match_paragraph: impl FnMut(&str, usize) -> Result<Vec<Replacement>, Error>,
    ) -> Result<usize, Error> {
        let mut count = 0;
        self.transact(|s| {
            let paragraphs = s.blocks.iter().flat_map(|block| match block {
                Block::Paragraph(p) => vec![p],
                Block::Table(table) => table
                    .rows
                    .iter()
                    .flatten()
                    .flat_map(|cell| &cell.paragraphs)
                    .collect(),
                _ => vec![],
            });
            let mut plans = vec![];
            let mut resulting_bytes = 0usize;
            let context_items = s
                .bookmarks
                .len()
                .saturating_add(s.comments.len())
                .saturating_add(s.footnotes.len())
                .saturating_add(s.fields.len())
                .saturating_add(s.revisions.len().saturating_mul(2))
                .saturating_add(
                    s.tables_of_contents
                        .iter()
                        .map(|toc| {
                            toc.entries.len().saturating_add(
                                toc.odt_cached
                                    .as_ref()
                                    .map_or(0, |cache| cache.title_paragraphs.len()),
                            )
                        })
                        .sum::<usize>(),
                );
            for paragraph in paragraphs {
                let text: String = paragraph.runs.iter().map(|run| run.text.as_str()).collect();
                let matches = match_paragraph(&text, 100_001 - count)?;
                count += matches.len();
                if count > 100_000 {
                    return Err(Error::Limit);
                }
                let (removed, inserted) = matches
                    .iter()
                    .try_fold((0usize, 0usize), |(old, new), edit| {
                        Some((
                            old.checked_add(edit.range.len())?,
                            new.checked_add(edit.text.len())?,
                        ))
                    })
                    .ok_or(Error::Limit)?;
                resulting_bytes = resulting_bytes
                    .checked_add(text.len() - removed)
                    .and_then(|n| n.checked_add(inserted))
                    .filter(|n| *n <= MAX_BYTES)
                    .ok_or(Error::Limit)?;
                if matches.is_empty() {
                    continue;
                }
                // The existing anchor remapper walks document-wide contexts.
                // Bound aggregate work before entering any matching/rewriting loop.
                if count.saturating_mul(context_items) > 10_000_000 {
                    return Err(Error::Limit);
                }
                if s.footnotes.iter().any(|note| {
                    note.at.paragraph == paragraph.id
                        && matches
                            .iter()
                            .any(|edit| edit.range.contains(&note.at.offset))
                }) {
                    return Err(Error::Unsupported(
                        "replacement includes a footnote or endnote marker".into(),
                    ));
                }
                if cached_toc_paragraph(s, paragraph.id) {
                    return Err(Error::Unsupported(
                        "update the imported TOC before editing its cached entries".into(),
                    ));
                }
                if s.revisions.iter().any(|revision| {
                    revision.start.paragraph == paragraph.id
                        || revision.end.paragraph == paragraph.id
                }) || s
                    .fields
                    .iter()
                    .any(|field| field.at.paragraph == paragraph.id)
                {
                    return Err(Error::Unsupported(
                        "replacement at revision or field anchors".into(),
                    ));
                }
                let mut ends = Vec::with_capacity(paragraph.runs.len());
                let mut end = 0;
                for run in &paragraph.runs {
                    end += run.text.len();
                    ends.push(end);
                }
                let mut output = vec![];
                let mut cursor = 0;
                let mut remaps = Vec::with_capacity(matches.len());
                for edit in matches {
                    let start = edit.range.start;
                    let inserted = edit.text.len();
                    append_indexed_run_slice(&paragraph.runs, &ends, cursor..start, &mut output);
                    if inserted != 0 {
                        let index = ends.partition_point(|end| *end <= start);
                        let mut run = paragraph
                            .runs
                            .get(index)
                            .ok_or_else(|| Error::Invalid("replace position".into()))?
                            .clone();
                        run.text = edit.text;
                        output.push(run);
                    }
                    cursor = edit.range.end;
                    remaps.push((edit.range, inserted));
                }
                append_indexed_run_slice(&paragraph.runs, &ends, cursor..text.len(), &mut output);
                plans.push((paragraph.id, output, remaps));
            }
            for (id, runs, matches) in plans {
                paragraph_mut(s, id)
                    .ok_or_else(|| Error::Invalid("replacement paragraph missing".into()))?
                    .runs = runs;
                // Original byte ranges remain valid when applied from right to left.
                for (range, length) in matches.into_iter().rev() {
                    annotations::remap_edit(s, id, range, length);
                }
            }
            if count > 0 {
                s.selection = Selection::default();
            }
            Ok(())
        })?;
        Ok(count)
    }
}
fn append_indexed_run_slice(
    runs: &[Run],
    ends: &[usize],
    range: std::ops::Range<usize>,
    out: &mut Vec<Run>,
) {
    if range.is_empty() {
        return;
    }
    let index = ends.partition_point(|end| *end <= range.start);
    for i in index..runs.len() {
        let start = if i == 0 { 0 } else { ends[i - 1] };
        let end = ends[i];
        if start >= range.end {
            break;
        }
        let left = range.start.max(start) - start;
        let right = range.end.min(end) - start;
        if left < right {
            let mut run = runs[i].clone();
            run.text = run.text[left..right].into();
            out.push(run);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn paragraph(id: u64, text: &str) -> Paragraph {
        Paragraph {
            id,
            style: Default::default(),
            runs: vec![Run {
                text: text.into(),
                style: Default::default(),
                link: None,
            }],
        }
    }
    fn text(state: &State, id: u64) -> String {
        let mut copy = state.clone();
        paragraph_mut(&mut copy, id)
            .unwrap()
            .runs
            .iter()
            .map(|run| run.text.as_str())
            .collect()
    }
    #[test]
    fn literal_replace_remaps_bookmarks_even_without_other_annotations() {
        let mut doc = RichDocument::decode(b"aa target tail", Format::Text).unwrap();
        doc.add_bookmark(
            "tail".into(),
            Position {
                paragraph: 1,
                offset: 10,
            },
            Position {
                paragraph: 1,
                offset: 14,
            },
        )
        .unwrap();
        let before = doc.snapshot();
        assert_eq!(doc.replace_all_text("target", "replacement").unwrap(), 1);
        let bookmark = &doc.state().bookmarks[0];
        assert_eq!((bookmark.start.offset, bookmark.end.offset), (15, 19));
        assert_eq!(
            &text(doc.state(), 1)[bookmark.start.offset..bookmark.end.offset],
            "tail"
        );
        assert!(doc.undo());
        assert_eq!(doc.state(), &before);
    }
    #[test]
    fn table_image_and_annotated_body_replace_preserve_destinations_history_and_office_output() {
        use image::ImageEncoder;
        let mut png = vec![];
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[20, 40, 60, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let mut first = paragraph(1, "aa target tail");
            first.runs = vec![
                Run {
                    text: "aa tar".into(),
                    style: TextStyle {
                        bold: true,
                        ..Default::default()
                    },
                    link: Some("https://example.invalid/".into()),
                },
                Run {
                    text: "get tail".into(),
                    style: Default::default(),
                    link: None,
                },
            ];
            s.blocks = vec![
                Block::Paragraph(first),
                Block::Image(Image {
                    id: 2,
                    mime: "image/png".into(),
                    data: png.clone(),
                    width_twips: 200,
                    height_twips: 200,
                    alt: "unmodified image".into(),
                }),
                Block::Table(Table {
                    id: 3,
                    rows: vec![vec![Cell {
                        paragraphs: vec![paragraph(4, "target cell")],
                        row_span: 1,
                        col_span: 1,
                        border: None,
                        fill: Some(0xeeeeee),
                    }]],
                    column_widths_twips: vec![3000],
                }),
                Block::Paragraph(paragraph(5, "after target end")),
            ];
            s.sections[0].header = vec![paragraph(100, "target header")];
            s.next_id = 101;
            Ok(())
        })
        .unwrap();
        doc.add_bookmark(
            "tail".into(),
            Position {
                paragraph: 1,
                offset: 10,
            },
            Position {
                paragraph: 1,
                offset: 14,
            },
        )
        .unwrap();
        doc.add_comment(
            Position {
                paragraph: 1,
                offset: 3,
            },
            Position {
                paragraph: 1,
                offset: 9,
            },
            "Reviewer".into(),
            "comment remains".into(),
        )
        .unwrap();
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 14,
            },
            "target note remains".into(),
            false,
        )
        .unwrap();
        doc.mark_saved();
        let before = doc.snapshot();
        let search = Search::new(
            "(TAR)(GET)",
            suite42_core::text_search::Options {
                regex: true,
                whole_words: true,
                ..Default::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(doc.replace_matches(&search, "replacement").unwrap(), 3);
        assert_eq!(text(doc.state(), 1), "aa replacement tail");
        assert_eq!(text(doc.state(), 4), "replacement cell");
        assert_eq!(text(doc.state(), 5), "after replacement end");
        assert_eq!(doc.state().blocks[1], before.blocks[1]);
        assert_eq!(doc.state().sections, before.sections);
        assert_eq!(doc.state().footnotes[0].at.offset, 19);
        assert_eq!(
            (
                doc.state().comments[0].start.offset,
                doc.state().comments[0].end.offset
            ),
            (3, 14)
        );
        assert_eq!(
            (
                doc.state().bookmarks[0].start.offset,
                doc.state().bookmarks[0].end.offset
            ),
            (15, 19)
        );
        let Block::Paragraph(first) = &doc.state().blocks[0] else {
            panic!()
        };
        assert_eq!(first.runs[1].text, "replacement");
        assert!(first.runs[1].style.bold);
        assert_eq!(
            first.runs[1].link.as_deref(),
            Some("https://example.invalid/")
        );
        assert_eq!(
            doc.state().footnotes[0].paragraphs,
            before.footnotes[0].paragraphs
        );
        let after = doc.snapshot();
        assert!(doc.undo());
        assert_eq!(doc.state(), &before);
        assert!(!doc.is_dirty());
        assert!(doc.redo());
        assert_eq!(doc.state(), &after);
        for format in [Format::Project, Format::Docx, Format::Odt] {
            let bytes = doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            let bookmark = &reopened.state().bookmarks[0];
            assert!(
                paragraph_mut(&mut reopened.snapshot(), bookmark.start.paragraph).is_some(),
                "{format:?}: bookmark points outside the body: {bookmark:?}",
            );
            let body = text(reopened.state(), bookmark.start.paragraph);
            assert_eq!(&body[bookmark.start.offset..bookmark.end.offset], "tail");
            assert_eq!(reopened.state().comments.len(), 1);
            assert_eq!(reopened.state().footnotes.len(), 1);
            assert_eq!(
                reopened
                    .state()
                    .blocks
                    .iter()
                    .filter(|b| matches!(b, Block::Image(_)))
                    .count(),
                1
            );
            assert!(reopened.state().blocks.iter().any(|b|matches!(b,Block::Table(t)if t.rows[0][0].paragraphs[0].runs.iter().map(|r|r.text.as_str()).collect::<String>()=="replacement cell")));
        }
    }
    #[test]
    fn repeated_matches_remap_from_the_right_and_limits_are_atomic() {
        let mut doc = RichDocument::decode(b"a a tail", Format::Text).unwrap();
        doc.add_bookmark(
            "tail".into(),
            Position {
                paragraph: 1,
                offset: 4,
            },
            Position {
                paragraph: 1,
                offset: 8,
            },
        )
        .unwrap();
        // Match only isolated source occurrences; replacement's own characters aren't revisited.
        assert_eq!(doc.replace_all_text("a ", "long ").unwrap(), 2);
        assert_eq!(text(doc.state(), 1), "long long tail");
        assert_eq!(
            (
                doc.state().bookmarks[0].start.offset,
                doc.state().bookmarks[0].end.offset
            ),
            (10, 14)
        );
        let mut large = RichDocument::decode("x".repeat(100001).as_bytes(), Format::Text).unwrap();
        let before = large.snapshot();
        assert!(matches!(
            large.replace_all_text("x", "y"),
            Err(Error::Limit)
        ));
        assert_eq!(large.state(), &before);
        assert!(!large.can_undo());
        let mut expansion = RichDocument::decode(b"xxx", Format::Text).unwrap();
        let before = expansion.snapshot();
        assert!(matches!(
            expansion.replace_all_text("x", &"y".repeat(6 * 1024 * 1024)),
            Err(Error::Limit)
        ));
        assert_eq!(expansion.state(), &before);
    }
    #[test]
    fn a_match_covering_an_invisible_note_anchor_is_refused_atomically() {
        let mut doc = RichDocument::decode(b"before target after", Format::Text).unwrap();
        doc.add_note(
            Position {
                paragraph: 1,
                offset: 9,
            },
            "keep this note".into(),
            false,
        )
        .unwrap();
        doc.mark_saved();
        let before = doc.snapshot();
        assert!(matches!(
            doc.replace_all_text("target", "changed"),
            Err(Error::Unsupported(_))
        ));
        assert_eq!(doc.state(), &before);
        assert!(!doc.is_dirty());
    }
    #[test]
    fn excessive_annotation_remap_work_is_refused_before_mutation() {
        let mut doc = RichDocument::decode("x".repeat(10001).as_bytes(), Format::Text).unwrap();
        doc.transact(|state| {
            for index in 0..1001 {
                let id = state.next_id;
                state.next_id += 1;
                state.bookmarks.push(Bookmark {
                    id,
                    name: format!("mark{index}"),
                    start: Position {
                        paragraph: 1,
                        offset: 0,
                    },
                    end: Position {
                        paragraph: 1,
                        offset: 0,
                    },
                });
            }
            Ok(())
        })
        .unwrap();
        doc.mark_saved();
        let before = doc.snapshot();
        assert!(matches!(doc.replace_all_text("x", "y"), Err(Error::Limit)));
        assert_eq!(doc.state(), &before);
        assert!(!doc.is_dirty());
        assert!(doc.undo());
        assert!(doc.state().bookmarks.is_empty());
        assert!(doc.redo());
        assert_eq!(doc.state(), &before);
    }
}
