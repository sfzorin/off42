//! Classic Word comments and document relationships.
use super::*;
use std::collections::BTreeMap;
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const CT: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
fn append(xml: &mut String, text: &str) -> Result<(), Error> {
    let i = xml
        .rfind("</")
        .ok_or_else(|| bad("invalid DOCX package XML"))?;
    xml.insert_str(i, text);
    Ok(())
}
pub(super) fn write(
    parts: &mut BTreeMap<String, Vec<u8>>,
    comments: &[Annotation],
) -> Result<(), Error> {
    let mut rels = String::from_utf8(parts.get("word/_rels/document.xml.rels").cloned().unwrap_or_else(|| b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec())).map_err(bad)?;
    let doc = roxmltree::Document::parse(&rels).map_err(bad)?;
    let relationships = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && n.attribute("Type")
                    .is_some_and(|v| v.ends_with("/comments"))
        })
        .collect::<Vec<_>>();
    if relationships.len() > 1 {
        return Err(Error::Unsupported("multiple DOCX comments parts".into()));
    }
    let existing = relationships
        .first()
        .map(|n| n.attribute("Target").unwrap_or("").to_owned());
    if comments.is_empty() && existing.is_none() {
        return Ok(());
    }
    let target = existing.unwrap_or_else(|| "commentsDOC42.xml".into());
    if target.is_empty()
        || target.contains('/')
        || target.contains('\\')
        || target.contains(':')
        || target.contains("..")
    {
        return Err(Error::Unsupported("DOCX comments target".into()));
    }
    let path = format!("word/{target}");
    if relationships.is_empty() {
        let mut n = 0u32;
        let rid = loop {
            let id = format!("rIdDOC42Comments{n}");
            if !rels.contains(&format!("Id=\"{id}\"")) {
                break id;
            }
            n = n.checked_add(1).ok_or(Error::Limit)?;
        };
        append(
            &mut rels,
            &format!(
                "<Relationship xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\" Id=\"{rid}\" Type=\"{R}/comments\" Target=\"{}\"/>",
                esc(&target)
            ),
        )?;
    }
    let mut ct = String::from_utf8(
        parts
            .get("[Content_Types].xml")
            .cloned()
            .ok_or_else(|| bad("missing content types"))?,
    )
    .map_err(bad)?;
    let content_types = roxmltree::Document::parse(&ct).map_err(bad)?;
    let part_name = format!("/{path}");
    if !content_types.descendants().any(|n| {
        n.is_element()
            && n.tag_name().name() == "Override"
            && n.attribute("PartName") == Some(part_name.as_str())
    }) {
        append(&mut ct, &format!("<Override xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\" PartName=\"/{path}\" ContentType=\"{CT}\"/>"))?;
    }
    let mut xml = format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:comments xmlns:w=\"{W}\">");
    for c in comments {
        if c.resolved {
            return Err(Error::Unsupported("resolved DOCX comments".into()));
        }
        if c.id > i32::MAX as u64
            || c.text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
        {
            return Err(Error::Unsupported("DOCX comment content".into()));
        }
        let date = c
            .date
            .as_ref()
            .map(|v| format!(" w:date=\"{}\"", esc(v)))
            .unwrap_or_default();
        let initials = c
            .initials
            .as_ref()
            .map(|v| format!(" w:initials=\"{}\"", esc(v)))
            .unwrap_or_default();
        xml.push_str(&format!(
            "<w:comment w:id=\"{}\" w:author=\"{}\"{date}{initials}>",
            c.id,
            esc(&c.author)
        ));
        for paragraph in c.text.split('\n') {
            xml.push_str("<w:p><w:r>");
            for (i, part) in paragraph.split('\t').enumerate() {
                if i != 0 {
                    xml.push_str("<w:tab/>");
                }
                xml.push_str(&format!(
                    "<w:t xml:space=\"preserve\">{}</w:t>",
                    esc(part.trim_end_matches('\r'))
                ));
            }
            xml.push_str("</w:r></w:p>");
        }
        xml.push_str("</w:comment>");
    }
    xml.push_str("</w:comments>");
    parts.insert(path, xml.into_bytes());
    parts.insert("word/_rels/document.xml.rels".into(), rels.into_bytes());
    parts.insert("[Content_Types].xml".into(), ct.into_bytes());
    Ok(())
}

#[derive(Clone)]
pub(super) struct CommentData {
    pub author: String,
    pub text: String,
    pub date: Option<String>,
    pub initials: Option<String>,
}
pub(super) fn read(parts: &BTreeMap<String, Vec<u8>>) -> Result<BTreeMap<u64, CommentData>, Error> {
    let Some(rels) = parts.get("word/_rels/document.xml.rels") else {
        return Ok(BTreeMap::new());
    };
    let rels = std::str::from_utf8(rels).map_err(bad)?;
    let doc = roxmltree::Document::parse(rels).map_err(bad)?;
    let relationships = doc
        .descendants()
        .filter(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && n.attribute("Type")
                    .is_some_and(|v| v.ends_with("/comments"))
        })
        .collect::<Vec<_>>();
    if relationships.len() > 1 {
        return Err(Error::Unsupported("multiple DOCX comments parts".into()));
    }
    let Some(rel) = relationships.first() else {
        return Ok(BTreeMap::new());
    };
    if rel.attribute("TargetMode").is_some() {
        return Err(Error::Unsupported("external DOCX comments".into()));
    }
    let target = rel
        .attribute("Target")
        .ok_or_else(|| bad("missing comments target"))?;
    if target.is_empty()
        || target.contains('/')
        || target.contains('\\')
        || target.contains(':')
        || target.contains("..")
    {
        return Err(Error::Unsupported("DOCX comments target".into()));
    }
    let bytes = parts
        .get(&format!("word/{target}"))
        .ok_or_else(|| bad("missing comments part"))?;
    let xml = std::str::from_utf8(bytes).map_err(bad)?;
    let document = roxmltree::Document::parse(xml).map_err(bad)?;
    let root = document.root_element();
    if !root.has_tag_name((W, "comments")) {
        return Err(bad("invalid comments part"));
    }
    allowed(root, &["comment"])?;
    let mut result = BTreeMap::new();
    for node in root.children().filter(|n| n.is_element()) {
        for a in node.attributes() {
            if a.namespace() != Some(W) || !["id", "author", "date", "initials"].contains(&a.name())
            {
                return Err(Error::Unsupported("DOCX comment metadata".into()));
            }
        }
        let id = val(node, "id")
            .ok_or_else(|| bad("comment id"))?
            .parse::<u64>()
            .map_err(bad)?;
        let author = val(node, "author").unwrap_or("").to_owned();
        allowed(node, &["p"])?;
        let mut paragraphs = Vec::new();
        for p in node.children().filter(|n| n.is_element()) {
            allowed(p, &["pPr", "r"])?;
            if let Some(properties) = p.children().find(|n| n.has_tag_name((W, "pPr"))) {
                allowed(properties, &["pStyle"])?;
                if properties
                    .children()
                    .any(|n| n.has_tag_name((W, "pStyle")) && val(n, "val") != Some("CommentText"))
                {
                    return Err(Error::Unsupported("formatted DOCX comment".into()));
                }
            }
            let mut text = String::new();
            for run in p.children().filter(|n| n.has_tag_name((W, "r"))) {
                allowed(run, &["rPr", "t", "tab", "br", "annotationRef"])?;
                for child in run.children().filter(|n| n.is_element()) {
                    match child.tag_name().name() {
                        "rPr" => {
                            allowed(child, &["rStyle", "noProof"])?;
                            if child.children().any(|n| {
                                n.has_tag_name((W, "rStyle"))
                                    && val(n, "val") != Some("CommentReference")
                            }) {
                                return Err(Error::Unsupported("formatted DOCX comment".into()));
                            }
                        }
                        "t" => text.push_str(child.text().unwrap_or("")),
                        "tab" => text.push('\t'),
                        "br" => text.push('\n'),
                        "annotationRef" => {}
                        _ => unreachable!(),
                    }
                }
            }
            paragraphs.push(text);
        }
        let item = CommentData {
            author,
            text: paragraphs.join("\n"),
            date: val(node, "date").map(str::to_owned),
            initials: val(node, "initials").map(str::to_owned),
        };
        if result.insert(id, item).is_some() {
            return Err(bad("duplicate comment id"));
        }
    }
    Ok(result)
}
