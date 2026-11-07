//! Read-only body statistics, including table paragraphs without projecting away images.
use super::*;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TextStatistics {
    pub words: usize,
    pub characters: usize,
    pub characters_without_spaces: usize,
    pub paragraphs: usize,
}
impl TextStatistics {
    fn add_paragraph(&mut self, paragraph: &Paragraph) {
        let text = paragraph
            .runs
            .iter()
            .map(|r| r.text.as_str())
            .collect::<String>()
            .replace(['\u{fffc}', '\u{000c}'], "");
        self.words += text.unicode_words().count();
        for grapheme in text
            .graphemes(true)
            .filter(|s| !matches!(*s, "\r" | "\n" | "\r\n" | "\u{2029}"))
        {
            self.characters += 1;
            if !grapheme.chars().all(char::is_whitespace) {
                self.characters_without_spaces += 1;
            }
        }
        self.paragraphs += text
            .split(['\r', '\n', '\u{2028}', '\u{2029}'])
            .filter(|s| !s.trim().is_empty())
            .count();
    }
}
impl RichDocument {
    /// Unicode word boundaries/graphemes; excludes image alternate text, notes, comments and page decorations.
    pub fn statistics(&self) -> TextStatistics {
        let mut stats = TextStatistics::default();
        for block in &self.state().blocks {
            match block {
                Block::Paragraph(p) => stats.add_paragraph(p),
                Block::Table(t) => {
                    for p in t.rows.iter().flatten().flat_map(|c| &c.paragraphs) {
                        stats.add_paragraph(p);
                    }
                }
                _ => {}
            }
        }
        stats
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn whole_body_counts_table_paragraphs_and_joined_style_runs() {
        let doc=RichDocument::decode(br"{\rtf1\ansi Before TARGET\par\trowd\cellx2000\cellx4000\pard\intbl Table TARGET\cell Other cell\cell\row\pard After TARGET\par}",Format::Rtf).unwrap();
        assert!(doc.editor_text().is_err());
        assert_eq!(doc.statistics().words, 8);
        assert_eq!(doc.statistics().paragraphs, 4);
        let mut doc = RichDocument::new();
        doc.transact(|s| {
            let Block::Paragraph(p) = &mut s.blocks[0] else {
                panic!()
            };
            p.runs = vec![
                Run {
                    text: "ca".into(),
                    style: Default::default(),
                    link: None,
                },
                Run {
                    text: "fé e\u{301} 😀\u{fffc}\u{000c}".into(),
                    style: TextStyle {
                        bold: true,
                        ..Default::default()
                    },
                    link: None,
                },
            ];
            Ok(())
        })
        .unwrap();
        assert_eq!(
            doc.statistics(),
            TextStatistics {
                words: 2,
                characters: 8,
                characters_without_spaces: 6,
                paragraphs: 1
            }
        );
    }
    #[test]
    fn count_changes_with_complete_history_and_office_roundtrips() {
        let mut doc=RichDocument::decode(br"{\rtf1\ansi Before TARGET\par\trowd\cellx2000\cellx4000\pard\intbl Table TARGET\cell Other cell\cell\row\pard After TARGET\par}",Format::Rtf).unwrap();
        let original = doc.statistics();
        assert_eq!(doc.replace_all_text("TARGET", "two words").unwrap(), 3);
        let changed = doc.statistics();
        assert_eq!(changed.words, original.words + 3);
        assert_eq!(changed.paragraphs, original.paragraphs);
        doc.undo();
        assert_eq!(doc.statistics(), original);
        doc.redo();
        assert_eq!(doc.statistics(), changed);
        for format in [Format::Project, Format::Docx, Format::Odt] {
            let bytes = doc.encode(format).unwrap();
            let reopened = RichDocument::decode(&bytes, format).unwrap();
            assert_eq!(reopened.statistics(), changed);
        }
    }
}
