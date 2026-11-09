use super::{append_element_content, esc, valid_notes, MAX_TEXT};
use roxmltree::Node;
use std::ops::Range;

const PRESENTATION: &str = "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const SVG: &str = "urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0";

fn locate<'a, 'input>(
    page: Node<'a, 'input>,
) -> Result<Option<(Node<'a, 'input>, Node<'a, 'input>)>, String> {
    let mut notes = page
        .children()
        .filter(|n| n.has_tag_name((PRESENTATION, "notes")));
    let Some(notes_page) = notes.next() else {
        return Ok(None);
    };
    let mut frames = notes_page.descendants().filter(|n| {
        n.has_tag_name((DRAW, "frame")) && n.attribute((PRESENTATION, "class")) == Some("notes")
    });
    let frame = frames.next().ok_or("No standard ODP speaker-notes frame")?;
    let mut boxes = frame
        .children()
        .filter(|n| n.has_tag_name((DRAW, "text-box")));
    let body = boxes.next().ok_or("No ODP speaker-notes text box")?;
    if notes.next().is_some() || frames.next().is_some() || boxes.next().is_some() {
        return Err("Ambiguous ODP speaker-notes layout".into());
    }
    Ok(Some((frame, body)))
}

pub(super) fn read(page: Node<'_, '_>) -> (String, bool) {
    let (_, body) = match locate(page) {
        Ok(Some(value)) => value,
        Ok(None) => return (String::new(), true),
        Err(_) => return (String::new(), false),
    };
    let mut notes = String::new();
    for (index, paragraph) in body
        .descendants()
        .filter(|n| n.has_tag_name((TEXT, "p")))
        .enumerate()
    {
        if index > 0 {
            notes.push('\n');
        }
        for node in paragraph.descendants().skip(1) {
            if node.is_text() {
                notes.push_str(node.text().unwrap_or_default());
            } else if node.has_tag_name((TEXT, "s")) {
                let count = node
                    .attribute((TEXT, "c"))
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(1usize);
                if count > MAX_TEXT.saturating_sub(notes.len()) {
                    return (String::new(), false);
                }
                notes.extend(std::iter::repeat_n(' ', count));
            } else if node.has_tag_name((TEXT, "tab")) {
                notes.push('\t');
            } else if node.has_tag_name((TEXT, "line-break")) {
                notes.push('\n');
            }
            if notes.len() > MAX_TEXT {
                return (String::new(), false);
            }
        }
    }
    if !valid_notes(&notes) {
        return (String::new(), false);
    }
    (notes, plain_style(body).is_ok())
}

fn plain_style<'a>(body: Node<'a, '_>) -> Result<Option<&'a str>, String> {
    let mut style = None;
    for paragraph in body.children().filter(|n| n.is_element()) {
        if !paragraph.has_tag_name((TEXT, "p"))
            || paragraph
                .attributes()
                .any(|attr| attr.namespace() != Some(TEXT) || attr.name() != "style-name")
            || paragraph.children().filter(|n| n.is_element()).any(|n| {
                !matches!(n.tag_name().name(), "s" | "tab")
                    || n.tag_name().namespace() != Some(TEXT)
            })
        {
            return Err("Rich ODP speaker-notes formatting requires a richer editor".into());
        }
        let current = paragraph.attribute((TEXT, "style-name"));
        if style.is_some_and(|prior| prior != current) {
            return Err("Mixed speaker-notes paragraph styles require a richer editor".into());
        }
        style = Some(current);
    }
    Ok(style.flatten())
}

fn paragraphs(notes: &str, style: Option<&str>) -> String {
    let style = style.map_or_else(String::new, |name| {
        format!(" text:style-name=\"{}\"", esc(name))
    });
    notes
        .split('\n')
        .map(|line| {
            let mut body = String::new();
            for (index, token) in line.split(' ').enumerate() {
                if index > 0 {
                    body.push_str("<text:s/>");
                }
                body.push_str(&esc(token).replace('\t', "<text:tab/>"));
            }
            format!("<text:p xmlns:text=\"{TEXT}\"{style}>{body}</text:p>")
        })
        .collect()
}

pub(super) fn generate(notes: &str) -> String {
    if notes.is_empty() {
        return String::new();
    }
    format!(
        "<presentation:notes xmlns:presentation=\"{PRESENTATION}\" xmlns:draw=\"{DRAW}\" xmlns:svg=\"{SVG}\"><draw:frame presentation:class=\"notes\" svg:x=\"2cm\" svg:y=\"14cm\" svg:width=\"17cm\" svg:height=\"12cm\"><draw:text-box>{}</draw:text-box></draw:frame></presentation:notes>",
        paragraphs(notes, None)
    )
}

pub(super) fn patch(
    page: Node<'_, '_>,
    notes: &str,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<String, String> {
    let Some((frame, body)) = locate(page)? else {
        return Ok(generate(notes));
    };
    let replacement = paragraphs(notes, plain_style(body)?);
    let nodes: Vec<_> = body.children().filter(|n| n.is_element()).collect();
    if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
        patches.push((first.range().start..last.range().end, replacement));
    } else {
        let range = body.range();
        let mut xml = source[range.clone()].to_owned();
        append_element_content(&mut xml, &replacement)?;
        patches.push((range, xml));
    }
    if let Some(placeholder) = frame.attribute_node((PRESENTATION, "placeholder")) {
        patches.push((placeholder.range_value(), notes.is_empty().to_string()));
    }
    Ok(String::new())
}
