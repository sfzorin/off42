use crate::Ppt;
use doc42_core::rich::{
    Block, Format, Paragraph, ParagraphStyle, RichDocument, Run, State, TextStyle,
};
use ppt42_core::{ObjectKind, OutlineRole, Slide};
use std::fmt::Write;

#[derive(Clone, Copy)]
pub(super) enum Output {
    Rtf,
    Markdown,
}

impl Output {
    pub(super) const CHOICES: [&'static str; 2] = ["Rich Text (RTF)", "Markdown"];

    pub(super) fn from_choice(values: &[String]) -> Result<Self, String> {
        match values {
            [value] if value == Self::CHOICES[0] => Ok(Self::Rtf),
            [value] if value == Self::CHOICES[1] => Ok(Self::Markdown),
            _ => Err("Choose a supported outline format".into()),
        }
    }

    pub(super) fn extension(self) -> &'static str {
        match self {
            Self::Rtf => "rtf",
            Self::Markdown => "md",
        }
    }

    pub(super) fn encode(self, app: &Ppt) -> Result<Vec<u8>, String> {
        let mut markdown = String::new();
        let mut blocks = Vec::new();
        for (index, slide) in app.deck.slides().iter().enumerate() {
            let (title, paragraphs) = slide_text(app, slide);
            let title = format!("{}. {title}", index + 1);
            match self {
                Self::Rtf => {
                    if blocks.len() + paragraphs.len() >= 100_000 {
                        return Err("Outline exceeds the rich text paragraph limit".into());
                    }
                    push_paragraph(&mut blocks, title, true);
                    for paragraph in paragraphs {
                        push_paragraph(&mut blocks, format!("•\t{paragraph}"), false);
                    }
                }
                Self::Markdown => {
                    writeln!(markdown, "## {}\n", markdown_text(&title)).unwrap();
                    for paragraph in paragraphs {
                        writeln!(markdown, "- {}", markdown_text(&paragraph)).unwrap();
                    }
                    if !slide.notes.is_empty() {
                        markdown.push('\n');
                        for line in slide.notes.split('\n') {
                            writeln!(markdown, "> {}", markdown_text(line)).unwrap();
                        }
                    }
                    markdown.push('\n');
                    if markdown.len() > 64 * 1024 * 1024 {
                        return Err("Outline exceeds the Markdown size limit".into());
                    }
                }
            }
        }
        match self {
            Self::Markdown => Ok(markdown.into_bytes()),
            Self::Rtf => {
                let state = State {
                    next_id: blocks.len() as u64 + 1,
                    blocks,
                    ..State::default()
                };
                RichDocument::encode_snapshot(state, Format::Rtf).map_err(|error| error.to_string())
            }
        }
    }
}

fn slide_text(app: &Ppt, slide: &Slide) -> (String, Vec<String>) {
    let imported = app.pptx_source.is_some() || app.odp_source.is_some();
    let role = |id| {
        let slide = slide.source_identity();
        app.pptx_source
            .as_ref()
            .and_then(|source| source.outline_role(slide, id))
            .or_else(|| {
                app.odp_source
                    .as_ref()
                    .and_then(|source| source.outline_role(slide, id))
            })
    };
    let is_text = |kind: &ObjectKind| !matches!(kind, ObjectKind::Picture | ObjectKind::Table);
    let title_object = slide
        .objects
        .iter()
        .find(|object| role(object.id) == Some(OutlineRole::Title));
    let title_object = title_object.or_else(|| {
        if !imported || !slide.title.trim().is_empty() {
            return None;
        }
        slide
            .objects
            .iter()
            .filter(|object| {
                is_text(&object.kind) && role(object.id).is_none() && !object.text.trim().is_empty()
            })
            .min_by(|a, b| a.y.total_cmp(&b.y).then(a.x.total_cmp(&b.x)))
    });
    let title = clean_inline(title_object.map_or(&slide.title, |object| &object.text));
    let title = if title.trim().is_empty() {
        win42::localization::text("Untitled")
    } else {
        title
    };
    let mut paragraphs = Vec::new();
    for text in
        std::iter::once(slide.body.as_str()).chain(slide.objects.iter().filter_map(|object| {
            (is_text(&object.kind)
                && title_object.is_none_or(|title| title.id != object.id)
                && !matches!(
                    role(object.id),
                    Some(OutlineRole::Title | OutlineRole::Footer)
                ))
            .then_some(object.text.as_str())
        }))
    {
        for line in text.lines() {
            if !line.trim().is_empty() {
                paragraphs.push(clean_inline(line.trim()));
            }
        }
    }
    (title, paragraphs)
}

fn clean_inline(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn markdown_text(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(
            ch,
            '\\' | '*' | '_' | '~' | '`' | '[' | ']' | '<' | '>' | '&' | '#' | '|'
        ) {
            escaped.push('\\');
        }
        escaped.push(ch);
    }
    escaped
}

fn push_paragraph(blocks: &mut Vec<Block>, text: String, heading: bool) {
    let id = blocks.len() as u64 + 1;
    blocks.push(Block::Paragraph(Paragraph {
        id,
        style: ParagraphStyle {
            before_twips: if heading && id > 1 { 280 } else { 0 },
            after_twips: if heading { 80 } else { 0 },
            left_twips: if heading { 0 } else { 600 },
            first_line_twips: if heading { 0 } else { -240 },
            tabs_twips: if heading { Vec::new() } else { vec![600] },
            outline_level: heading.then_some(0),
            ..ParagraphStyle::default()
        },
        runs: vec![Run {
            text,
            style: TextStyle {
                bold: heading,
                size_half_points: if heading { 36 } else { 26 },
                ..TextStyle::default()
            },
            link: None,
        }],
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ppt42_core::Presentation;

    #[test]
    fn outline_formats_keep_slide_order_unicode_and_document_state() {
        let mut deck = Presentation::new();
        deck.edit("Питание & I/O".into(), "Check [SDA]\nSee <ref>".into())
            .unwrap();
        deck.set_notes("Private *cue*\nSecond line").unwrap();
        deck.add().unwrap();
        deck.edit("Hidden slide".into(), "Second bullet".into()).unwrap();
        deck.set_hidden(true).unwrap();
        let app = Ppt::new(deck);
        let before = app.deck.save_snapshot().unwrap().0;
        let md = String::from_utf8(Output::Markdown.encode(&app).unwrap()).unwrap();
        assert!(md.starts_with("## 1. Питание \\& I/O\n\n- Check \\[SDA\\]\n- See \\<ref\\>\n"));
        assert!(md.contains(
            "> Private \\*cue\\*\n> Second line\n\n## 2. Hidden slide\n\n- Second bullet"
        ));
        let rtf = Output::Rtf.encode(&app).unwrap();
        let document = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        let paragraphs = document
            .state()
            .blocks
            .iter()
            .map(|block| {
                let Block::Paragraph(paragraph) = block else {
                    panic!("expected outline paragraph")
                };
                paragraph
                    .runs
                    .iter()
                    .map(|run| run.text.as_str())
                    .collect::<String>()
            })
            .collect::<Vec<_>>();
        assert_eq!(
            paragraphs,
            [
                "1. Питание & I/O",
                "•\tCheck [SDA]",
                "•\tSee <ref>",
                "2. Hidden slide",
                "•\tSecond bullet"
            ]
        );
        assert_eq!(app.deck.save_snapshot().unwrap().0, before);
    }
}
