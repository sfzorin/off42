//! The Word numbering relationship maps list instances to abstract level definitions.
use super::*;
use std::collections::{BTreeMap, HashMap};
#[derive(Default)]
pub(super) struct Numbering {
    items: HashMap<(u64, u8), bool>,
    abstracts: std::collections::HashSet<u64>,
}
impl Numbering {
    pub(super) fn read(parts: &BTreeMap<String, Vec<u8>>) -> Result<Self, Error> {
        let Some(bytes) = parts.get("word/numbering.xml") else {
            return Ok(Self::default());
        };
        let text = std::str::from_utf8(bytes).map_err(bad)?;
        let doc = roxmltree::Document::parse(text).map_err(bad)?;
        let mut abstracts: HashMap<u64, Vec<(u8, bool)>> = HashMap::new();
        for a in doc
            .descendants()
            .filter(|n| n.has_tag_name((W, "abstractNum")))
        {
            let id = num(a, "abstractNumId", 0u64)?;
            let mut levels = Vec::new();
            for lvl in a.children().filter(|n| n.has_tag_name((W, "lvl"))) {
                let level = num(lvl, "ilvl", 0u8)?;
                let fmt = lvl
                    .children()
                    .find(|n| n.has_tag_name((W, "numFmt")))
                    .and_then(|n| val(n, "val"))
                    .unwrap_or("decimal");
                levels.push((level, fmt != "bullet"));
            }
            abstracts.insert(id, levels);
        }
        let mut result = Self::default();
        result.abstracts.extend(abstracts.keys().copied());
        for n in doc.descendants().filter(|n| n.has_tag_name((W, "num"))) {
            let id = num(n, "numId", 0u64)?;
            let abstract_id = n
                .children()
                .find(|x| x.has_tag_name((W, "abstractNumId")))
                .ok_or_else(|| bad("numbering missing abstract id"))?;
            let aid = num(abstract_id, "val", 0u64)?;
            let levels = abstracts
                .get(&aid)
                .ok_or_else(|| bad("missing abstract numbering"))?;
            for &(level, numbered) in levels {
                result.items.insert((id, level), numbered);
            }
        }
        Ok(result)
    }
    pub(super) fn marker(&self, id: u64, level: u8) -> Result<ListMarker, Error> {
        let numbered = *self
            .items
            .get(&(id, level))
            .ok_or_else(|| Error::Unsupported(format!("missing DOCX list {id}:{level}")))?;
        Ok(ListMarker {
            id,
            level,
            numbered,
        })
    }
}
fn add_before_end(xml: &mut String, addition: &str) -> Result<(), Error> {
    let i = xml
        .rfind("</")
        .ok_or_else(|| bad("invalid package relationship XML"))?;
    xml.insert_str(i, addition);
    Ok(())
}
pub(super) fn add_parts(
    parts: &mut BTreeMap<String, Vec<u8>>,
    blocks: &[Block],
) -> Result<(), Error> {
    let mut lists = BTreeMap::<u64, BTreeMap<u8, bool>>::new();
    for b in blocks {
        let paras: Vec<&Paragraph> = match b {
            Block::Paragraph(p) => vec![p],
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| r.iter().flat_map(|c| c.paragraphs.iter()))
                .collect(),
            _ => vec![],
        };
        for p in paras {
            if let Some(list) = &p.style.list {
                if list.id == 0 || list.id > u32::MAX as u64 || list.level > 8 {
                    return Err(Error::Invalid("invalid DOCX list ID or level".into()));
                }
                if lists
                    .entry(list.id)
                    .or_default()
                    .insert(list.level, list.numbered)
                    .is_some_and(|n| n != list.numbered)
                {
                    return Err(Error::Unsupported(
                        "conflicting numbering for list level".into(),
                    ));
                }
            }
        }
    }
    if lists.is_empty() {
        return Ok(());
    }
    let existing = Numbering::read(parts)?;
    let had_numbering = parts.contains_key("word/numbering.xml");
    let mut missing = BTreeMap::new();
    for (id, levels) in lists {
        if existing.items.keys().any(|(n, _)| *n == id) {
            for (level, numbered) in levels {
                if existing.items.get(&(id, level)) != Some(&numbered) {
                    return Err(Error::Unsupported("conflicting existing list level".into()));
                }
            }
        } else {
            missing.insert(id, levels);
        }
    }
    if missing.is_empty() {
        return Ok(());
    }
    let mut next_abstract = existing
        .abstracts
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Limit)?;
    let mut fragments = String::new();
    for (id, levels) in missing {
        let abstract_id = next_abstract;
        next_abstract = next_abstract.checked_add(1).ok_or(Error::Limit)?;
        fragments.push_str(&format!("<w:abstractNum w:abstractNumId=\"{abstract_id}\"><w:multiLevelType w:val=\"multilevel\"/>"));
        let fallback = levels.values().next().copied().unwrap_or(true);
        for level in 0..9 {
            let numbered = levels.get(&level).copied().unwrap_or(fallback);
            let fmt = if numbered { "decimal" } else { "bullet" };
            let text = if numbered {
                format!("%{}.", level + 1)
            } else {
                "•".into()
            };
            fragments.push_str(&format!("<w:lvl w:ilvl=\"{level}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/></w:lvl>"));
        }
        fragments.push_str(&format!("</w:abstractNum><w:num w:numId=\"{id}\"><w:abstractNumId w:val=\"{abstract_id}\"/></w:num>"));
    }
    let mut xml = if let Some(bytes) = parts.get("word/numbering.xml") {
        String::from_utf8(bytes.clone()).map_err(bad)?
    } else {
        format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><w:numbering xmlns:w=\"{W}\"></w:numbering>"
        )
    };
    add_before_end(&mut xml, &fragments)?;
    parts.insert("word/numbering.xml".into(), xml.into_bytes());
    if had_numbering {
        return Ok(());
    }
    let rel=parts.entry("word/_rels/document.xml.rels".into()).or_insert_with(||b"<?xml version=\"1.0\"?><Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec());
    let mut relxml = String::from_utf8(rel.clone()).map_err(bad)?;
    let relid = "rIdDOC42Numbering";
    if relxml.contains(relid) {
        return Err(bad("duplicate relationship"));
    }
    add_before_end(&mut relxml,&format!("<Relationship Id=\"{relid}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering\" Target=\"numbering.xml\"/>"))?;
    *rel = relxml.into_bytes();
    let ct = parts
        .get_mut("[Content_Types].xml")
        .ok_or_else(|| bad("missing content types"))?;
    let mut ctxml = String::from_utf8(ct.clone()).map_err(bad)?;
    add_before_end(&mut ctxml,"<Override PartName=\"/word/numbering.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml\"/>")?;
    *ct = ctxml.into_bytes();
    Ok(())
}
