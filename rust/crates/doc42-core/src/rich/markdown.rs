//! CommonMark/GFM text adapter. Unsupported model features fail before export.
use super::*;
use pulldown_cmark::{html, Options, Parser};
fn unsupported(s: &str) -> Error {
    Error::Unsupported(format!("Markdown {s}"))
}
pub(super) fn decode(bytes: &[u8]) -> Result<State, Error> {
    decode_with(bytes, None)
}
pub(super) fn decode_with(
    bytes: &[u8],
    resolver: Option<&dyn Fn(&str) -> Result<Vec<u8>, Error>>,
) -> Result<State, Error> {
    let input = std::str::from_utf8(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    let parser = Parser::new_ext(input, options);
    let mut body = String::new();
    html::push_html(&mut body, parser);
    let document = format!("<!doctype html><html><body>{body}</body></html>");
    super::html::decode_with(document.as_bytes(), resolver)
}
fn escaped(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if matches!(
            c,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '<'
                | '>'
                | '#'
                | '+'
                | '-'
                | '!'
                | '|'
                | '~'
        ) {
            out.push('\\')
        }
        out.push(c)
    }
    out
}
fn run(r: &Run) -> Result<String, Error> {
    let t = &r.style;
    if t.source_style.is_some()
        || !t.odt_extra.is_empty()
        || t.theme_color.is_some()
        || t.color.is_some()
        || t.highlight.is_some()
        || t.underline
        || t.size_half_points != 24
        || (!matches!(t.font.as_str(), "Arial" | "monospace"))
    {
        return Err(unsupported("character formatting"));
    }
    let raw = escaped(&r.text);
    let mut text = if t.font == "monospace" {
        if r.text.contains('`') {
            return Err(unsupported("inline code containing backtick"));
        }
        format!("`{}`", r.text)
    } else {
        raw
    };
    if t.strike {
        text = format!("~~{text}~~")
    }
    if t.italic {
        text = format!("*{text}*")
    }
    if t.bold {
        text = format!("**{text}**")
    }
    if let Some(url) = &r.link {
        if url.contains(['(', ')', '\n', '\r']) {
            return Err(unsupported("link target"));
        }
        text = format!("[{text}]({url})");
    }
    Ok(text)
}
fn paragraph(p: &Paragraph) -> Result<String, Error> {
    let mut plain = p.style.clone();
    plain.list = None;
    plain.outline_level = None;
    if plain != ParagraphStyle::default() {
        return Err(unsupported("paragraph formatting"));
    }
    let mut out = String::new();
    for r in &p.runs {
        out.push_str(&run(r)?)
    }
    Ok(out)
}
pub(super) fn encode(s: &State) -> Result<Vec<u8>, Error> {
    let (main, _) = encode_resources(s, None)?;
    Ok(main)
}
pub(super) fn encode_resources(
    s: &State,
    assets_name: Option<&str>,
) -> Result<(Vec<u8>, Vec<suite42_core::session::SaveSidecar>), Error> {
    use sha2::{Digest, Sha256};
    let mut sidecars = Vec::new();
    if s.sections.len() != 1
        || s.sections[0].page != PageSettings::default()
        || !s.sections[0].header.is_empty()
        || !s.sections[0].footer.is_empty()
        || !s.comments.is_empty()
        || !s.footnotes.is_empty()
        || !s.revisions.is_empty()
        || !s.fields.is_empty()
    {
        return Err(unsupported("page or review content"));
    }
    let mut out = String::new();
    let mut previous_list: Option<(u64, bool)> = None;
    for block in &s.blocks {
        match block {
            Block::Paragraph(p) => {
                let text = paragraph(p)?;
                if let Some(list) = &p.style.list {
                    if list.level > 8 {
                        return Err(unsupported("list depth"));
                    }
                    if previous_list
                        .is_some_and(|(id, kind)| id != list.id && kind == list.numbered)
                    {
                        return Err(unsupported("adjacent distinct lists"));
                    }
                    out.push_str(&"  ".repeat(list.level as usize));
                    out.push_str(if list.numbered { "1. " } else { "- " });
                    out.push_str(&text);
                    out.push('\n');
                    previous_list = Some((list.id, list.numbered));
                } else {
                    if previous_list.take().is_some() {
                        out.push('\n')
                    }
                    if text.is_empty() && p.style.outline_level.is_none() {
                        out.push_str("<p></p>\n\n");
                        continue;
                    }
                    if let Some(level) = p.style.outline_level {
                        if level > 5 {
                            return Err(unsupported("heading level"));
                        }
                        out.push_str(&"#".repeat(level as usize + 1));
                        out.push(' ')
                    }
                    out.push_str(&text);
                    out.push_str("\n\n");
                }
            }
            Block::Table(table) => {
                let html = table.column_widths_twips.iter().any(|width| *width != 1440)
                    || table.rows.iter().flatten().any(|cell| {
                        cell.row_span != 1
                            || cell.col_span != 1
                            || cell.border.is_some()
                            || cell.fill.is_some()
                            || cell.paragraphs.len() != 1
                            || cell.paragraphs.first().is_some_and(|p| {
                                p.style.list.is_some()
                                    || p.style.outline_level.is_some()
                                    || paragraph(p).is_err()
                            })
                    });
                if html {
                    if previous_list.take().is_some() {
                        out.push('\n');
                    }
                    out.push_str(&super::html::table_markup(table)?);
                    out.push_str("\n\n");
                    continue;
                }
                if previous_list.take().is_some() {
                    out.push('\n')
                }
                if table.rows.is_empty() || table.rows[0].is_empty() {
                    return Err(unsupported("empty table"));
                }
                for (i, row) in table.rows.iter().enumerate() {
                    if row.len() != table.rows[0].len() {
                        return Err(unsupported("irregular table"));
                    }
                    out.push('|');
                    for cell in row {
                        if cell.row_span != 1
                            || cell.col_span != 1
                            || cell.border.is_some()
                            || cell.fill.is_some()
                            || cell.paragraphs.len() != 1
                        {
                            return Err(unsupported("complex table"));
                        }
                        let p = &cell.paragraphs[0];
                        if p.style.list.is_some() || p.style.outline_level.is_some() {
                            return Err(unsupported("table paragraph"));
                        }
                        out.push(' ');
                        out.push_str(&paragraph(p)?);
                        out.push_str(" |");
                    }
                    out.push('\n');
                    if i == 0 {
                        out.push('|');
                        for _ in row {
                            out.push_str(" --- |")
                        }
                        out.push('\n');
                    }
                }
                out.push('\n');
            }
            Block::Image(image) => {
                let dir = assets_name
                    .ok_or_else(|| unsupported("embedded image requires a sidecar save"))?;
                let ext = match image.mime.as_str() {
                    "image/png" if image.data.starts_with(b"\x89PNG\r\n\x1a\n") => "png",
                    "image/jpeg" if image.data.starts_with(&[0xff, 0xd8, 0xff]) => "jpeg",
                    _ => return Err(unsupported("image encoding")),
                };
                let digest = Sha256::digest(&image.data);
                let filename = format!("{digest:x}.{ext}");
                let relative_path = std::path::Path::new(dir).join(&filename);
                if let Some(existing) =
                    sidecars
                        .iter()
                        .find(|entry: &&suite42_core::session::SaveSidecar| {
                            entry.relative_path == relative_path
                        })
                {
                    if existing.bytes != image.data {
                        return Err(unsupported("image hash collision"));
                    }
                } else {
                    sidecars.push(suite42_core::session::SaveSidecar {
                        relative_path,
                        bytes: image.data.clone(),
                    });
                }
                let url = dir
                    .bytes()
                    .map(|b| {
                        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                            (b as char).to_string()
                        } else {
                            format!("%{b:02X}")
                        }
                    })
                    .collect::<String>();
                let alt = image
                    .alt
                    .replace('&', "&amp;")
                    .replace('"', "&quot;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                if previous_list.take().is_some() {
                    out.push('\n');
                }
                out.push_str(&format!("<img src=\"{url}/{filename}\" style=\"width:{}pt;height:{}pt\" alt=\"{alt}\">\n\n", image.width_twips as f64 / 20.0, image.height_twips as f64 / 20.0));
            }
            _ => return Err(unsupported("section break")),
        }
    }
    Ok((out.into_bytes(), sidecars))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn image_bundle_retains_bytes_and_fractional_point_geometry() {
        let png = vec![
            137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1,
            8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248, 207,
            192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66,
            96, 130,
        ];
        let mut s = State::default();
        s.blocks.push(Block::Image(Image {
            id: 2,
            mime: "image/png".into(),
            data: png.clone(),
            width_twips: 1441,
            height_twips: 722,
            alt: "sample & image".into(),
        }));
        s.next_id = 3;
        let (main, sidecars) = encode_resources(&s, Some("my file.assets")).unwrap();
        assert_eq!(sidecars.len(), 1);
        assert_eq!(sidecars[0].bytes, png);
        let doc = decode_with(
            &main,
            Some(&|path| {
                assert!(path.starts_with("my%20file.assets/"));
                Ok(png.clone())
            }),
        )
        .unwrap();
        let image = doc
            .blocks
            .iter()
            .find_map(|b| {
                if let Block::Image(i) = b {
                    Some(i)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!((image.width_twips, image.height_twips), (1441, 722));
        assert_eq!(image.alt, "sample & image");
        assert_eq!(image.data, png);
        let html = super::super::html::encode(&s).unwrap();
        let html_doc = super::super::html::decode(&html).unwrap();
        let image = html_doc
            .blocks
            .iter()
            .find_map(|b| {
                if let Block::Image(i) = b {
                    Some(i)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!((image.width_twips, image.height_twips), (1441, 722));
    }
    #[test]
    fn commonmark_gfm_paragraph_lists_table_round_trip() {
        let input=b"Hello **world** and *friend*.\n\n- first\n- second\n\n| A | B |\n|---|---|\n| x | y |\n";
        let state = decode(input).unwrap();
        assert_eq!(state.blocks.len(), 4);
        let encoded = encode(&state).unwrap();
        let reopened = decode(&encoded).unwrap();
        assert_eq!(reopened.blocks.len(), 4);
        let Block::Paragraph(p) = &reopened.blocks[0] else {
            panic!()
        };
        assert!(p.runs.iter().any(|r| r.text == "world" && r.style.bold));
    }
}
