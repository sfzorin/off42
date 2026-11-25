//! Header/footer parts and their standard WordprocessingML relationships.
use super::*;
use std::collections::BTreeMap;
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
fn append(xml: &mut String, text: &str) -> Result<(), Error> {
    let i = xml
        .rfind("</")
        .ok_or_else(|| bad("invalid DOCX package XML"))?;
    xml.insert_str(i, text);
    Ok(())
}
pub(super) fn write(
    parts: &mut BTreeMap<String, Vec<u8>>,
    section: &Section,
    fields: &[Field],
) -> Result<String, Error> {
    if section.header.is_empty() && section.footer.is_empty() {
        return Ok(String::new());
    }
    let mut ct=String::from_utf8(parts.get("[Content_Types].xml").cloned().unwrap_or_else(||b"<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/></Types>".to_vec())).map_err(bad)?;
    let mut rels=String::from_utf8(parts.get("word/_rels/document.xml.rels").cloned().unwrap_or_else(||b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec())).map_err(bad)?;
    let mut refs = String::new();
    for (kind, paras) in [("header", &section.header), ("footer", &section.footer)] {
        if paras.is_empty() {
            continue;
        }
        let mut n = 0u64;
        let (name, rid) = loop {
            let file = format!("word/{kind}DOC42{n}.xml");
            let rid = format!("rIdDOC42{kind}{n}");
            if !parts.contains_key(&file) && !rels.contains(&format!("Id=\"{rid}\"")) {
                break (file, rid);
            }
            n = n.checked_add(1).ok_or(Error::Limit)?
        };
        let root = if kind == "header" { "hdr" } else { "ftr" };
        let mut xml = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:{root} xmlns:w=\"{W}\" xmlns:r=\"{R}\">"
        );
        for p in paras {
            write_paragraph(&mut xml, p, fields, &[], &[], &[])?;
        }
        xml.push_str(&format!("</w:{root}>"));
        parts.insert(name.clone(), xml.into_bytes());
        append(
            &mut rels,
            &format!(
                "<Relationship Id=\"{rid}\" Type=\"{R}/{kind}\" Target=\"{}\"/>",
                name.trim_start_matches("word/")
            ),
        )?;
        append(&mut ct,&format!("<Override PartName=\"/{name}\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.{kind}+xml\"/>"))?;
        refs.push_str(&format!(
            "<w:{kind}Reference w:type=\"default\" r:id=\"{rid}\"/>"
        ));
    }
    parts.insert("word/_rels/document.xml.rels".into(), rels.into_bytes());
    parts.insert("[Content_Types].xml".into(), ct.into_bytes());
    Ok(refs)
}
pub(super) fn read(
    parts: &BTreeMap<String, Vec<u8>>,
    id: &str,
    footer: bool,
) -> Result<Vec<u8>, Error> {
    let bytes = parts
        .get("word/_rels/document.xml.rels")
        .ok_or_else(|| bad("missing header relationships"))?;
    let text = std::str::from_utf8(bytes).map_err(bad)?;
    let rels = roxmltree::Document::parse(text).map_err(bad)?;
    let rel = rels
        .descendants()
        .find(|n| {
            n.is_element() && n.tag_name().name() == "Relationship" && n.attribute("Id") == Some(id)
        })
        .ok_or_else(|| bad("missing header relation"))?;
    let kind = if footer { "/footer" } else { "/header" };
    if !rel.attribute("Type").is_some_and(|t| t.ends_with(kind))
        || rel.attribute("TargetMode").is_some()
    {
        return Err(Error::Unsupported("DOCX header relation".into()));
    }
    let target = rel
        .attribute("Target")
        .ok_or_else(|| bad("missing header target"))?;
    if target.contains("..") || target.starts_with('/') || target.contains(':') {
        return Err(Error::Unsupported("DOCX header target".into()));
    }
    parts
        .get(&format!("word/{target}"))
        .cloned()
        .ok_or_else(|| bad("missing header part"))
}
