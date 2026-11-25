//! Normalize a supported Word TOC field for the ordinary paragraph reader.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct Scan {
    pub tables: Vec<Vec<(usize, String)>>,
    pub bookmarks: BTreeMap<String, usize>,
    pub cached: Vec<Option<Vec<String>>>,
    pub levels: Vec<(u8, u8)>,
}

pub(super) fn normalize(xml: &str) -> Result<(String, Option<Scan>), Error> {
    let doc = roxmltree::Document::parse(xml).map_err(bad)?;
    if !doc.descendants().any(|n| {
        n.has_tag_name((W, "instrText"))
            && n.text()
                .unwrap_or("")
                .trim_start()
                .to_ascii_uppercase()
                .starts_with("TOC ")
    }) {
        return Ok((xml.to_owned(), None));
    }
    let body = doc
        .descendants()
        .find(|n| n.has_tag_name((W, "body")))
        .ok_or_else(|| bad("DOCX body"))?;
    let mut ranges = Vec::new();
    let mut tables: Vec<Vec<(usize, String)>> = Vec::new();
    let mut cached = Vec::new();
    let mut levels = Vec::new();
    let mut state = 0u8;
    let mut instruction = String::new();
    let mut all_bookmarks: BTreeMap<String, (String, usize, std::ops::Range<usize>)> =
        BTreeMap::new();
    let mut ends = BTreeMap::new();
    for (index, p) in body.children().filter(|n| n.is_element()).enumerate() {
        if !p.has_tag_name((W, "p")) {
            if state != 0 {
                return Err(Error::Unsupported("non-paragraph Word TOC".into()));
            }
            continue;
        }
        for child in p.children().filter(|n| n.is_element()) {
            if child.has_tag_name((W, "bookmarkStart")) {
                if child
                    .attributes()
                    .any(|a| a.namespace() != Some(W) || !["id", "name"].contains(&a.name()))
                    || child.children().any(|n| n.is_element())
                {
                    return Err(Error::Unsupported("Word bookmark attributes".into()));
                }
                let id = val(child, "id").ok_or_else(|| bad("bookmark id"))?;
                let name = val(child, "name").ok_or_else(|| bad("bookmark name"))?;
                if all_bookmarks
                    .insert(name.to_owned(), (id.to_owned(), index, child.range()))
                    .is_some()
                {
                    return Err(bad("duplicate bookmark"));
                }
            } else if child.has_tag_name((W, "bookmarkEnd")) {
                if child
                    .attributes()
                    .any(|a| a.namespace() != Some(W) || a.name() != "id")
                    || child.children().any(|n| n.is_element())
                {
                    return Err(Error::Unsupported("Word bookmark end attributes".into()));
                }
                let id = val(child, "id").ok_or_else(|| bad("bookmark end"))?;
                if ends.insert(id.to_owned(), child.range()).is_some() {
                    return Err(bad("duplicate bookmark end"));
                }
            } else if child.has_tag_name((W, "r")) {
                let children: Vec<_> = child.children().filter(|n| n.is_element()).collect();
                if children.len() == 1 && children[0].has_tag_name((W, "fldChar")) {
                    if children[0].attributes().any(|a| {
                        a.namespace() != Some(W)
                            || !(a.name() == "fldCharType"
                                || (a.name() == "dirty"
                                    && matches!(a.value(), "0" | "1" | "true" | "false")))
                    }) || children[0].children().any(|n| n.is_element())
                    {
                        return Err(Error::Unsupported("Word TOC field attributes".into()));
                    }
                    match val(children[0], "fldCharType") {
                        Some("begin") if state == 0 => {
                            state = 1;
                            instruction.clear();
                            ranges.push(child.range());
                        }
                        Some("separate")
                            if state == 1
                                && instruction
                                    .trim_start()
                                    .to_ascii_uppercase()
                                    .starts_with("TOC ") =>
                        {
                            let words = instruction.split_whitespace().collect::<Vec<_>>();
                            let mut index = 1;
                            let mut uses_headings = false;
                            let mut level_range = (0u8, 8u8);
                            while index < words.len() {
                                match words[index].to_ascii_lowercase().as_str() {
                                    "\\o" => {
                                        index += 1;
                                        let range = words
                                            .get(index)
                                            .ok_or_else(|| {
                                                Error::Unsupported("Word TOC range".into())
                                            })?
                                            .trim_matches('"');
                                        let (first, last) =
                                            range.split_once('-').ok_or_else(|| {
                                                Error::Unsupported("Word TOC outline range".into())
                                            })?;
                                        let first = first.parse::<u8>().map_err(bad)?;
                                        let last = last.parse::<u8>().map_err(bad)?;
                                        if first == 0 || first > last || last > 9 {
                                            return Err(Error::Unsupported(
                                                "Word TOC outline range".into(),
                                            ));
                                        }
                                        level_range = (first - 1, last - 1);
                                        uses_headings = true;
                                    }
                                    "\\u" => uses_headings = true,
                                    "\\h" | "\\z" => {}
                                    "\\*" => {
                                        index += 1;
                                        if !words
                                            .get(index)
                                            .is_some_and(|w| w.eq_ignore_ascii_case("MERGEFORMAT"))
                                        {
                                            return Err(Error::Unsupported(
                                                "Word TOC formatting switch".into(),
                                            ));
                                        }
                                    }
                                    _ => {
                                        return Err(Error::Unsupported(
                                            "Word TOC field switches".into(),
                                        ))
                                    }
                                }
                                index += 1;
                            }
                            if !uses_headings {
                                return Err(Error::Unsupported("Word TOC source".into()));
                            }
                            state = 2;
                            tables.push(Vec::new());
                            cached.push(false);
                            levels.push(level_range);
                            ranges.push(child.range());
                        }
                        Some("end") if state == 2 => {
                            state = 0;
                            ranges.push(child.range());
                        }
                        _ if state != 0 => {
                            return Err(Error::Unsupported("complex Word TOC field".into()))
                        }
                        _ => {}
                    }
                } else if state == 1
                    && children.len() == 1
                    && children[0].has_tag_name((W, "instrText"))
                {
                    instruction.push_str(children[0].text().unwrap_or(""));
                    ranges.push(child.range());
                } else if state == 2 {
                    *cached.last_mut().unwrap() = true;
                } else if state != 0 {
                    return Err(Error::Unsupported("Word TOC field result".into()));
                }
            } else if child.has_tag_name((W, "hyperlink")) && state == 2 {
                let target = val(child, "anchor")
                    .ok_or_else(|| Error::Unsupported("external Word TOC link".into()))?;
                if child.attributes().any(|a| {
                    a.namespace() != Some(W)
                        || !(a.name() == "anchor"
                            || (a.name() == "history"
                                && matches!(a.value(), "0" | "1" | "true" | "false")))
                }) {
                    return Err(Error::Unsupported("Word TOC link attributes".into()));
                }
                if child.descendants().any(|n| n.has_tag_name((W, "tab"))) {
                    *cached.last_mut().unwrap() = true;
                }
                let mut nested = 0u8;
                let mut nested_instruction = String::new();
                for run in child.children().filter(|n| n.is_element()) {
                    if !run.has_tag_name((W, "r")) {
                        return Err(Error::Unsupported("Word TOC hyperlink content".into()));
                    }
                    let elements = run
                        .children()
                        .filter(|n| n.is_element() && !n.has_tag_name((W, "rPr")))
                        .collect::<Vec<_>>();
                    if elements.len() == 1 && elements[0].has_tag_name((W, "fldChar")) {
                        match val(elements[0], "fldCharType") {
                            Some("begin") if nested == 0 => {
                                nested = 1;
                                nested_instruction.clear();
                            }
                            Some("separate")
                                if nested == 1
                                    && nested_instruction
                                        .trim_start()
                                        .to_ascii_uppercase()
                                        .starts_with("PAGEREF ") =>
                            {
                                nested = 2
                            }
                            Some("end") if nested == 2 => nested = 0,
                            _ => {
                                return Err(Error::Unsupported("Word TOC cached page field".into()))
                            }
                        }
                        *cached.last_mut().unwrap() = true;
                        ranges.push(run.range());
                    } else if nested == 1
                        && elements.len() == 1
                        && elements[0].has_tag_name((W, "instrText"))
                    {
                        nested_instruction.push_str(elements[0].text().unwrap_or(""));
                        ranges.push(run.range());
                    } else if nested == 1 {
                        return Err(Error::Unsupported("Word TOC page instruction".into()));
                    }
                }
                if nested != 0 {
                    return Err(Error::Unsupported("unclosed Word TOC page field".into()));
                }
                let range = child.range();
                let text = &xml[range.clone()];
                let open = text.find('>').ok_or_else(|| bad("Word TOC link"))? + 1;
                let close = text.rfind('<').ok_or_else(|| bad("Word TOC link"))?;
                ranges.push(range.start..range.start + open);
                ranges.push(range.start + close..range.end);
                tables.last_mut().unwrap().push((index, target.to_owned()));
            } else if state != 0 && !child.has_tag_name((W, "pPr")) {
                return Err(Error::Unsupported("Word TOC content".into()));
            }
        }
    }
    if state != 0 || tables.is_empty() || tables.iter().any(Vec::is_empty) {
        return Err(Error::Unsupported("incomplete Word TOC".into()));
    }
    let cached_targets: BTreeSet<_> = tables
        .iter()
        .zip(&cached)
        .filter(|(_, c)| **c)
        .flat_map(|(t, _)| t.iter().map(|(_, name)| name.clone()))
        .collect();
    let wanted: BTreeSet<_> = tables
        .iter()
        .flatten()
        .map(|(_, name)| name.clone())
        .collect();
    let mut bookmarks = BTreeMap::new();
    for name in wanted {
        let (id, index, start) = all_bookmarks
            .get(&name)
            .ok_or_else(|| Error::Unsupported("unresolved Word TOC bookmark".into()))?;
        let end = ends.get(id).ok_or_else(|| bad("unclosed bookmark"))?;
        if !cached_targets.contains(&name) {
            ranges.push(start.clone());
            ranges.push(end.clone());
        }
        bookmarks.insert(name, *index);
    }
    let nodes = body
        .children()
        .filter(|n| n.is_element())
        .collect::<Vec<_>>();
    let mut cached_xml = Vec::new();
    for (table, keep) in tables.iter().zip(cached) {
        if !keep {
            cached_xml.push(None);
            continue;
        }
        if table.windows(2).any(|w| w[1].0 != w[0].0 + 1) {
            return Err(Error::Unsupported("non-contiguous Word TOC cache".into()));
        }
        let mut paragraphs = Vec::new();
        for (index, _) in table {
            let p = nodes.get(*index).ok_or_else(|| bad("Word TOC paragraph"))?;
            let mut raw = xml[p.range()].to_owned();
            let insert = raw.find('>').ok_or_else(|| bad("Word TOC paragraph tag"))?;
            let opening = &raw[..insert];
            let mut declarations = String::new();
            for ns in doc.root_element().namespaces() {
                if let Some(prefix) = ns.name() {
                    if prefix != "xml" && !opening.contains(&format!("xmlns:{prefix}")) {
                        declarations.push_str(&format!(" xmlns:{prefix}=\"{}\"", esc(ns.uri())));
                    }
                } else if !opening.contains("xmlns=") {
                    declarations.push_str(&format!(" xmlns=\"{}\"", esc(ns.uri())));
                }
            }
            raw.insert_str(insert, &declarations);
            paragraphs.push(raw);
        }
        cached_xml.push(Some(paragraphs));
    }
    ranges.sort_by_key(|r| r.start);
    if ranges.windows(2).any(|w| w[0].end > w[1].start) {
        return Err(bad("overlapping Word field ranges"));
    }
    let mut output = xml.to_owned();
    for range in ranges.into_iter().rev() {
        output.replace_range(range, "");
    }
    Ok((
        output,
        Some(Scan {
            tables,
            bookmarks,
            cached: cached_xml,
            levels,
        }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn equivalent_word_switches_keep_linked_heading() {
        let xml = format!(
            r#"<w:document xmlns:w="{W}"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin" w:dirty="1"/></w:r><w:r><w:instrText> toc \h \o "1-9" \u \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:hyperlink w:anchor="Head" w:history="1"><w:r><w:t>Title</w:t></w:r></w:hyperlink><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:bookmarkStart w:id="9" w:name="Head"/><w:bookmarkEnd w:id="9"/><w:r><w:t>Title</w:t></w:r></w:p></w:body></w:document>"#
        );
        let (_, scan) = normalize(&xml).unwrap();
        let scan = scan.unwrap();
        assert_eq!(scan.tables[0][0].1, "Head");
        assert_eq!(scan.bookmarks["Head"], 1);
        let restricted = xml.replace("1-9", "1-3");
        assert_eq!(normalize(&restricted).unwrap().1.unwrap().levels[0], (0, 2));
    }
}
