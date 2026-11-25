//! Clone the source XF and change only explicitly requested style properties.
use super::{
    office::{apply_edits, escape_xml, prefix, xml},
    CellStyle, Error,
};
pub(super) fn attrs(text: &str, updates: &[(&str, String)]) -> Result<String, Error> {
    let d = xml(text)?;
    let n = d.root_element();
    let range = n.range();
    let original = &text[range.clone()];
    let opening = original
        .find('>')
        .ok_or(Error::Invalid("invalid XLSX style"))?;
    let at = if original.as_bytes().get(opening.wrapping_sub(1)) == Some(&b'/') {
        opening - 1
    } else {
        opening
    };
    let mut edits = Vec::new();
    let mut add = String::new();
    for (name, value) in updates {
        if let Some(a) = n.attributes().find(|a| a.name() == *name) {
            let r = a.range();
            edits.push((
                r.start - range.start,
                r.end - range.start,
                format!("{name}=\"{}\"", escape_xml(value)),
            ));
        } else {
            add.push_str(&format!(" {name}=\"{}\"", escape_xml(value)))
        }
    }
    edits.push((at, at, add));
    Ok(apply_edits(original.to_string(), edits))
}
fn children(text: &str, updates: &[(&str, Option<String>)]) -> Result<String, Error> {
    let d = xml(text)?;
    let n = d.root_element();
    let original = &text[n.range()];
    let p = prefix(original);
    let mut edits = Vec::new();
    let mut add = String::new();
    for (name, value) in updates {
        let matches: Vec<_> = n.children().filter(|v| v.has_tag_name(*name)).collect();
        if matches.len() > 1 {
            return Err(Error::Invalid("Unsupported duplicate style properties"));
        }
        if let Some(node) = matches.first() {
            let r = node.range();
            edits.push((
                r.start - n.range().start,
                r.end - n.range().start,
                value.clone().unwrap_or_default(),
            ))
        } else if let Some(value) = value {
            add.push_str(value)
        }
    }
    if original.trim_end().ends_with("/>") && !add.is_empty() {
        let at = original.rfind("/>").unwrap();
        let name = n.tag_name().name();
        return Ok(format!("{}>{add}</{p}{name}>", &original[..at]));
    }
    let at = original
        .rfind("</")
        .ok_or(Error::Invalid("invalid XLSX style element"))?;
    edits.push((at, at, add));
    Ok(apply_edits(original.to_string(), edits))
}
fn node_at(xmltext: &str, section: &str, tag: &str, index: usize) -> Result<String, Error> {
    let doc = xml(xmltext)?;
    let root = doc.root_element();
    let node = root
        .children()
        .find(|n| n.has_tag_name(section))
        .and_then(|s| s.children().filter(|n| n.has_tag_name(tag)).nth(index))
        .ok_or(Error::Invalid("XLSX style index is out of bounds"))?;
    Ok(xmltext[node.range()].to_owned())
}
fn section_count(xmltext: &str, section: &str, tag: &str) -> Result<usize, Error> {
    let d = xml(xmltext)?;
    Ok(d.root_element()
        .children()
        .find(|n| n.has_tag_name(section))
        .map_or(0, |n| n.children().filter(|x| x.has_tag_name(tag)).count()))
}
fn append(xmltext: &str, section: &str, tag: &str, element: &str) -> Result<String, Error> {
    let d = xml(xmltext)?;
    let root = d.root_element();
    let Some(node) = root.children().find(|n| n.has_tag_name(section)) else {
        if section != "numFmts" {
            return Err(Error::Invalid("missing XLSX style section"));
        }
        let insert = root
            .children()
            .find(|n| n.has_tag_name("fonts"))
            .map(|n| n.range().start)
            .ok_or(Error::Invalid("missing fonts"))?;
        let p = prefix(&xmltext[root.range()]);
        let mut out = xmltext.to_owned();
        out.insert_str(
            insert,
            &format!("<{p}numFmts count=\"1\">{element}</{p}numFmts>"),
        );
        return Ok(out);
    };
    let count = node.children().filter(|n| n.has_tag_name(tag)).count();
    if count >= 65_000 {
        return Err(Error::Limit);
    }
    let range = node.range();
    let raw = &xmltext[range.clone()];
    let p = prefix(raw);
    let mut inside = if raw.trim_end().ends_with("/>") {
        let at = raw.rfind("/>").unwrap();
        format!("{}>{element}</{p}{section}>", &raw[..at])
    } else {
        let at = raw
            .rfind("</")
            .ok_or(Error::Invalid("invalid style section"))?;
        let mut v = raw.to_owned();
        v.insert_str(at, element);
        v
    };
    inside = attrs(&inside, &[("count", (count + 1).to_string())])?;
    let mut out = xmltext.to_owned();
    out.replace_range(range, &inside);
    Ok(out)
}
pub(super) fn clone_style(
    styles: &str,
    xf_index: usize,
    old: &CellStyle,
    new: &CellStyle,
) -> Result<(String, usize), Error> {
    let mut doc = styles.to_owned();
    let mut xf = node_at(&doc, "cellXfs", "xf", xf_index)?;
    let xfdoc = xml(&xf)?;
    let root = xfdoc.root_element();
    let idx = |name| {
        root.attribute(name)
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0)
    };
    let (font_id, fill_id, border_id) = (idx("fontId"), idx("fillId"), idx("borderId"));
    let mut updates = Vec::<(&str, String)>::new();
    if old.bold != new.bold
        || old.italic != new.italic
        || old.underline != new.underline
        || old.font != new.font
        || old.font_size != new.font_size
        || old.foreground != new.foreground
    {
        let font = node_at(&doc, "fonts", "font", font_id)?;
        let p = prefix(&font);
        let mut u = Vec::new();
        if old.bold != new.bold {
            u.push(("b", new.bold.then(|| format!("<{p}b/>"))))
        }
        if old.italic != new.italic {
            u.push(("i", new.italic.then(|| format!("<{p}i/>"))))
        }
        if old.underline != new.underline {
            u.push(("u", new.underline.then(|| format!("<{p}u/>"))))
        }
        if old.font != new.font {
            u.push((
                "name",
                (!new.font.is_empty())
                    .then(|| format!("<{p}name val=\"{}\"/>", escape_xml(&new.font))),
            ))
        }
        if old.font_size != new.font_size {
            u.push((
                "sz",
                (new.font_size > 0).then(|| format!("<{p}sz val=\"{}\"/>", new.font_size)),
            ))
        }
        if old.foreground != new.foreground {
            u.push((
                "color",
                new.foreground
                    .map(|c| format!("<{p}color rgb=\"FF{:06X}\"/>", c & 0xffffff)),
            ))
        }
        let font = children(&font, &u)?;
        let index = section_count(&doc, "fonts", "font")?;
        doc = append(&doc, "fonts", "font", &font)?;
        updates.push(("fontId", index.to_string()));
        updates.push(("applyFont", "1".into()));
    }
    if old.background != new.background {
        let fill = node_at(&doc, "fills", "fill", fill_id)?;
        let p = prefix(&fill);
        if xml(&fill)?.descendants().any(|n| n.has_tag_name("extLst")) {
            return Err(Error::Invalid("Cannot safely alter an extended XLSX fill"));
        }
        let part=match new.background{Some(c)=>format!("<{p}fill><{p}patternFill patternType=\"solid\"><{p}fgColor rgb=\"FF{:06X}\"/><{p}bgColor indexed=\"64\"/></{p}patternFill></{p}fill>",c&0xffffff),None=>format!("<{p}fill><{p}patternFill patternType=\"none\"/></{p}fill>")};
        let index = section_count(&doc, "fills", "fill")?;
        doc = append(&doc, "fills", "fill", &part)?;
        updates.push(("fillId", index.to_string()));
        updates.push(("applyFill", "1".into()));
    }
    if old.borders != new.borders {
        let border = node_at(&doc, "borders", "border", border_id)?;
        let p = prefix(&border);
        let mut u = Vec::new();
        for (name, bit) in [("top", 1), ("right", 2), ("bottom", 4), ("left", 8)] {
            if (old.borders & bit) != (new.borders & bit) {
                let part = if new.borders & bit != 0 {
                    format!("<{p}{name} style=\"thin\"><{p}color auto=\"1\"/></{p}{name}>")
                } else {
                    format!("<{p}{name}/>")
                };
                u.push((name, Some(part)))
            }
        }
        let border = children(&border, &u)?;
        let index = section_count(&doc, "borders", "border")?;
        doc = append(&doc, "borders", "border", &border)?;
        updates.push(("borderId", index.to_string()));
        updates.push(("applyBorder", "1".into()));
    }
    if old.number_format != new.number_format {
        let id = if new.number_format.is_empty() {
            0
        } else {
            let d = xml(&doc)?;
            let largest = d
                .descendants()
                .filter(|n| n.has_tag_name("numFmt"))
                .filter_map(|n| {
                    n.attribute("numFmtId")
                        .and_then(|x| x.parse::<usize>().ok())
                })
                .max()
                .unwrap_or(163)
                .max(163);
            let id = largest + 1;
            let p = prefix(&doc[d.root_element().range()]);
            let element = format!(
                "<{p}numFmt numFmtId=\"{id}\" formatCode=\"{}\"/>",
                escape_xml(&new.number_format)
            );
            doc = append(&doc, "numFmts", "numFmt", &element)?;
            id
        };
        updates.push(("numFmtId", id.to_string()));
        updates.push(("applyNumberFormat", "1".into()));
    }
    if !updates.is_empty() {
        xf = attrs(&xf, &updates)?
    }
    let index = section_count(&doc, "cellXfs", "xf")?;
    doc = append(&doc, "cellXfs", "xf", &xf)?;
    Ok((doc, index))
}
