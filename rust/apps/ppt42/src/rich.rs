//! RichEdit projection for a presentation object's authoritative paragraph model.
use doc42_core::rich::{
    Alignment, Block, ListMarker, Paragraph, ParagraphStyle, RichDocument, Run, Selection,
    TextStyle as DocStyle,
};
use ppt42_core::{RichParagraph, RichRun, SlideObject, TextAlign, TextStyle};
fn doc_style(s: &TextStyle) -> DocStyle {
    DocStyle {
        font: s.family.clone(),
        size_half_points: (s.points * 2.0).round().clamp(2.0, 1024.0) as u16,
        bold: s.bold,
        italic: s.italic,
        underline: s.underline,
        color: Some(((s.color[0] as u32) << 16) | ((s.color[1] as u32) << 8) | s.color[2] as u32),
        ..Default::default()
    }
}
fn ppt_style(s: &DocStyle, align: TextAlign) -> TextStyle {
    let color = s.color.unwrap_or(0);
    TextStyle {
        family: s.font.clone(),
        points: s.size_half_points as f64 / 2.0,
        bold: s.bold,
        italic: s.italic,
        underline: s.underline,
        color: [(color >> 16) as u8, (color >> 8) as u8, color as u8],
        align,
    }
}
fn alignment(a: TextAlign) -> Alignment {
    match a {
        TextAlign::Left => Alignment::Left,
        TextAlign::Center => Alignment::Center,
        TextAlign::Right => Alignment::Right,
    }
}
fn ppt_alignment(a: Alignment) -> TextAlign {
    match a {
        Alignment::Center => TextAlign::Center,
        Alignment::Right => TextAlign::Right,
        _ => TextAlign::Left,
    }
}
pub fn document(object: &SlideObject) -> Result<RichDocument, String> {
    let paragraphs = if object.paragraphs.is_empty() {
        object
            .text
            .replace("\r\n", "\n")
            .split('\n')
            .map(|s| RichParagraph {
                runs: vec![RichRun {
                    text: s.into(),
                    style: object.text_style.clone(),
                }],
                align: object.text_style.align,
                bullet: false,
            })
            .collect::<Vec<_>>()
    } else {
        object.paragraphs.clone()
    };
    let mut document = RichDocument::new();
    document
        .transact(|state| {
            state.blocks = paragraphs
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    Block::Paragraph(Paragraph {
                        id: i as u64 + 1,
                        style: ParagraphStyle {
                            alignment: alignment(p.align),
                            list: p.bullet.then_some(ListMarker {
                                id: 1,
                                level: 0,
                                numbered: false,
                            }),
                            ..Default::default()
                        },
                        runs: p
                            .runs
                            .iter()
                            .map(|r| Run {
                                text: r.text.clone(),
                                style: doc_style(&r.style),
                                link: None,
                            })
                            .collect(),
                    })
                })
                .collect();
            state.next_id = state.blocks.len() as u64 + 1;
            state.selection = Selection::default();
            Ok(())
        })
        .map_err(|e| e.to_string())?;
    Ok(document)
}
pub fn update_object(object: &mut SlideObject, doc: &RichDocument) -> Result<(), String> {
    let mut paragraphs = Vec::new();
    for block in &doc.state().blocks {
        let Block::Paragraph(p) = block else {
            return Err("Only text paragraphs can be placed in a slide object".into());
        };
        let align = ppt_alignment(p.style.alignment);
        let runs = p
            .runs
            .iter()
            .map(|r| RichRun {
                text: r.text.clone(),
                style: ppt_style(&r.style, align),
            })
            .collect();
        paragraphs.push(RichParagraph {
            runs,
            align,
            bullet: p.style.list.is_some(),
        });
    }
    if paragraphs.is_empty() {
        return Err("Text body cannot be empty".into());
    }
    object.text = paragraphs
        .iter()
        .map(|p| p.runs.iter().map(|r| r.text.as_str()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(run) = paragraphs.iter().flat_map(|p| p.runs.iter()).next() {
        object.text_style = run.style.clone();
    }
    object.paragraphs = paragraphs;
    Ok(())
}
pub fn selection_style(
    doc: &RichDocument,
    range: (usize, usize),
) -> Result<(u64, std::ops::Range<usize>, DocStyle, Selection), String> {
    let start = doc
        .position_from_utf16(range.0.min(range.1))
        .map_err(|e| e.to_string())?;
    let end = doc
        .position_from_utf16(range.0.max(range.1))
        .map_err(|e| e.to_string())?;
    if start.paragraph != end.paragraph || start.offset == end.offset {
        return Err("Select text within one paragraph".into());
    }
    let paragraph = doc
        .state()
        .blocks
        .iter()
        .find_map(|b| match b {
            Block::Paragraph(p) if p.id == start.paragraph => Some(p),
            _ => None,
        })
        .ok_or("Paragraph missing")?;
    let mut offset = 0;
    let mut style = DocStyle::default();
    for run in &paragraph.runs {
        if start.offset < offset + run.text.len() {
            style = run.style.clone();
            break;
        }
        offset += run.text.len();
    }
    Ok((
        start.paragraph,
        start.offset..end.offset,
        style,
        Selection {
            anchor: Some(start),
            focus: Some(end),
        },
    ))
}
