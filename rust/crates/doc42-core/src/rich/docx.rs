//! Strict WordprocessingML subset: paragraphs, direct run formatting and page geometry.
use super::*;
mod comments;
mod headers;
mod images;
mod links;
mod notes;
mod numbering;
mod styles;
mod toc;
use std::io::{Cursor, Read, Write};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};
const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
fn bad(e: impl std::fmt::Display) -> Error {
    Error::Invalid(e.to_string())
}
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn package(files: &std::collections::BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, Error> {
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in files {
        out.start_file(
            name,
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(bad)?;
        out.write_all(content).map_err(bad)?;
    }
    Ok(out.finish().map_err(bad)?.into_inner())
}
pub(super) fn encode(s: &State) -> Result<Vec<u8>, Error> {
    if s.sections.len() != 1
        || !s.revisions.is_empty()
        || s.blocks
            .iter()
            .any(|b| !matches!(b, Block::Paragraph(_) | Block::Table(_) | Block::Image(_)))
    {
        return Err(Error::Unsupported(
            "DOCX tables, images, section breaks and annotations".into(),
        ));
    }
    let header_ids: std::collections::HashSet<_> = s.sections[0]
        .header
        .iter()
        .chain(&s.sections[0].footer)
        .map(|p| p.id)
        .collect();
    if s.fields.iter().any(|f| {
        !header_ids.contains(&f.at.paragraph)
            || !matches!(f.kind, FieldKind::PageNumber | FieldKind::PageCount)
    }) {
        return Err(Error::Unsupported("DOCX body or custom field".into()));
    }
    let body_ids: std::collections::HashSet<_> = s
        .blocks
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph(p) => vec![p.id],
            Block::Table(t) => t
                .rows
                .iter()
                .flatten()
                .flat_map(|c| c.paragraphs.iter().map(|p| p.id))
                .collect(),
            _ => vec![],
        })
        .collect();
    if s.comments
        .iter()
        .any(|c| !body_ids.contains(&c.start.paragraph) || !body_ids.contains(&c.end.paragraph))
    {
        return Err(Error::Unsupported("DOCX comments outside body text".into()));
    }
    if s.footnotes
        .iter()
        .any(|n| !body_ids.contains(&n.at.paragraph))
    {
        return Err(Error::Unsupported("DOCX note anchors outside body".into()));
    }
    if s.bookmarks.iter().any(|b| {
        !body_ids.contains(&b.start.paragraph)
            || !body_ids.contains(&b.end.paragraph)
            || b.name.len() > 40
            || !b
                .name
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
            || !b
                .name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
            || s.tables_of_contents
                .iter()
                .flat_map(|t| &t.entries)
                .any(|e| {
                    b.name == format!("DOC42_H{}", e.heading)
                        && (b.start.paragraph != e.heading || b.start.offset != 0)
                })
    }) {
        return Err(Error::Unsupported(
            "DOCX bookmark name, target or conflict".into(),
        ));
    }
    if s.sections
        .iter()
        .flat_map(|sec| sec.header.iter().chain(&sec.footer))
        .chain(s.footnotes.iter().flat_map(|n| &n.paragraphs))
        .any(|p| p.runs.iter().any(|r| r.link.is_some()))
    {
        return Err(Error::Unsupported("DOCX links in headers or notes".into()));
    }
    let mut body = String::new();
    for b in &s.blocks {
        match b {
            Block::Paragraph(p) => {
                if super::cached_toc_paragraph(s, p.id)
                    && (s
                        .comments
                        .iter()
                        .any(|c| c.start.paragraph == p.id || c.end.paragraph == p.id)
                        || s.footnotes.iter().any(|n| n.at.paragraph == p.id)
                        || s.bookmarks
                            .iter()
                            .any(|b| b.start.paragraph == p.id || b.end.paragraph == p.id)
                        || s.fields.iter().any(|f| f.at.paragraph == p.id))
                {
                    return Err(Error::Unsupported("annotations in cached Word TOC".into()));
                }
                if let Some(raw) = s.tables_of_contents.iter().find_map(|toc| {
                    let index = toc.entries.iter().position(|e| e.paragraph == p.id)?;
                    toc.docx_cached.as_ref()?.get(index)
                }) {
                    body.push_str(raw);
                    continue;
                }
                let entry = s
                    .tables_of_contents
                    .iter()
                    .flat_map(|t| &t.entries)
                    .find(|e| e.paragraph == p.id);
                let mut copy = p.clone();
                if entry.is_some() {
                    if s.comments
                        .iter()
                        .any(|c| c.start.paragraph == p.id || c.end.paragraph == p.id)
                        || s.footnotes.iter().any(|n| n.at.paragraph == p.id)
                        || s.fields.iter().any(|f| f.at.paragraph == p.id)
                    {
                        return Err(Error::Unsupported("annotations on DOCX TOC entries".into()));
                    }
                    for run in &mut copy.runs {
                        run.link = None;
                    }
                }
                let mut xml = String::new();
                write_paragraph(
                    &mut xml,
                    &copy,
                    &s.fields,
                    &s.comments,
                    &s.footnotes,
                    &s.bookmarks,
                )?;
                let pos = xml
                    .find("</w:pPr>")
                    .ok_or_else(|| bad("DOCX paragraph properties"))?
                    + "</w:pPr>".len();
                if s.tables_of_contents
                    .iter()
                    .filter(|t| t.docx_cached.is_none())
                    .flat_map(|t| &t.entries)
                    .any(|e| e.heading == p.id)
                    && !s.bookmarks.iter().any(|b| {
                        b.name == format!("DOC42_H{}", p.id)
                            && b.start.paragraph == p.id
                            && b.start.offset == 0
                    })
                {
                    xml.insert_str(pos, &format!("<w:bookmarkStart w:id=\"{}\" w:name=\"DOC42_H{}\"/><w:bookmarkEnd w:id=\"{}\"/>", p.id, p.id, p.id));
                }
                if let Some(entry) = entry {
                    xml.insert_str(
                        pos,
                        &format!("<w:hyperlink w:anchor=\"DOC42_H{}\">", entry.heading),
                    );
                    let close = xml.rfind("</w:p>").ok_or_else(|| bad("DOCX paragraph"))?;
                    xml.insert_str(close, "</w:hyperlink>");
                    if let Some(toc) = s
                        .tables_of_contents
                        .iter()
                        .find(|t| t.entries.first().is_some_and(|e| e.paragraph == p.id))
                    {
                        if toc.max_level > 8 {
                            return Err(Error::Unsupported("Word TOC outline level 10".into()));
                        }
                        xml.insert_str(pos, &format!("<w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"1\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> TOC \\o &quot;{}-{}&quot; \\h \\z \\u </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r>", toc.min_level + 1, toc.max_level + 1));
                    }
                    if s.tables_of_contents
                        .iter()
                        .any(|t| t.entries.last().is_some_and(|e| e.paragraph == p.id))
                    {
                        let close = xml.rfind("</w:p>").ok_or_else(|| bad("DOCX paragraph"))?;
                        xml.insert_str(close, "<w:r><w:fldChar w:fldCharType=\"end\"/></w:r>");
                    }
                }
                body.push_str(&xml);
            }
            Block::Table(t) => {
                body.push_str("<w:tbl>");
                if !t.column_widths_twips.is_empty() {
                    body.push_str("<w:tblGrid>");
                    for width in &t.column_widths_twips {
                        body.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
                    }
                    body.push_str("</w:tblGrid>");
                }
                let layout = super::table_edit::table_layout(t)?;
                for (row_index, row) in t.rows.iter().enumerate() {
                    body.push_str("<w:tr>");
                    for (index, cell) in row.iter().enumerate() {
                        let (owner_row, owner_col) = layout[row_index][index];
                        if index != owner_col {
                            continue;
                        }
                        let visual = &t.rows[owner_row][owner_col];
                        if cell.paragraphs.is_empty() {
                            return Err(Error::Unsupported(
                                "DOCX merged, bordered, shaded or empty table cells".into(),
                            ));
                        }
                        body.push_str("<w:tc>");
                        if visual.col_span > 1
                            || visual.row_span > 1
                            || visual.fill.is_some()
                            || visual.border.is_some()
                        {
                            body.push_str("<w:tcPr>");
                            if visual.col_span > 1 {
                                body.push_str(&format!(
                                    "<w:gridSpan w:val=\"{}\"/>",
                                    visual.col_span
                                ));
                            }
                            if visual.row_span > 1 {
                                body.push_str(if row_index == owner_row {
                                    "<w:vMerge w:val=\"restart\"/>"
                                } else {
                                    "<w:vMerge/>"
                                });
                            }
                            if let Some(fill) = visual.fill {
                                if fill > 0xffffff {
                                    return Err(bad("invalid cell background"));
                                }
                                body.push_str(&format!("<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{fill:06X}\"/>"));
                            }
                            if let Some(border) = &visual.border {
                                if border.width_twips == 0
                                    || border.width_twips % 5 != 0
                                    || border.color > 0xffffff
                                {
                                    return Err(Error::Unsupported(
                                        "DOCX table border width or color".into(),
                                    ));
                                }
                                body.push_str("<w:tcBorders>");
                                for edge in ["top", "left", "bottom", "right"] {
                                    let bottom = owner_row + visual.row_span as usize - 1;
                                    if edge == "top" && row_index != owner_row
                                        || edge == "bottom" && row_index != bottom
                                    {
                                        continue;
                                    }
                                    body.push_str(&format!("<w:{edge} w:val=\"single\" w:sz=\"{}\" w:space=\"0\" w:color=\"{:06X}\"/>", u32::from(border.width_twips) * 2 / 5, border.color));
                                }
                                body.push_str("</w:tcBorders>");
                            }
                            body.push_str("</w:tcPr>");
                        }
                        if row_index == owner_row {
                            for p in &cell.paragraphs {
                                write_paragraph(
                                    &mut body,
                                    p,
                                    &s.fields,
                                    &s.comments,
                                    &s.footnotes,
                                    &s.bookmarks,
                                )?;
                            }
                        } else {
                            body.push_str("<w:p/>");
                        }
                        body.push_str("</w:tc>");
                    }
                    body.push_str("</w:tr>");
                }
                body.push_str("</w:tbl>");
            }
            Block::Image(image) => body.push_str(&images::paragraph(image)?),
            _ => return Err(Error::Unsupported("DOCX section break".into())),
        }
    }
    let p = &s.sections[0].page;
    if p.columns != 1 {
        return Err(Error::Unsupported("DOCX columns".into()));
    }
    let mut parts = s.docx_parts.clone();
    let refs = headers::write(&mut parts, &s.sections[0], &s.fields)?;
    body.push_str(&format!("<w:sectPr>{refs}<w:pgSz w:w=\"{}\" w:h=\"{}\"/><w:pgMar w:left=\"{}\" w:right=\"{}\" w:top=\"{}\" w:bottom=\"{}\" w:header=\"{}\" w:footer=\"{}\"/></w:sectPr>",p.width_twips,p.height_twips,p.margin_left,p.margin_right,p.margin_top,p.margin_bottom,p.header_distance_twips,p.footer_distance_twips));
    let doc=format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:document xmlns:w=\"{W}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><w:body>{body}</w:body></w:document>");
    let ct="<?xml version=\"1.0\" encoding=\"UTF-8\"?><Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".to_owned();
    let rel="<?xml version=\"1.0\" encoding=\"UTF-8\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>".to_owned();
    for name in parts.keys() {
        if !safe_part_name(name) {
            return Err(Error::Unsupported(format!("DOCX package part {name}")));
        }
    }
    parts
        .entry("[Content_Types].xml".into())
        .or_insert_with(|| ct.into_bytes());
    parts
        .entry("_rels/.rels".into())
        .or_insert_with(|| rel.into_bytes());
    numbering::add_parts(&mut parts, &s.blocks)?;
    images::add_parts(&mut parts, &s.blocks)?;
    links::write_rels(&mut parts, s)?;
    comments::write(&mut parts, &s.comments)?;
    notes::write(&mut parts, &s.footnotes)?;
    parts.insert("word/document.xml".into(), doc.into_bytes());
    package(&parts)
}
fn write_paragraph(
    body: &mut String,
    p: &Paragraph,
    fields: &[Field],
    comments: &[Annotation],
    notes: &[Footnote],
    bookmarks: &[Bookmark],
) -> Result<(), Error> {
    if !p.style.odt_extra.is_empty() || p.runs.iter().any(|r| !r.style.odt_extra.is_empty()) {
        return Err(Error::Unsupported(
            "DOCX cannot preserve OpenDocument style attributes".into(),
        ));
    }
    let list_ref = p
        .style
        .list
        .as_ref()
        .map(|list| {
            format!(
                "<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{}\"/></w:numPr>",
                list.level, list.id
            )
        })
        .unwrap_or_default();
    let a = match p.style.alignment {
        Alignment::Left => "left",
        Alignment::Center => "center",
        Alignment::Right => "right",
        Alignment::Justify => "both",
    };
    let style_ref = p
        .style
        .source_style
        .as_ref()
        .map(|name| format!("<w:pStyle w:val=\"{}\"/>", esc(name)))
        .unwrap_or_default();
    let extras = format!(
        "<w:keepNext w:val=\"{}\"/><w:keepLines w:val=\"{}\"/><w:pageBreakBefore w:val=\"{}\"/>{}",
        u8::from(p.style.keep_with_next),
        u8::from(p.style.keep_lines),
        u8::from(p.style.page_break_before),
        p.style
            .outline_level
            .map(|v| format!("<w:outlineLvl w:val=\"{v}\"/>"))
            .unwrap_or_default()
    );
    let tabs = if p.style.tabs_twips.is_empty() {
        String::new()
    } else {
        format!(
            "<w:tabs>{}</w:tabs>",
            p.style
                .tabs_twips
                .iter()
                .map(|n| format!("<w:tab w:val=\"left\" w:pos=\"{n}\"/>"))
                .collect::<String>()
        )
    };
    let first = if p.style.first_line_twips < 0 {
        format!(
            "w:hanging=\"{}\"",
            i64::from(p.style.first_line_twips).unsigned_abs()
        )
    } else {
        format!("w:firstLine=\"{}\"", p.style.first_line_twips)
    };
    body.push_str(&format!("<w:p><w:pPr>{style_ref}{list_ref}{extras}{tabs}<w:jc w:val=\"{a}\"/><w:ind w:left=\"{}\" w:right=\"{}\" {first}/><w:spacing w:before=\"{}\" w:after=\"{}\" w:line=\"{}\"/></w:pPr>",p.style.left_twips,p.style.right_twips,p.style.before_twips,p.style.after_twips,p.style.line_twips));
    enum Marker<'a> {
        Field(&'a Field),
        Start(u64),
        End(u64),
        Note(&'a Footnote),
        BookmarkStart(&'a Bookmark),
        BookmarkEnd(u64),
    }
    let mut markers: Vec<(usize, u8, Marker<'_>)> = fields
        .iter()
        .filter(|f| f.at.paragraph == p.id)
        .map(|f| (f.at.offset, 1, Marker::Field(f)))
        .collect();
    for comment in comments {
        if comment.start.paragraph == p.id {
            markers.push((comment.start.offset, 0, Marker::Start(comment.id)));
        }
        if comment.end.paragraph == p.id {
            markers.push((comment.end.offset, 2, Marker::End(comment.id)));
        }
    }
    for bookmark in bookmarks {
        if bookmark.start.paragraph == p.id {
            markers.push((bookmark.start.offset, 0, Marker::BookmarkStart(bookmark)));
        }
        if bookmark.end.paragraph == p.id {
            markers.push((bookmark.end.offset, 2, Marker::BookmarkEnd(bookmark.id)));
        }
    }
    for note in notes {
        if note.at.paragraph == p.id {
            markers.push((note.at.offset, 1, Marker::Note(note)));
        }
    }
    markers.sort_by_key(|(offset, priority, _)| (*offset, *priority));
    let mut index = 0;
    let mut base = 0;
    let emit = |body: &mut String, marker: &Marker<'_>| -> Result<(), Error> {
        match marker {
            Marker::Field(field) => write_field(body, &field.kind)?,
            Marker::Start(id) => body.push_str(&format!("<w:commentRangeStart w:id=\"{id}\"/>")),
            Marker::End(id) => body.push_str(&format!(
                "<w:commentRangeEnd w:id=\"{id}\"/><w:r><w:commentReference w:id=\"{id}\"/></w:r>"
            )),
            Marker::BookmarkStart(bookmark) => body.push_str(&format!(
                "<w:bookmarkStart w:id=\"{}\" w:name=\"{}\"/>",
                bookmark.id,
                esc(&bookmark.name)
            )),
            Marker::BookmarkEnd(id) => body.push_str(&format!("<w:bookmarkEnd w:id=\"{id}\"/>")),
            Marker::Note(note) => {
                let tag = if note.endnote {
                    "endnoteReference"
                } else {
                    "footnoteReference"
                };
                body.push_str(&format!("<w:r><w:{tag} w:id=\"{}\"/></w:r>", note.id));
            }
        }
        Ok(())
    };
    for run in &p.runs {
        let end = base + run.text.len();
        let mut cursor = base;
        while index < markers.len() && markers[index].0 <= end {
            let at = markers[index].0;
            let piece = run
                .text
                .get(cursor - base..at - base)
                .ok_or_else(|| bad("DOCX anchor boundary"))?;
            write_run(body, run, piece)?;
            emit(body, &markers[index].2)?;
            cursor = at;
            index += 1;
        }
        write_run(
            body,
            run,
            run.text
                .get(cursor - base..)
                .ok_or_else(|| bad("DOCX anchor boundary"))?,
        )?;
        base = end;
    }
    while index < markers.len() {
        if markers[index].0 != base {
            return Err(bad("DOCX anchor position"));
        }
        emit(body, &markers[index].2)?;
        index += 1;
    }
    body.push_str("</w:p>");
    Ok(())
}
fn write_field(body: &mut String, kind: &FieldKind) -> Result<(), Error> {
    let instruction = match kind {
        FieldKind::PageNumber => "PAGE",
        FieldKind::PageCount => "NUMPAGES",
        _ => return Err(Error::Unsupported("DOCX field".into())),
    };
    body.push_str(&format!(
        "<w:fldSimple w:instr=\"{instruction}\"><w:r><w:t>1</w:t></w:r></w:fldSimple>"
    ));
    Ok(())
}
fn write_run(body: &mut String, r: &Run, text: &str) -> Result<(), Error> {
    if text.is_empty() {
        return Ok(());
    }
    if r.style.highlight.is_some() {
        return Err(Error::Unsupported("DOCX highlighting".into()));
    }
    if let Some(link) = &r.link {
        if let Some(anchor) = link.strip_prefix('#') {
            body.push_str(&format!("<w:hyperlink w:anchor=\"{}\">", esc(anchor)));
        } else {
            body.push_str(&format!(
                "<w:hyperlink r:id=\"{}\">",
                links::relation_id(link)
            ));
        }
    }
    if r.text
        .chars()
        .any(|c| c == '\0' || (c.is_control() && !matches!(c, '\t' | '\n' | '\r')))
    {
        return Err(Error::Unsupported("XML control character".into()));
    }
    let t = &r.style;
    let style_ref = t
        .source_style
        .as_ref()
        .map(|name| format!("<w:rStyle w:val=\"{}\"/>", esc(name)))
        .unwrap_or_default();
    let color = if let Some(value) = t.color {
        format!(
            "<w:color w:val=\"{value:06X}\"{}{}{} />",
            t.theme_color
                .as_ref()
                .map(|v| format!(" w:themeColor=\"{}\"", esc(v)))
                .unwrap_or_default(),
            t.theme_tint
                .as_ref()
                .map(|v| format!(" w:themeTint=\"{}\"", esc(v)))
                .unwrap_or_default(),
            t.theme_shade
                .as_ref()
                .map(|v| format!(" w:themeShade=\"{}\"", esc(v)))
                .unwrap_or_default()
        )
    } else {
        String::new()
    };
    body.push_str(&format!("<w:r><w:rPr>{style_ref}<w:rFonts w:ascii=\"{}\" w:hAnsi=\"{}\"/><w:sz w:val=\"{}\"/><w:b w:val=\"{}\"/><w:i w:val=\"{}\"/><w:u w:val=\"{}\"/><w:strike w:val=\"{}\"/>{color}</w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",esc(&t.font),esc(&t.font),t.size_half_points,u8::from(t.bold),u8::from(t.italic),if t.underline {"single"} else {"none"},u8::from(t.strike),esc(text)));
    if r.link.is_some() {
        body.push_str("</w:hyperlink>");
    }
    Ok(())
}
fn val<'a>(n: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    n.attribute((W, name))
}
fn num<T: std::str::FromStr>(
    n: roxmltree::Node<'_, '_>,
    name: &str,
    default: T,
) -> Result<T, Error> {
    match val(n, name) {
        Some(x) => x.parse().map_err(|_| bad("invalid Word number")),
        None => Ok(default),
    }
}
fn allowed(n: roxmltree::Node<'_, '_>, names: &[&str]) -> Result<(), Error> {
    for c in n.children().filter(|x| x.is_element()) {
        if c.tag_name().namespace() != Some(W) || !names.contains(&c.tag_name().name()) {
            return Err(Error::Unsupported(format!(
                "DOCX element {}",
                c.tag_name().name()
            )));
        }
    }
    Ok(())
}
fn safe_part_name(name: &str) -> bool {
    !name.contains("..")
        && !name.starts_with('/')
        && ([
            "[Content_Types].xml",
            "_rels/.rels",
            "word/document.xml",
            "word/styles.xml",
            "word/stylesWithEffects.xml",
            "word/numbering.xml",
            "word/settings.xml",
            "word/fontTable.xml",
            "word/webSettings.xml",
            "word/_rels/document.xml.rels",
        ]
        .contains(&name)
            || name.starts_with("docProps/")
            || name.starts_with("word/theme/")
            || name.starts_with("word/media/")
            || ((name.starts_with("word/footnotes") || name.starts_with("word/endnotes"))
                && name.ends_with(".xml")
                && !name[5..].contains('/'))
            || ((name.starts_with("word/comments") && name.ends_with(".xml"))
                && !name[5..].contains('/'))
            || ((name.starts_with("word/header") || name.starts_with("word/footer"))
                && name.ends_with(".xml")
                && !name[5..].contains('/')))
}
pub(super) fn decode(bytes: &[u8]) -> Result<State, Error> {
    let mut z = ZipArchive::new(Cursor::new(bytes)).map_err(bad)?;
    if z.len() > 128 {
        return Err(Error::Limit);
    }
    let mut parts = std::collections::BTreeMap::new();
    let mut total = 0u64;
    for i in 0..z.len() {
        let f = z.by_index(i).map_err(bad)?;
        let name = f.name().to_owned();
        if f.is_dir() {
            continue;
        }
        if !safe_part_name(&name) {
            return Err(Error::Unsupported(format!("DOCX package part {name}")));
        }
        total = total.checked_add(f.size()).ok_or(Error::Limit)?;
        if total > MAX_BYTES as u64 || parts.contains_key(&name) {
            return Err(Error::Limit);
        }
        let mut content = Vec::new();
        f.take((MAX_BYTES + 1) as u64)
            .read_to_end(&mut content)
            .map_err(bad)?;
        parts.insert(name, content);
    }
    let xml = String::from_utf8(
        parts
            .remove("word/document.xml")
            .ok_or_else(|| bad("missing document"))?,
    )
    .map_err(bad)?;
    if let Some(rels) = parts.get("word/_rels/document.xml.rels") {
        let text = std::str::from_utf8(rels).map_err(bad)?;
        let d = roxmltree::Document::parse(text).map_err(bad)?;
        for r in d
            .descendants()
            .filter(|n| n.is_element() && n.tag_name().name() == "Relationship")
        {
            let t = r.attribute("Type").unwrap_or("");
            if ![
                "/styles",
                "/settings",
                "/fontTable",
                "/webSettings",
                "/theme",
                "/numbering",
                "/hyperlink",
                "/image",
                "/header",
                "/footer",
                "/comments",
                "/footnotes",
                "/endnotes",
                "/stylesWithEffects",
            ]
            .iter()
            .any(|suffix| t.ends_with(suffix))
                || (r.attribute("TargetMode").is_some()
                    && !(t.ends_with("/hyperlink")
                        && r.attribute("TargetMode") == Some("External")))
            {
                return Err(Error::Unsupported(format!("DOCX relationship {t}")));
            }
        }
    }
    let (xml, toc_scan) = toc::normalize(&xml)?;
    let (xml, link_scan) = links::normalize(&xml, &parts)?;
    let doc = roxmltree::Document::parse(&xml).map_err(bad)?;
    for n in doc.descendants().filter(|n| n.is_element()) {
        if n.has_tag_name((W, "drawing")) || n.ancestors().any(|a| a.has_tag_name((W, "drawing"))) {
            continue;
        }
        let accepted: &[&str] = match n.tag_name().name() {
            "document" | "body" | "p" | "pPr" | "r" | "rPr" | "sectPr" | "tbl" | "tr" | "tc"
            | "numPr" | "tabs" | "tcPr" | "tcBorders" | "tblGrid" => &[],
            "headerReference" | "footerReference" => &["type", "id"],
            "gridCol" => &["w"],
            "vMerge" => &["val"],
            "fldSimple" => &["instr"],
            "commentRangeStart" | "commentRangeEnd" | "commentReference" | "footnoteReference"
            | "endnoteReference" => &["id"],
            "fldChar" => &["fldCharType", "dirty", "fldLock"],
            "instrText" => &["space"],
            "jc" | "sz" | "b" | "i" | "u" | "strike" | "pStyle" | "rStyle" | "ilvl" | "numId"
            | "keepNext" | "keepLines" | "pageBreakBefore" | "outlineLvl" | "gridSpan" => &["val"],
            "tab" => &["val", "pos"],
            "shd" => &["val", "color", "fill"],
            "top" | "left" | "bottom" | "right" => &["val", "sz", "space", "color"],
            "color" => &["val", "themeColor", "themeTint", "themeShade"],
            "ind" => &["left", "right", "firstLine", "hanging"],
            "spacing" => &["before", "after", "line"],
            "rFonts" => &["ascii", "hAnsi", "asciiTheme", "hAnsiTheme"],
            "pgSz" => &["w", "h"],
            "pgMar" => &["left", "right", "top", "bottom", "header", "footer"],
            "t" => &["space"],
            _ => {
                return Err(Error::Unsupported(format!(
                    "DOCX element {}",
                    n.tag_name().name()
                )))
            }
        };
        for a in n.attributes() {
            let word_revision = a.namespace() == Some(W)
                && matches!(
                    a.name(),
                    "rsidR" | "rsidRDefault" | "rsidP" | "rsidRPr" | "rsidSect"
                );
            let compat = a.namespace()
                == Some("http://schemas.openxmlformats.org/markup-compatibility/2006")
                && a.name() == "Ignorable"
                && n.has_tag_name((W, "document"));
            let tracking = a.namespace()
                == Some("http://schemas.microsoft.com/office/word/2010/wordml")
                && matches!(a.name(), "paraId" | "textId");
            if word_revision || compat || tracking {
                continue;
            }
            let expected_ns =
                if matches!(n.tag_name().name(), "headerReference" | "footerReference")
                    && a.name() == "id"
                {
                    Some("http://schemas.openxmlformats.org/officeDocument/2006/relationships")
                } else if matches!(n.tag_name().name(), "t" | "instrText") && a.name() == "space" {
                    Some("http://www.w3.org/XML/1998/namespace")
                } else {
                    Some(W)
                };
            if a.namespace() != expected_ns || !accepted.contains(&a.name()) {
                return Err(Error::Unsupported(format!("DOCX attribute {}", a.name())));
            }
        }
        if n.has_tag_name((W, "rFonts")) && val(n, "ascii") != val(n, "hAnsi") {
            return Err(Error::Unsupported("different script fonts".into()));
        }
        if n.has_tag_name((W, "u"))
            && !matches!(val(n, "val"), None | Some("single" | "none" | "0"))
        {
            return Err(Error::Unsupported("underline type".into()));
        }
    }
    let numbering = numbering::Numbering::read(&parts)?;
    let styles = styles::Styles::new(&parts, &numbering)?;
    let root = doc.root_element();
    if root.tag_name().namespace() != Some(W) || root.tag_name().name() != "document" {
        return Err(bad("not WordprocessingML"));
    }
    allowed(root, &["body"])?;
    let body = root
        .children()
        .find(|n| n.is_element())
        .ok_or_else(|| bad("missing body"))?;
    allowed(body, &["p", "tbl", "sectPr"])?;
    let mut s = State {
        docx_parts: parts,
        ..State::default()
    };
    s.blocks.clear();
    let mut markers: Vec<(u64, usize, FieldKind)> = Vec::new();
    let mut comment_marks: Vec<(u64, usize, u64, u8)> = Vec::new();
    let mut note_marks: Vec<(u64, usize, u64, bool)> = Vec::new();
    let mut body_ids = std::collections::BTreeMap::new();
    let mut paragraph_positions = std::collections::BTreeMap::new();
    for (body_index, node) in body.children().filter(|n| n.is_element()).enumerate() {
        if node.has_tag_name((W, "p")) {
            body_ids.insert(body_index, s.next_id);
        }
        match node.tag_name().name() {
            "p" if node.descendants().any(|n| n.has_tag_name((W, "drawing"))) => {
                let id = s.next_id;
                s.next_id += 1;
                s.blocks
                    .push(Block::Image(images::read(node, id, &s.docx_parts)?));
            }
            "p" => {
                paragraph_positions.insert(node.range().start, s.next_id);
                let id = s.next_id;
                s.next_id += 1;
                s.blocks.push(Block::Paragraph(read_paragraph(
                    node,
                    id,
                    &styles,
                    &numbering,
                    &mut markers,
                    &mut comment_marks,
                    &mut note_marks,
                )?));
            }
            "tbl" => {
                allowed(node, &["tr", "tblGrid"])?;
                let mut widths = Vec::new();
                if let Some(grid) = node.children().find(|n| n.has_tag_name((W, "tblGrid"))) {
                    allowed(grid, &["gridCol"])?;
                    for column in grid.children().filter(|n| n.is_element()) {
                        let width = num(column, "w", 0u32)?;
                        if width == 0 || width > 2_000_000 {
                            return Err(bad("invalid table column width"));
                        }
                        widths.push(width);
                    }
                }
                let id = s.next_id;
                s.next_id += 1;
                let mut rows = Vec::new();
                let mut vertical = Vec::new();
                let mut border_rows = Vec::new();
                for row in node.children().filter(|n| n.has_tag_name((W, "tr"))) {
                    allowed(row, &["tc"])?;
                    let mut cells = Vec::new();
                    let mut vertical_row = Vec::new();
                    let mut border_row = Vec::new();
                    for cell in row.children().filter(|n| n.is_element()) {
                        allowed(cell, &["p", "tcPr"])?;
                        let properties = cell.children().find(|n| n.has_tag_name((W, "tcPr")));
                        if let Some(p) = properties {
                            allowed(p, &["gridSpan", "shd", "tcBorders", "vMerge"])?;
                        }
                        let span = properties
                            .and_then(|p| p.children().find(|n| n.has_tag_name((W, "gridSpan"))))
                            .map(|n| num(n, "val", 1u16))
                            .transpose()?
                            .unwrap_or(1);
                        let vertical_flag = if let Some(v) = properties
                            .and_then(|p| p.children().find(|n| n.has_tag_name((W, "vMerge"))))
                        {
                            match val(v, "val") {
                                Some("restart") => 1,
                                None | Some("continue") => 2,
                                _ => {
                                    return Err(Error::Unsupported(
                                        "DOCX vertical merge marker".into(),
                                    ))
                                }
                            }
                        } else {
                            0
                        };
                        let mut edge_values: [Option<Border>; 4] = Default::default();
                        if let Some(borders) = properties
                            .and_then(|p| p.children().find(|n| n.has_tag_name((W, "tcBorders"))))
                        {
                            allowed(borders, &["top", "left", "bottom", "right"])?;
                            for (index, name) in
                                ["top", "left", "bottom", "right"].into_iter().enumerate()
                            {
                                let Some(edge) =
                                    borders.children().find(|n| n.has_tag_name((W, name)))
                                else {
                                    continue;
                                };
                                if val(edge, "val") != Some("single")
                                    || val(edge, "space").is_some_and(|v| v != "0")
                                {
                                    return Err(Error::Unsupported(
                                        "DOCX cell border style".into(),
                                    ));
                                }
                                let eighths = num(edge, "sz", 0u16)?;
                                if eighths == 0 || eighths % 2 != 0 {
                                    return Err(Error::Unsupported(
                                        "DOCX fractional cell border".into(),
                                    ));
                                }
                                let width_twips = u16::try_from(u32::from(eighths) * 5 / 2)
                                    .map_err(|_| Error::Limit)?;
                                let raw =
                                    val(edge, "color").ok_or_else(|| bad("cell border color"))?;
                                if raw.len() != 6 {
                                    return Err(Error::Unsupported(
                                        "DOCX cell border color".into(),
                                    ));
                                }
                                edge_values[index] = Some(Border {
                                    width_twips,
                                    color: u32::from_str_radix(raw, 16).map_err(bad)?,
                                });
                            }
                        }
                        let fill = if let Some(shading) = properties
                            .and_then(|p| p.children().find(|n| n.has_tag_name((W, "shd"))))
                        {
                            if val(shading, "val").is_some_and(|v| v != "clear")
                                || val(shading, "color").is_some_and(|v| v != "auto")
                            {
                                return Err(Error::Unsupported(
                                    "DOCX patterned table shading".into(),
                                ));
                            }
                            match val(shading, "fill") {
                                None | Some("auto") => None,
                                Some(v) if v.len() == 6 => {
                                    Some(u32::from_str_radix(v, 16).map_err(bad)?)
                                }
                                _ => return Err(bad("invalid cell background")),
                            }
                        } else {
                            None
                        };
                        if span == 0 || span > 256 {
                            return Err(bad("invalid table cell span"));
                        }
                        let mut paragraphs = Vec::new();
                        for p in cell.children().filter(|n| n.has_tag_name((W, "p"))) {
                            paragraph_positions.insert(p.range().start, s.next_id);
                            let pid = s.next_id;
                            s.next_id += 1;
                            paragraphs.push(read_paragraph(
                                p,
                                pid,
                                &styles,
                                &numbering,
                                &mut markers,
                                &mut comment_marks,
                                &mut note_marks,
                            )?);
                        }
                        if paragraphs.is_empty() {
                            return Err(bad("empty table cell"));
                        }
                        cells.push(Cell {
                            paragraphs,
                            row_span: 1,
                            col_span: span,
                            border: None,
                            fill,
                        });
                        for i in 0..span as usize {
                            border_row.push([
                                edge_values[0].clone(),
                                if i == 0 { edge_values[1].clone() } else { None },
                                edge_values[2].clone(),
                                if i + 1 == span as usize {
                                    edge_values[3].clone()
                                } else {
                                    None
                                },
                            ]);
                        }
                        vertical_row.extend(std::iter::repeat_n(vertical_flag, span as usize));
                        for _ in 1..span {
                            let pid = s.next_id;
                            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
                            cells.push(Cell {
                                paragraphs: vec![Paragraph {
                                    id: pid,
                                    style: ParagraphStyle::default(),
                                    runs: vec![Run {
                                        text: String::new(),
                                        style: TextStyle::default(),
                                        link: None,
                                    }],
                                }],
                                row_span: 1,
                                col_span: 1,
                                border: None,
                                fill: None,
                            });
                        }
                    }
                    rows.push(cells);
                    vertical.push(vertical_row);
                    border_rows.push(border_row);
                }
                if !widths.is_empty() && rows.iter().any(|row| row.len() != widths.len()) {
                    return Err(Error::Unsupported(
                        "DOCX table grid does not match cells".into(),
                    ));
                }
                let mut table = Table {
                    id,
                    rows,
                    column_widths_twips: widths,
                };
                super::table_edit::resolve_vertical(&mut table, &vertical)?;
                super::table_edit::apply_perimeter_borders(&mut table, &border_rows)?;
                s.blocks.push(Block::Table(table));
            }
            "sectPr" => {}
            _ => unreachable!(),
        }
    }
    if s.blocks.is_empty() {
        return Err(bad("empty body"));
    }
    if let Some(sec) = body.children().find(|n| n.has_tag_name((W, "sectPr"))) {
        allowed(
            sec,
            &["pgSz", "pgMar", "headerReference", "footerReference"],
        )?;
        for x in sec.children().filter(|n| n.is_element()) {
            let p = &mut s.sections[0].page;
            match x.tag_name().name() {
                "pgSz" => {
                    p.width_twips = num(x, "w", p.width_twips)?;
                    p.height_twips = num(x, "h", p.height_twips)?
                }
                "pgMar" => {
                    p.margin_left = num(x, "left", p.margin_left)?;
                    p.margin_right = num(x, "right", p.margin_right)?;
                    p.margin_top = num(x, "top", p.margin_top)?;
                    p.margin_bottom = num(x, "bottom", p.margin_bottom)?;
                    p.header_distance_twips = num(x, "header", p.header_distance_twips)?;
                    p.footer_distance_twips = num(x, "footer", p.footer_distance_twips)?
                }
                "headerReference" | "footerReference" => {}
                _ => unreachable!(),
            }
        }
        for reference in sec.children().filter(|n| {
            n.has_tag_name((W, "headerReference")) || n.has_tag_name((W, "footerReference"))
        }) {
            if val(reference, "type") != Some("default") {
                return Err(Error::Unsupported("DOCX alternate header/footer".into()));
            }
            let rid = reference
                .attribute((
                    "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                    "id",
                ))
                .ok_or_else(|| bad("missing header relation"))?;
            let footer = reference.has_tag_name((W, "footerReference"));
            let bytes = headers::read(&s.docx_parts, rid, footer)?;
            let text = std::str::from_utf8(&bytes).map_err(bad)?;
            let hdr = roxmltree::Document::parse(text).map_err(bad)?;
            let root = hdr.root_element();
            let name = if footer { "ftr" } else { "hdr" };
            if !root.has_tag_name((W, name)) {
                return Err(bad("invalid header/footer part"));
            }
            allowed(root, &["p"])?;
            let mut paras = Vec::new();
            for p in root.children().filter(|n| n.is_element()) {
                let id = s.next_id;
                s.next_id += 1;
                let mut header_comments = Vec::new();
                let mut header_notes = Vec::new();
                paras.push(read_paragraph(
                    p,
                    id,
                    &styles,
                    &numbering,
                    &mut markers,
                    &mut header_comments,
                    &mut header_notes,
                )?);
                if !header_comments.is_empty() || !header_notes.is_empty() {
                    return Err(Error::Unsupported("DOCX header comments or notes".into()));
                }
            }
            if footer {
                s.sections[0].footer = paras
            } else {
                s.sections[0].header = paras
            }
        }
    }
    let mut note_data = notes::read(&s.docx_parts, &styles, &numbering, &mut s.next_id)?;
    for (paragraph, offset, external, endnote) in note_marks {
        let paragraphs = note_data
            .remove(&(endnote, external))
            .ok_or_else(|| bad("missing DOCX note"))?;
        let id = s.next_id;
        s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
        s.footnotes.push(Footnote {
            id,
            at: Position { paragraph, offset },
            paragraphs,
            endnote,
            citation: None,
        });
    }
    if !note_data.is_empty() {
        return Err(Error::Unsupported("unanchored DOCX notes".into()));
    }
    let mut data = comments::read(&s.docx_parts)?;
    let mut grouped: std::collections::BTreeMap<
        u64,
        (Option<Position>, Option<Position>, Option<Position>),
    > = std::collections::BTreeMap::new();
    for (paragraph, offset, external, kind) in comment_marks {
        let entry = grouped.entry(external).or_default();
        let slot = match kind {
            0 => &mut entry.0,
            1 => &mut entry.1,
            _ => &mut entry.2,
        };
        if slot.replace(Position { paragraph, offset }).is_some() {
            return Err(bad("duplicate DOCX comment marker"));
        }
    }
    for (external, (start, end, reference)) in grouped {
        let item = data
            .remove(&external)
            .ok_or_else(|| bad("comment marker without comment"))?;
        let (start, end) = match (start, end, reference) {
            (Some(start), Some(end), _) => (start, end),
            (None, None, Some(at)) => (at.clone(), at),
            _ => return Err(bad("incomplete DOCX comment range")),
        };
        let id = s.next_id;
        s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
        s.comments.push(Annotation {
            id,
            start,
            end,
            author: item.author,
            text: item.text,
            resolved: false,
            date: item.date,
            initials: item.initials,
        });
    }
    if !data.is_empty() {
        return Err(Error::Unsupported("unanchored DOCX comments".into()));
    }
    for (paragraph, offset, kind) in markers {
        let id = s.next_id;
        s.next_id += 1;
        s.fields.push(Field {
            id,
            at: Position { paragraph, offset },
            kind,
        });
    }
    for (position, start, end, target) in link_scan.links {
        let id = *paragraph_positions
            .get(&position)
            .ok_or_else(|| bad("DOCX link paragraph"))?;
        let p = paragraph_mut(&mut s, id).ok_or_else(|| bad("DOCX link paragraph"))?;
        super::links::apply_link(p, start..end, Some(target))?;
    }
    for (name, start_para, start, end_para, end) in link_scan.bookmarks {
        let start = Position {
            paragraph: *paragraph_positions
                .get(&start_para)
                .ok_or_else(|| bad("DOCX bookmark start"))?,
            offset: start,
        };
        let end = Position {
            paragraph: *paragraph_positions
                .get(&end_para)
                .ok_or_else(|| bad("DOCX bookmark end"))?,
            offset: end,
        };
        let id = s.next_id;
        s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
        s.bookmarks.push(Bookmark {
            id,
            name,
            start,
            end,
        });
    }
    if let Some(scan) = toc_scan {
        for ((table, cached), (min_level, max_level)) in
            scan.tables.into_iter().zip(scan.cached).zip(scan.levels)
        {
            let id = s.next_id;
            s.next_id = s.next_id.checked_add(1).ok_or(Error::Limit)?;
            let mut entries = Vec::new();
            for (index, name) in table {
                let paragraph = *body_ids
                    .get(&index)
                    .ok_or_else(|| bad("Word TOC paragraph"))?;
                let heading_index = scan
                    .bookmarks
                    .get(&name)
                    .ok_or_else(|| bad("Word TOC heading"))?;
                let heading = *body_ids
                    .get(heading_index)
                    .ok_or_else(|| bad("Word TOC heading paragraph"))?;
                let p = paragraph_mut(&mut s, paragraph).ok_or_else(|| bad("Word TOC entry"))?;
                for run in &mut p.runs {
                    run.link = Some(format!("#DOC42_H{heading}"));
                }
                entries.push(TocEntry { paragraph, heading });
            }
            s.tables_of_contents.push(TableOfContents {
                id,
                entries,
                min_level,
                max_level,
                docx_cached: cached,
                odt_cached: None,
            });
        }
    }
    Ok(s)
}

fn page_field(instruction: &str) -> Result<FieldKind, Error> {
    let mut words = instruction.split_whitespace();
    let kind = match words.next().unwrap_or("").to_ascii_uppercase().as_str() {
        "PAGE" => FieldKind::PageNumber,
        "NUMPAGES" => FieldKind::PageCount,
        _ => return Err(Error::Unsupported("DOCX field instruction".into())),
    };
    while let Some(switch) = words.next() {
        let value = words
            .next()
            .ok_or_else(|| Error::Unsupported("DOCX field switches".into()))?;
        if switch != "\\*"
            || !(value.eq_ignore_ascii_case("MERGEFORMAT") || value.eq_ignore_ascii_case("ARABIC"))
        {
            return Err(Error::Unsupported("DOCX field switches".into()));
        }
    }
    Ok(kind)
}
fn read_paragraph(
    p: roxmltree::Node<'_, '_>,
    id: u64,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
    markers: &mut Vec<(u64, usize, FieldKind)>,
    comment_marks: &mut Vec<(u64, usize, u64, u8)>,
    note_marks: &mut Vec<(u64, usize, u64, bool)>,
) -> Result<Paragraph, Error> {
    allowed(
        p,
        &[
            "pPr",
            "r",
            "fldSimple",
            "commentRangeStart",
            "commentRangeEnd",
        ],
    )?;
    let props = p.children().find(|n| n.has_tag_name((W, "pPr")));
    if let Some(n) = props {
        allowed(
            n,
            &[
                "pStyle",
                "jc",
                "ind",
                "spacing",
                "numPr",
                "keepNext",
                "keepLines",
                "pageBreakBefore",
                "outlineLvl",
                "tabs",
            ],
        )?;
    }
    let (base, mut style) = styles.paragraph(props, numbering)?;
    if let Some(numpr) = props.and_then(|n| n.children().find(|c| c.has_tag_name((W, "numPr")))) {
        allowed(numpr, &["ilvl", "numId"])?;
        let level = numpr
            .children()
            .find(|c| c.has_tag_name((W, "ilvl")))
            .map(|c| num(c, "val", 0u8))
            .transpose()?
            .unwrap_or(0);
        if let Some(id) = numpr
            .children()
            .find(|c| c.has_tag_name((W, "numId")))
            .map(|c| num(c, "val", 0u64))
            .transpose()?
        {
            if id != 0 {
                style.list = Some(numbering.marker(id, level)?);
            }
        }
    }
    let mut runs = Vec::new();
    // Word commonly stores fields as begin/instruction/separate/result/end runs.
    // Keep the field itself, not its cached result, as the authoritative content.
    let mut field: Option<(String, bool)> = None;
    for r in p
        .children()
        .filter(|n| n.is_element() && !n.has_tag_name((W, "pPr")))
    {
        if r.has_tag_name((W, "commentRangeStart")) || r.has_tag_name((W, "commentRangeEnd")) {
            allowed(r, &[])?;
            let external = val(r, "id")
                .ok_or_else(|| bad("comment id"))?
                .parse::<u64>()
                .map_err(bad)?;
            let offset = runs.iter().map(|r: &Run| r.text.len()).sum();
            comment_marks.push((
                id,
                offset,
                external,
                if r.has_tag_name((W, "commentRangeStart")) {
                    0
                } else {
                    1
                },
            ));
            continue;
        }
        if r.has_tag_name((W, "fldSimple")) {
            if field.is_some() {
                return Err(Error::Unsupported("nested DOCX field".into()));
            }
            allowed(r, &["r"])?;
            for cached in r.children().filter(|n| n.is_element()) {
                allowed(cached, &["rPr", "t"])?;
                if let Some(properties) = cached.children().find(|n| n.has_tag_name((W, "rPr"))) {
                    allowed(
                        properties,
                        &["rStyle", "rFonts", "sz", "b", "i", "u", "strike", "color"],
                    )?;
                }
            }
            let kind = page_field(val(r, "instr").unwrap_or(""))?;
            let offset = runs.iter().map(|r: &Run| r.text.len()).sum();
            markers.push((id, offset, kind));
            continue;
        }
        allowed(
            r,
            &[
                "rPr",
                "t",
                "tab",
                "fldChar",
                "instrText",
                "commentReference",
                "footnoteReference",
                "endnoteReference",
                "footnoteRef",
                "endnoteRef",
            ],
        )?;
        let rp = r.children().find(|n| n.has_tag_name((W, "rPr")));
        if let Some(n) = rp {
            allowed(
                n,
                &["rStyle", "rFonts", "sz", "b", "i", "u", "strike", "color"],
            )?;
        }
        let reference_only = r
            .children()
            .filter(|n| n.is_element() && !n.has_tag_name((W, "rPr")))
            .all(|n| {
                [
                    "commentReference",
                    "footnoteReference",
                    "endnoteReference",
                    "footnoteRef",
                    "endnoteRef",
                ]
                .iter()
                .any(|tag| n.has_tag_name((W, *tag)))
            });
        let run_style = if reference_only {
            base.clone()
        } else {
            styles.run(rp, &base)?
        };
        for child in r
            .children()
            .filter(|n| n.is_element() && !n.has_tag_name((W, "rPr")))
        {
            allowed(child, &[])?;
            if child.has_tag_name((W, "fldChar"))
                && val(child, "fldLock").is_some_and(|v| !matches!(v, "0" | "false" | "off"))
            {
                return Err(Error::Unsupported("locked DOCX field".into()));
            }
            match child.tag_name().name() {
                "fldChar" => match val(child, "fldCharType") {
                    Some("begin") if field.is_none() => field = Some((String::new(), false)),
                    Some("separate") => {
                        let (instruction, separated) = field
                            .as_mut()
                            .ok_or_else(|| bad("DOCX field separator without begin"))?;
                        if *separated {
                            return Err(bad("duplicate DOCX field separator"));
                        }
                        let kind = page_field(instruction)?;
                        let offset = runs.iter().map(|r: &Run| r.text.len()).sum();
                        markers.push((id, offset, kind));
                        *separated = true;
                    }
                    Some("end") if field.as_ref().is_some_and(|(_, separated)| *separated) => {
                        field = None;
                    }
                    _ => return Err(Error::Unsupported("nested or malformed DOCX field".into())),
                },
                "instrText" => match field.as_mut() {
                    Some((instruction, false)) => instruction.push_str(child.text().unwrap_or("")),
                    _ => return Err(bad("DOCX instruction outside field")),
                },
                "footnoteRef" | "endnoteRef" => {}
                "footnoteReference" | "endnoteReference" => {
                    if field.is_some() {
                        return Err(Error::Unsupported("note in DOCX field".into()));
                    }
                    let external = val(child, "id")
                        .ok_or_else(|| bad("note id"))?
                        .parse::<u64>()
                        .map_err(bad)?;
                    let offset = runs.iter().map(|r: &Run| r.text.len()).sum();
                    note_marks.push((
                        id,
                        offset,
                        external,
                        child.has_tag_name((W, "endnoteReference")),
                    ));
                }
                "commentReference" => {
                    if field.is_some() {
                        return Err(Error::Unsupported("comment in DOCX field".into()));
                    }
                    let external = val(child, "id")
                        .ok_or_else(|| bad("comment id"))?
                        .parse::<u64>()
                        .map_err(bad)?;
                    let offset = runs.iter().map(|r: &Run| r.text.len()).sum();
                    comment_marks.push((id, offset, external, 2));
                }
                "tab" => {
                    if field.is_none() {
                        runs.push(Run {
                            text: "\t".into(),
                            style: run_style.clone(),
                            link: None,
                        });
                    }
                }
                "t" => {
                    if field.as_ref().is_some_and(|(_, separated)| !separated) {
                        return Err(bad("DOCX field result before separator"));
                    }
                    if field.is_none() {
                        runs.push(Run {
                            text: child.text().unwrap_or("").into(),
                            style: run_style.clone(),
                            link: None,
                        });
                    }
                }
                _ => unreachable!(),
            }
        }
    }
    if field.is_some() {
        return Err(bad("unclosed DOCX field"));
    }
    Ok(Paragraph { id, style, runs })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_word_toc_cache_survives_body_edit_and_update_invalidates_it() {
        let document = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="1"/></w:r><w:r><w:instrText> TOC \o &quot;1-3&quot; \h \z \u </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:hyperlink w:anchor="_Toc42"><w:r><w:t>Heading</w:t></w:r><w:r><w:tab/></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF _Toc42 \h </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:hyperlink><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:t>Body</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:bookmarkStart w:id="9" w:name="_Toc42"/><w:r><w:t>Heading</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:sectPr/></w:body></w:document>"#
        );
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), document.into_bytes());
        let mut doc = RichDocument::decode(&package(&parts).unwrap(), Format::Docx).unwrap();
        assert_eq!(doc.state().tables_of_contents[0].max_level, 2);
        assert!(doc.state().tables_of_contents[0].docx_cached.is_some());
        let projection = String::from_utf8(doc.rich_edit_projection().unwrap())
            .unwrap()
            .replacen("Body", "Body edited", 1);
        doc.apply_rich_edit_projection(projection.as_bytes(), (0, 0))
            .unwrap();
        let output = doc.encode(Format::Docx).unwrap();
        let reopened = RichDocument::decode(&output, Format::Docx).unwrap();
        let cached = &reopened.state().tables_of_contents[0]
            .docx_cached
            .as_ref()
            .unwrap()[0];
        assert!(cached.contains("PAGEREF _Toc42"));
        assert!(cached.contains(">7</w:t>"));
        let toc = doc.state().tables_of_contents[0].id;
        doc.update_toc(toc).unwrap();
        assert!(doc.state().tables_of_contents[0].docx_cached.is_none());
        let updated =
            RichDocument::decode(&doc.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        assert!(updated.state().tables_of_contents[0].docx_cached.is_none());
        assert_eq!(updated.state().tables_of_contents[0].max_level, 2);
    }
    #[test]
    fn external_word_links_and_unrelated_bookmarks_roundtrip() {
        let document = format!(
            r#"<w:document xmlns:w="{W}" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:bookmarkStart w:id="7" w:name="MyMark"/><w:r><w:t>Target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:hyperlink r:id="rId4" w:history="1"><w:r><w:t>Visit</w:t></w:r></w:hyperlink><w:r><w:t> </w:t></w:r><w:hyperlink w:anchor="MyMark"><w:r><w:t>Jump</w:t></w:r></w:hyperlink></w:p><w:sectPr/></w:body></w:document>"#
        );
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/_rels/document.xml.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.org/" TargetMode="External"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), document.into_bytes());
        let doc = RichDocument::decode(&package(&parts).unwrap(), Format::Docx).unwrap();
        assert_eq!(doc.state().bookmarks[0].name, "MyMark");
        let Block::Paragraph(p) = &doc.state().blocks[1] else {
            panic!()
        };
        assert!(p
            .runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://example.org/")));
        assert!(p.runs.iter().any(|r| r.link.as_deref() == Some("#MyMark")));
        let reopened =
            RichDocument::decode(&doc.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        assert_eq!(reopened.state().bookmarks[0].name, "MyMark");
        let Block::Paragraph(p) = &reopened.state().blocks[1] else {
            panic!()
        };
        assert!(p
            .runs
            .iter()
            .any(|r| r.link.as_deref() == Some("https://example.org/")));
    }
    #[test]
    fn external_word_footnote_and_separator_survive_resave() {
        let doc = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p><w:sectPr/></w:body></w:document>"#
        );
        let notes = format!(
            r#"<w:footnotes xmlns:w="{W}"><w:footnote w:type="separator" w:id="-1"><w:p><w:pPr><w:spacing w:after="100"/></w:pPr><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:footnoteRef/></w:r><w:r><w:t>Source note</w:t></w:r></w:p></w:footnote></w:footnotes>"#
        );
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/_rels/document.xml.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), doc.into_bytes());
        parts.insert("word/footnotes.xml".into(), notes.into_bytes());
        let doc = RichDocument::decode(&package(&parts).unwrap(), Format::Docx).unwrap();
        assert_eq!(doc.state().footnotes[0].at.offset, 5);
        assert_eq!(
            doc.state().footnotes[0].paragraphs[0].runs[0].text,
            "Source note"
        );
        let output = doc.encode(Format::Docx).unwrap();
        let reopened = RichDocument::decode(&output, Format::Docx).unwrap();
        assert_eq!(
            reopened.state().footnotes[0].paragraphs[0].runs[0].text,
            "Source note"
        );
        assert!(
            std::str::from_utf8(&reopened.state().docx_parts["word/footnotes.xml"])
                .unwrap()
                .contains("w:after=\"100\"")
        );
    }
    #[test]
    fn external_word_comment_range_and_metadata_survive_edit_and_resave() {
        let doc = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:t>Hello </w:t></w:r><w:commentRangeStart w:id="17"/><w:r><w:t>world</w:t></w:r><w:commentRangeEnd w:id="17"/><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:commentReference w:id="17"/></w:r></w:p><w:sectPr/></w:body></w:document>"#
        );
        let comments = format!(
            r#"<w:comments xmlns:w="{W}"><w:comment w:id="17" w:author="Ada" w:initials="AL" w:date="2026-01-02T03:04:05Z"><w:p><w:pPr><w:pStyle w:val="CommentText"/></w:pPr><w:r><w:rPr><w:rStyle w:val="CommentReference"/></w:rPr><w:annotationRef/></w:r><w:r><w:t>Review</w:t><w:tab/><w:t>this</w:t></w:r></w:p></w:comment></w:comments>"#
        );
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/_rels/document.xml.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), doc.into_bytes());
        parts.insert("word/comments.xml".into(), comments.into_bytes());
        let bytes = package(&parts).unwrap();
        let mut doc = RichDocument::decode(&bytes, Format::Docx).unwrap();
        assert_eq!(doc.state().comments[0].text, "Review\tthis");
        assert_eq!(
            doc.state().comments[0].date.as_deref(),
            Some("2026-01-02T03:04:05Z")
        );
        assert_eq!(doc.state().comments[0].initials.as_deref(), Some("AL"));
        let id = doc.state().comments[0].id;
        doc.edit_comment(id, "Updated".into(), false).unwrap();
        let reopened =
            RichDocument::decode(&doc.encode(Format::Docx).unwrap(), Format::Docx).unwrap();
        assert_eq!(reopened.state().comments[0].text, "Updated");
        assert_eq!(reopened.state().comments[0].initials.as_deref(), Some("AL"));
    }
    #[test]
    fn external_word_vertical_merge_is_reconstructed() {
        let xml = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:tbl><w:tblGrid><w:gridCol w:w="1200"/><w:gridCol w:w="1800"/></w:tblGrid><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>External</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl><w:sectPr/></w:body></w:document>"#
        );
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), xml.into_bytes());
        let state = decode(&package(&parts).unwrap()).unwrap();
        let Block::Table(t) = &state.blocks[0] else {
            panic!()
        };
        assert_eq!((t.rows[0][0].row_span, t.rows[0][0].col_span), (2, 2));
        assert_eq!(t.column_widths_twips, [1200, 1800]);
        assert_eq!(t.rows[0][0].paragraphs[0].runs[0].text, "External");
    }
    #[test]
    fn word_complex_page_fields_in_header_are_retained() {
        use std::io::Read;
        let mut state = State::default();
        state.sections[0].header.push(Paragraph {
            id: 2,
            style: ParagraphStyle::default(),
            runs: vec![Run {
                text: "placeholder".into(),
                style: TextStyle::default(),
                link: None,
            }],
        });
        state.next_id = 3;
        let bytes = encode(&state).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut parts = std::collections::BTreeMap::new();
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).unwrap();
            let name = file.name().to_owned();
            let mut data = Vec::new();
            file.read_to_end(&mut data).unwrap();
            parts.insert(name, data);
        }
        let header = format!(
            r#"<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>Page </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PAGE \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>7</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:hdr>"#
        );
        parts.insert("word/headerDOC420.xml".into(), header.into_bytes());
        let decoded = decode(&package(&parts).unwrap()).unwrap();
        assert_eq!(decoded.fields.len(), 1);
        assert_eq!(decoded.fields[0].kind, FieldKind::PageNumber);
        assert_eq!(decoded.fields[0].at.offset, 5);
        assert_eq!(decoded.sections[0].header[0].runs[0].text, "Page ");
        let roundtrip = decode(&encode(&decoded).unwrap()).unwrap();
        assert_eq!(roundtrip.fields.len(), 1);
        assert!(page_field("PAGE \\* ARABIC \\* MERGEFORMAT").is_ok());
        assert!(page_field("PAGE \\* ROMAN").is_err());
    }
    #[test]
    fn independently_authored_word_xml_with_metadata_opens() {
        let xml=br#"<?xml version="1.0"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="w14"><w:body><w:p w:rsidR="00112233"><w:r><w:t>Hello external</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:bottom="1440" w:left="1440" w:right="1440"/></w:sectPr></w:body></w:document>"#;
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("[Content_Types].xml".into(), br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec());
        parts.insert("_rels/.rels".into(), br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec());
        parts.insert("word/document.xml".into(), xml.to_vec());
        parts.insert("docProps/core.xml".into(), b"<core/>".to_vec());
        let state = decode(&package(&parts).unwrap()).unwrap();
        let Block::Paragraph(p) = &state.blocks[0] else {
            panic!()
        };
        assert_eq!(p.runs[0].text, "Hello external");
        assert!(state.docx_parts.contains_key("docProps/core.xml"));
    }
    #[test]
    fn inline_png_round_trip() {
        let mut state = State::default();
        state.blocks.push(Block::Image(Image {
            id: 2,
            mime: "image/png".into(),
            data: vec![
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0,
                1, 8, 6, 0, 0, 0, 31, 21, 196, 137, 0, 0, 0, 13, 73, 68, 65, 84, 120, 156, 99, 248,
                207, 192, 240, 31, 0, 5, 0, 1, 255, 137, 153, 61, 29, 0, 0, 0, 0, 73, 69, 78, 68,
                174, 66, 96, 130,
            ],
            width_twips: 1440,
            height_twips: 720,
            alt: "A picture".into(),
        }));
        state.next_id = 3;
        let reopened = decode(&encode(&state).unwrap()).unwrap();
        let Block::Image(p) = &reopened.blocks[1] else {
            panic!()
        };
        assert_eq!(p.width_twips, 1440);
        assert_eq!(p.height_twips, 720);
        assert_eq!(p.alt, "A picture");
        assert!(p.data.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert!(p.data.len() > 50);
        let next = decode(&encode(&reopened).unwrap()).unwrap();
        assert!(decode(&encode(&next).unwrap()).is_ok());
    }
    #[test]
    fn numbered_list_round_trip() {
        let mut state = State::default();
        let Block::Paragraph(p) = &mut state.blocks[0] else {
            panic!()
        };
        p.runs[0].text = "List item".into();
        p.style.list = Some(ListMarker {
            id: 7,
            level: 1,
            numbered: true,
        });
        let reopened = decode(&encode(&state).unwrap()).unwrap();
        let Block::Paragraph(p) = &reopened.blocks[0] else {
            panic!()
        };
        assert_eq!(
            p.style.list,
            Some(ListMarker {
                id: 7,
                level: 1,
                numbered: true
            })
        );
        assert!(decode(&encode(&reopened).unwrap()).is_ok());
    }
    #[test]
    fn theme_color_reference_survives_projection() {
        let mut state = State::default();
        let Block::Paragraph(p) = &mut state.blocks[0] else {
            panic!()
        };
        p.runs[0].text = "theme".into();
        p.runs[0].style.color = Some(0x336699);
        p.runs[0].style.theme_color = Some("accent1".into());
        p.runs[0].style.theme_tint = Some("80".into());
        let mut doc = RichDocument::decode(&encode(&state).unwrap(), Format::Docx).unwrap();
        let projection = doc.rich_edit_projection().unwrap();
        doc.apply_rich_edit_projection(&projection, (0, 0)).unwrap();
        let Block::Paragraph(p) = &doc.state().blocks[0] else {
            panic!()
        };
        assert_eq!(p.runs[0].style.theme_color.as_deref(), Some("accent1"));
        assert_eq!(p.runs[0].style.theme_tint.as_deref(), Some("80"));
        assert_eq!(p.runs[0].style.color, Some(0x336699));
    }
    #[test]
    fn inherited_styles_and_theme_fonts_round_trip() {
        let xml=br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading"/></w:pPr><w:r><w:rPr><w:rStyle w:val="Emphasis"/></w:rPr><w:t>Styled</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#;
        let styles=br#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults><w:style w:type="paragraph" w:styleId="Base"><w:rPr><w:b/></w:rPr></w:style><w:style w:type="paragraph" w:styleId="Heading"><w:basedOn w:val="Base"/><w:pPr><w:jc w:val="center"/></w:pPr></w:style><w:style w:type="character" w:styleId="Emphasis"><w:rPr><w:i/></w:rPr></w:style></w:styles>"#;
        let theme=br#"<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><a:themeElements><a:fontScheme><a:majorFont><a:latin typeface="Cambria"/></a:majorFont><a:minorFont><a:latin typeface="Calibri"/></a:minorFont></a:fontScheme></a:themeElements></a:theme>"#;
        let mut parts = std::collections::BTreeMap::new();
        parts.insert("word/document.xml".into(), xml.to_vec());
        parts.insert("word/styles.xml".into(), styles.to_vec());
        parts.insert("word/theme/theme1.xml".into(), theme.to_vec());
        let state = decode(&package(&parts).unwrap()).unwrap();
        let Block::Paragraph(p) = &state.blocks[0] else {
            panic!()
        };
        assert_eq!(p.style.source_style.as_deref(), Some("Heading"));
        assert_eq!(p.style.alignment, Alignment::Center);
        assert_eq!(p.runs[0].style.font, "Calibri");
        assert_eq!(p.runs[0].style.size_half_points, 22);
        assert!(p.runs[0].style.bold && p.runs[0].style.italic);
        assert_eq!(p.runs[0].style.source_style.as_deref(), Some("Emphasis"));
        let reopened = decode(&encode(&state).unwrap()).unwrap();
        let Block::Paragraph(p) = &reopened.blocks[0] else {
            panic!()
        };
        assert_eq!(p.style.source_style.as_deref(), Some("Heading"));
        assert_eq!(p.runs[0].style.source_style.as_deref(), Some("Emphasis"));
        assert!(p.runs[0].style.bold && p.runs[0].style.italic);
    }
    #[test]
    fn simple_table_round_trip() {
        let mut s = State::default();
        let p = Paragraph {
            id: 3,
            style: ParagraphStyle::default(),
            runs: vec![Run {
                text: "cell".into(),
                style: TextStyle::default(),
                link: None,
            }],
        };
        s.blocks.push(Block::Table(Table {
            column_widths_twips: Vec::new(),
            id: 2,
            rows: vec![vec![Cell {
                paragraphs: vec![p],
                row_span: 1,
                col_span: 1,
                border: None,
                fill: None,
            }]],
        }));
        s.next_id = 4;
        let decoded = decode(&encode(&s).unwrap()).unwrap();
        let Block::Table(t) = &decoded.blocks[1] else {
            panic!()
        };
        assert_eq!(t.rows[0][0].paragraphs[0].runs[0].text, "cell");
    }
    #[test]
    fn styled_document_and_preserved_parts_round_trip() {
        let mut s = State::default();
        if let Block::Paragraph(p) = &mut s.blocks[0] {
            p.runs[0].text = "Hello & 世界".into();
            p.runs[0].style.bold = true;
            p.runs[0].style.color = Some(0x336699);
            p.style.alignment = Alignment::Center;
        }
        s.docx_parts
            .insert("docProps/core.xml".into(), b"<core/>".to_vec());
        let bytes = encode(&s).unwrap();
        let decoded = decode(&bytes).unwrap();
        assert_eq!(
            decoded.docx_parts.get("docProps/core.xml").unwrap(),
            b"<core/>"
        );
        let (Block::Paragraph(a), Block::Paragraph(b)) = (&s.blocks[0], &decoded.blocks[0]) else {
            panic!()
        };
        assert_eq!(a.runs, b.runs);
        assert_eq!(a.style, b.style);
        assert_eq!(s.sections[0].page, decoded.sections[0].page);
        assert!(decode(&encode(&decoded).unwrap()).is_ok());
    }
}
