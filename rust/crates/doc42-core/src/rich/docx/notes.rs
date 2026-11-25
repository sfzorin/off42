//! Word footnote/endnote parts and relationships.
use super::*;
use std::collections::BTreeMap;
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
fn append(xml: &mut String, text: &str) -> Result<(), Error> {
    let at = xml
        .rfind("</")
        .ok_or_else(|| bad("invalid DOCX package XML"))?;
    xml.insert_str(at, text);
    Ok(())
}
fn target(parts: &BTreeMap<String, Vec<u8>>, kind: &str) -> Result<Option<String>, Error> {
    let Some(bytes) = parts.get("word/_rels/document.xml.rels") else {
        return Ok(None);
    };
    let text = std::str::from_utf8(bytes).map_err(bad)?;
    let doc = roxmltree::Document::parse(text).map_err(bad)?;
    let suffix = format!("/{kind}s");
    let relations = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && n.attribute("Type").is_some_and(|t| t.ends_with(&suffix))
        })
        .collect::<Vec<_>>();
    if relations.len() > 1 {
        return Err(Error::Unsupported("multiple DOCX note parts".into()));
    }
    let Some(rel) = relations.first() else {
        return Ok(None);
    };
    if rel.attribute("TargetMode").is_some() {
        return Err(Error::Unsupported("external DOCX note part".into()));
    }
    let name = rel
        .attribute("Target")
        .ok_or_else(|| bad("missing note target"))?;
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains(':')
        || name.contains("..")
    {
        return Err(Error::Unsupported("DOCX note target".into()));
    }
    Ok(Some(name.to_owned()))
}
fn note_shell(
    parts: &BTreeMap<String, Vec<u8>>,
    path: &str,
    kind: &str,
) -> Result<(String, Vec<String>, String), Error> {
    let Some(bytes) = parts.get(path) else {
        return Ok((format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:{kind}s xmlns:w=\"{W}\">"), vec![format!("<w:{kind} w:type=\"separator\" w:id=\"-1\"><w:p><w:r><w:separator/></w:r></w:p></w:{kind}>"),format!("<w:{kind} w:type=\"continuationSeparator\" w:id=\"0\"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{kind}>")],format!("</w:{kind}s>")));
    };
    let xml = std::str::from_utf8(bytes).map_err(bad)?;
    let doc = roxmltree::Document::parse(xml).map_err(bad)?;
    let root = doc.root_element();
    let expected = format!("{kind}s");
    if !root.has_tag_name((W, expected.as_str())) {
        return Err(bad("invalid DOCX note root"));
    }
    if root
        .lookup_namespace_uri(Some("w"))
        .is_some_and(|uri| uri != W)
    {
        return Err(Error::Unsupported("DOCX note namespace conflict".into()));
    }
    let source = &xml[root.range()];
    let mut quote = None;
    let mut end = None;
    for (i, c) in source.char_indices() {
        match (quote, c) {
            (Some(q), x) if q == x => quote = None,
            (None, '\'' | '"') => quote = Some(c),
            (None, '>') => {
                end = Some(i);
                break;
            }
            _ => {}
        }
    }
    let end = end.ok_or_else(|| bad("invalid DOCX note root"))?;
    let mut open = source[..end].to_owned();
    if open.ends_with('/') {
        open.pop();
    }
    if root.lookup_namespace_uri(Some("w")).is_none() {
        open.push_str(&format!(" xmlns:w=\"{W}\""));
    }
    open.push('>');
    let tag_end = source
        .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
        .ok_or_else(|| bad("invalid note root"))?;
    let tag = &source[1..tag_end];
    let mut special = Vec::new();
    for node in root.children().filter(|n| n.is_element()) {
        if let Some(typ) = val(node, "type") {
            if !matches!(typ, "separator" | "continuationSeparator") {
                return Err(Error::Unsupported("DOCX note separator type".into()));
            }
            special.push(xml[node.range()].to_owned());
        }
    }
    Ok((open, special, format!("</{tag}>")))
}
pub(super) fn write(
    parts: &mut BTreeMap<String, Vec<u8>>,
    notes: &[Footnote],
) -> Result<(), Error> {
    for (endnote, kind) in [(false, "footnote"), (true, "endnote")] {
        let selected: Vec<_> = notes.iter().filter(|n| n.endnote == endnote).collect();
        let existing = target(parts, kind)?;
        if selected.is_empty() && existing.is_none() {
            continue;
        }
        let name = existing.clone().unwrap_or_else(|| format!("{kind}s.xml"));
        let path = format!("word/{name}");
        let mut rels = String::from_utf8(parts.get("word/_rels/document.xml.rels").cloned().unwrap_or_else(|| b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec())).map_err(bad)?;
        if existing.is_none() {
            let mut n = 0u32;
            let rid = loop {
                let id = format!("rIdDOC42{kind}{n}");
                if !rels.contains(&format!("Id=\"{id}\"")) {
                    break id;
                }
                n = n.checked_add(1).ok_or(Error::Limit)?;
            };
            append(&mut rels,&format!("<Relationship xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\" Id=\"{rid}\" Type=\"{R}/{kind}s\" Target=\"{}\"/>",esc(&name)))?;
        }
        let mut types = String::from_utf8(
            parts
                .get("[Content_Types].xml")
                .cloned()
                .ok_or_else(|| bad("missing content types"))?,
        )
        .map_err(bad)?;
        let doc = roxmltree::Document::parse(&types).map_err(bad)?;
        let part_name = format!("/{path}");
        if !doc.descendants().any(|n| {
            n.is_element()
                && n.tag_name().name() == "Override"
                && n.attribute("PartName") == Some(part_name.as_str())
        }) {
            append(&mut types,&format!("<Override xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\" PartName=\"/{path}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}s+xml\"/>"))?;
        }
        let (mut xml, special, close) = note_shell(parts, &path, kind)?;
        for value in special {
            xml.push_str(&value);
        }
        for (index, note) in selected.iter().enumerate() {
            if note.id > i32::MAX as u64
                || note
                    .citation
                    .as_ref()
                    .is_some_and(|v| v != &(index + 1).to_string())
                || note.paragraphs.is_empty()
            {
                return Err(Error::Unsupported(
                    "DOCX custom note citation or content".into(),
                ));
            }
            xml.push_str(&format!("<w:{kind} w:id=\"{}\">", note.id));
            for (i, paragraph) in note.paragraphs.iter().enumerate() {
                let mut p = String::new();
                write_paragraph(&mut p, paragraph, &[], &[], &[], &[])?;
                if i == 0 {
                    let at =
                        p.find("</w:pPr>").ok_or_else(|| bad("note paragraph"))? + "</w:pPr>".len();
                    p.insert_str(at, &format!("<w:r><w:{kind}Ref/></w:r>"));
                }
                xml.push_str(&p);
            }
            xml.push_str(&format!("</w:{kind}>"));
        }
        xml.push_str(&close);
        parts.insert(path, xml.into_bytes());
        parts.insert("word/_rels/document.xml.rels".into(), rels.into_bytes());
        parts.insert("[Content_Types].xml".into(), types.into_bytes());
    }
    Ok(())
}
pub(super) fn read(
    parts: &BTreeMap<String, Vec<u8>>,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
    next_id: &mut u64,
) -> Result<BTreeMap<(bool, u64), Vec<Paragraph>>, Error> {
    let mut result = BTreeMap::new();
    for (endnote, kind) in [(false, "footnote"), (true, "endnote")] {
        let Some(name) = target(parts, kind)? else {
            continue;
        };
        let bytes = parts
            .get(&format!("word/{name}"))
            .ok_or_else(|| bad("missing DOCX note part"))?;
        let xml = std::str::from_utf8(bytes).map_err(bad)?;
        let document = roxmltree::Document::parse(xml).map_err(bad)?;
        let root = document.root_element();
        let root_name = format!("{kind}s");
        if !root.has_tag_name((W, root_name.as_str())) {
            return Err(bad("invalid DOCX note part"));
        }
        allowed(root, &[kind])?;
        for node in root.children().filter(|n| n.is_element()) {
            let raw = val(node, "id").ok_or_else(|| bad("note id"))?;
            if let Some(typ) = val(node, "type") {
                if !matches!(typ, "separator" | "continuationSeparator") {
                    return Err(Error::Unsupported("DOCX note separator type".into()));
                }
                continue;
            }
            for attr in node.attributes() {
                if attr.namespace() != Some(W) || attr.name() != "id" {
                    return Err(Error::Unsupported("DOCX note metadata".into()));
                }
            }
            let external = raw.parse::<u64>().map_err(bad)?;
            allowed(node, &["p"])?;
            let mut paragraphs = Vec::new();
            for p in node.children().filter(|n| n.is_element()) {
                let id = *next_id;
                *next_id = next_id.checked_add(1).ok_or(Error::Limit)?;
                let mut fields = Vec::new();
                let mut comments = Vec::new();
                let mut nested_notes = Vec::new();
                paragraphs.push(read_paragraph(
                    p,
                    id,
                    styles,
                    numbering,
                    &mut fields,
                    &mut comments,
                    &mut nested_notes,
                )?);
                if !fields.is_empty() || !comments.is_empty() || !nested_notes.is_empty() {
                    return Err(Error::Unsupported("nested DOCX note content".into()));
                }
            }
            if paragraphs.is_empty() || result.insert((endnote, external), paragraphs).is_some() {
                return Err(bad("empty or duplicate DOCX note"));
            }
        }
    }
    Ok(result)
}
