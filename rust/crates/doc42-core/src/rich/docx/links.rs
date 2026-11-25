//! DOCX hyperlink relationship support.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
pub(super) fn relation_id(target: &str) -> String {
    let digest = Sha256::digest(target.as_bytes());
    let mut id = String::from("rIdDOC42Link");
    for byte in digest {
        id.push_str(&format!("{byte:02x}"));
    }
    id
}
pub(super) fn write_rels(parts: &mut BTreeMap<String, Vec<u8>>, s: &State) -> Result<(), Error> {
    let mut targets = BTreeSet::new();
    for p in s.blocks.iter().flat_map(|b| match b {
        Block::Paragraph(p) => vec![p],
        Block::Table(t) => t
            .rows
            .iter()
            .flatten()
            .flat_map(|c| c.paragraphs.iter())
            .collect(),
        _ => Vec::new(),
    }) {
        for link in p.runs.iter().filter_map(|r| r.link.as_deref()) {
            if !link.starts_with('#') {
                targets.insert(link.to_owned());
            }
        }
    }
    if targets.is_empty() {
        return Ok(());
    }
    let path = "word/_rels/document.xml.rels";
    let mut xml = String::from_utf8(parts.get(path).cloned().unwrap_or_else(|| b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec())).map_err(bad)?;
    for target in targets {
        let id = relation_id(&target);
        let doc = roxmltree::Document::parse(&xml).map_err(bad)?;
        if let Some(r) = doc
            .descendants()
            .find(|n| n.attribute("Id") == Some(id.as_str()))
        {
            if r.attribute("Target") != Some(target.as_str())
                || r.attribute("TargetMode") != Some("External")
                || !r.attribute("Type").unwrap_or("").ends_with("/hyperlink")
            {
                return Err(Error::Unsupported("DOCX relationship id collision".into()));
            }
            continue;
        }
        let close = xml.rfind("</").ok_or_else(|| bad("DOCX relationships"))?;
        xml.insert_str(close, &format!("<Relationship Id=\"{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink\" Target=\"{}\" TargetMode=\"External\"/>", esc(&target)));
    }
    parts.insert(path.into(), xml.into_bytes());
    Ok(())
}
#[derive(Default)]
pub(super) struct Scan {
    pub links: Vec<(usize, usize, usize, String)>,
    pub bookmarks: Vec<(String, usize, usize, usize, usize)>,
}
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
fn visible(n: roxmltree::Node<'_, '_>) -> usize {
    n.descendants()
        .filter_map(|c| {
            if c.has_tag_name((W, "t")) {
                Some(c.text().unwrap_or("").len())
            } else if c.has_tag_name((W, "tab")) {
                Some(1)
            } else {
                None
            }
        })
        .sum()
}
pub(super) fn normalize(
    xml: &str,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<(String, Scan), Error> {
    let doc = roxmltree::Document::parse(xml).map_err(bad)?;
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name((W, "body")))
        .ok_or_else(|| bad("Word body"))?;
    let mut rels = BTreeMap::new();
    if let Some(bytes) = parts.get("word/_rels/document.xml.rels") {
        let x = std::str::from_utf8(bytes).map_err(bad)?;
        let d = roxmltree::Document::parse(x).map_err(bad)?;
        for n in d.descendants().filter(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && n.attribute("Type").unwrap_or("").ends_with("/hyperlink")
        }) {
            if n.attribute("TargetMode") != Some("External") {
                return Err(Error::Unsupported(
                    "internal DOCX hyperlink relationship".into(),
                ));
            }
            let id = n
                .attribute("Id")
                .ok_or_else(|| bad("link relationship id"))?;
            let target = n.attribute("Target").ok_or_else(|| bad("link target"))?;
            if rels.insert(id.to_owned(), target.to_owned()).is_some() {
                return Err(bad("duplicate link relationship"));
            }
        }
    }
    let mut ranges = Vec::new();
    let mut links = Vec::new();
    let mut starts: BTreeMap<String, (String, usize, usize)> = BTreeMap::new();
    let mut ends: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for p in body.descendants().filter(|n| n.has_tag_name((W, "p"))) {
        let paragraph = p.range().start;
        let mut offset = 0usize;
        let mut has_anchor = false;
        for child in p.children().filter(|n| n.is_element()) {
            if child.has_tag_name((W, "hyperlink")) {
                has_anchor = true;
                if child.attributes().any(|a| {
                    !((a.namespace() == Some(W)
                        && (a.name() == "anchor"
                            || (a.name() == "history"
                                && matches!(a.value(), "1" | "true" | "0" | "false"))))
                        || (a.namespace() == Some(REL) && a.name() == "id"))
                }) {
                    return Err(Error::Unsupported("DOCX hyperlink attributes".into()));
                }
                if child
                    .children()
                    .filter(|n| n.is_element())
                    .any(|n| !n.has_tag_name((W, "r")))
                {
                    return Err(Error::Unsupported("DOCX hyperlink content".into()));
                }
                let target = match (val(child, "anchor"), child.attribute((REL, "id"))) {
                    (Some(anchor), None) => format!("#{anchor}"),
                    (None, Some(id)) => rels
                        .get(id)
                        .ok_or_else(|| bad("missing link relationship"))?
                        .clone(),
                    _ => return Err(Error::Unsupported("DOCX hyperlink target".into())),
                };
                let len = visible(child);
                if len == 0 {
                    return Err(Error::Unsupported("empty DOCX hyperlink".into()));
                }
                links.push((paragraph, offset, offset + len, target));
                offset += len;
                let r = child.range();
                let tag = &xml[r.clone()];
                let open = tag.find('>').ok_or_else(|| bad("hyperlink tag"))? + 1;
                let close = tag.rfind('<').ok_or_else(|| bad("hyperlink tag"))?;
                ranges.push(r.start..r.start + open);
                ranges.push(r.start + close..r.end);
            } else if child.has_tag_name((W, "bookmarkStart")) {
                has_anchor = true;
                if child
                    .attributes()
                    .any(|a| a.namespace() != Some(W) || !["id", "name"].contains(&a.name()))
                {
                    return Err(Error::Unsupported("DOCX bookmark attributes".into()));
                }
                let id = val(child, "id")
                    .ok_or_else(|| bad("bookmark id"))?
                    .to_owned();
                let name = val(child, "name")
                    .ok_or_else(|| bad("bookmark name"))?
                    .to_owned();
                if starts.insert(id, (name, paragraph, offset)).is_some() {
                    return Err(bad("duplicate bookmark id"));
                }
                ranges.push(child.range());
            } else if child.has_tag_name((W, "bookmarkEnd")) {
                has_anchor = true;
                if child
                    .attributes()
                    .any(|a| a.namespace() != Some(W) || a.name() != "id")
                {
                    return Err(Error::Unsupported("DOCX bookmark end attributes".into()));
                }
                let id = val(child, "id")
                    .ok_or_else(|| bad("bookmark id"))?
                    .to_owned();
                if ends.insert(id, (paragraph, offset)).is_some() {
                    return Err(bad("duplicate bookmark end"));
                }
                ranges.push(child.range());
            } else if child.has_tag_name((W, "r")) {
                offset += visible(child);
            }
        }
        if has_anchor
            && p.descendants()
                .any(|n| n.has_tag_name((W, "fldChar")) || n.has_tag_name((W, "fldSimple")))
        {
            return Err(Error::Unsupported(
                "DOCX links or bookmarks mixed with fields".into(),
            ));
        }
    }
    let mut bookmarks = Vec::new();
    for (id, (name, paragraph, offset)) in starts {
        let (end_para, end_offset) = ends.remove(&id).ok_or_else(|| bad("unclosed bookmark"))?;
        bookmarks.push((name, paragraph, offset, end_para, end_offset));
    }
    if !ends.is_empty() {
        return Err(bad("orphan bookmark end"));
    }
    ranges.sort_by_key(|r| r.start);
    if ranges.windows(2).any(|w| w[0].end > w[1].start) {
        return Err(bad("overlapping link markup"));
    }
    let mut removed = Vec::with_capacity(ranges.len() + 1);
    removed.push(0usize);
    for range in &ranges {
        removed.push(removed.last().copied().unwrap() + range.end - range.start);
    }
    let remap = |position: usize| {
        let count = ranges.partition_point(|r| r.end <= position);
        position - removed[count]
    };
    for link in &mut links {
        link.0 = remap(link.0);
    }
    for bookmark in &mut bookmarks {
        bookmark.1 = remap(bookmark.1);
        bookmark.3 = remap(bookmark.3);
    }
    let mut output = String::with_capacity(xml.len() - removed.last().copied().unwrap_or(0));
    let mut cursor = 0;
    for range in ranges {
        output.push_str(&xml[cursor..range.start]);
        cursor = range.end;
    }
    output.push_str(&xml[cursor..]);
    Ok((output, Scan { links, bookmarks }))
}
