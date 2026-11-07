//! Only pure, explicitly dynamic cell-metadata records are treated as disposable spill caches.
use super::*;
pub(super) fn indices(text: &str) -> Result<std::collections::BTreeSet<usize>, Error> {
    let doc = xml(text)?;
    let types: Vec<_> = doc
        .descendants()
        .find(|n| n.has_tag_name((S, "metadataTypes")))
        .into_iter()
        .flat_map(|n| n.children().filter(|n| n.is_element()))
        .map(|n| n.attribute("name").unwrap_or(""))
        .collect();
    let mut out = std::collections::BTreeSet::new();
    let future: Vec<_> = doc
        .descendants()
        .find(|n| n.has_tag_name((S, "futureMetadata")) && n.attribute("name") == Some("XLDAPR"))
        .into_iter()
        .flat_map(|n| n.children().filter(|n| n.has_tag_name((S, "bk"))))
        .collect();
    let records = doc
        .descendants()
        .find(|n| n.has_tag_name((S, "cellMetadata")))
        .into_iter()
        .flat_map(|n| n.children().filter(|n| n.has_tag_name((S, "bk"))));
    for (index, bk) in records.enumerate() {
        let children: Vec<_> = bk.children().filter(|n| n.is_element()).collect();
        if children.len() != 1 || !children[0].has_tag_name((S, "rc")) {
            continue;
        }
        let rc = children[0];
        let Some(t) = rc
            .attribute("t")
            .and_then(|v| v.parse::<usize>().ok())
            .and_then(|v| v.checked_sub(1))
        else {
            continue;
        };
        let Some(v) = rc.attribute("v").and_then(|v| v.parse::<usize>().ok()) else {
            continue;
        };
        if types.get(t) == Some(&"XLDAPR")
            && future.get(v).is_some_and(|n| {
                n.descendants().any(|p| {
                    p.has_tag_name((
                        "http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray",
                        "dynamicArrayProperties",
                    )) && matches!(p.attribute("fDynamic"), Some("1" | "true"))
                })
            })
        {
            out.insert(index + 1);
        }
    }
    Ok(out)
}
pub(super) fn ensure(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    entries: &mut BTreeMap<String, Vec<u8>>,
) -> Result<usize, Error> {
    if let Some(text) = zip_text(archive, "xl/metadata.xml")? {
        if let Some(index) = indices(&text)?.first() {
            return Ok(*index);
        }
        return Err(Error::Invalid(
            "This workbook's existing metadata cannot yet register dynamic arrays safely",
        ));
    }
    let types =
        zip_text(archive, "[Content_Types].xml")?.ok_or(Error::Invalid("Missing content types"))?;
    let doc = xml(&types)?;
    let root = doc.root_element();
    if doc
        .descendants()
        .any(|n| n.attribute("PartName") == Some("/xl/metadata.xml"))
    {
        return Err(Error::Invalid("Missing declared array metadata"));
    }
    let p = prefix(&types[root.range()]);
    let addition=format!("<{p}Override PartName=\"/xl/metadata.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheetMetadata+xml\"/>");
    let end = root.range().start
        + types[root.range()]
            .rfind("</")
            .ok_or(Error::Invalid("Invalid content types"))?;
    entries.insert(
        "[Content_Types].xml".into(),
        apply_edits(types.clone(), vec![(end, end, addition)]).into_bytes(),
    );
    let rels = zip_text(archive, "xl/_rels/workbook.xml.rels")?
        .ok_or(Error::Invalid("Missing workbook relationships"))?;
    let doc = xml(&rels)?;
    let root = doc.root_element();
    if doc.descendants().any(|n| {
        n.attribute("Type")
            .is_some_and(|t| t.ends_with("/sheetMetadata"))
    }) {
        return Err(Error::Invalid("Missing linked array metadata"));
    }
    let mut id = 1;
    while doc
        .descendants()
        .any(|n| n.attribute("Id") == Some(format!("rId{id}").as_str()))
    {
        id += 1;
    }
    let p = prefix(&rels[root.range()]);
    let addition=format!("<{p}Relationship Id=\"rId{id}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/sheetMetadata\" Target=\"metadata.xml\"/>");
    let at = root.range().start
        + rels[root.range()]
            .rfind("</")
            .ok_or(Error::Invalid("Invalid workbook relationships"))?;
    entries.insert(
        "xl/_rels/workbook.xml.rels".into(),
        apply_edits(rels.clone(), vec![(at, at, addition)]).into_bytes(),
    );
    entries.insert("xl/metadata.xml".into(),format!(r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><metadata xmlns="{S}" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes count="1"><metadataType name="XLDAPR" minSupportedVersion="120000" copy="1" pasteAll="1" pasteValues="1" merge="1" splitFirst="1" rowColShift="1" clearFormats="1" clearComments="1" assign="1" coerce="1" cellMeta="1"/></metadataTypes><futureMetadata name="XLDAPR" count="1"><bk><extLst><ext uri="{{bdbb8cdc-fa1e-496e-a857-3c3f30c029c3}}"><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></ext></extLst></bk></futureMetadata><cellMetadata count="1"><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>"#).into_bytes());
    Ok(1)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_pure_explicitly_dynamic_records_identify_disposable_caches() {
        let text = format!(
            r#"<metadata xmlns="{S}" xmlns:xda="http://schemas.microsoft.com/office/spreadsheetml/2017/dynamicarray"><metadataTypes><metadataType name="XLDAPR"/><metadataType name="Other"/></metadataTypes><futureMetadata name="XLDAPR"><bk><xda:dynamicArrayProperties fDynamic="1" fCollapsed="0"/></bk><bk><xda:dynamicArrayProperties fDynamic="0"/></bk></futureMetadata><cellMetadata><bk><rc t="1" v="0"/></bk><bk><rc t="1" v="0"/><rc t="2" v="0"/></bk><bk><rc t="1" v="1"/></bk><bk><rc t="1" v="0"/></bk></cellMetadata></metadata>"#
        );
        assert_eq!(
            indices(&text).unwrap(),
            std::collections::BTreeSet::from([1, 4])
        );
    }
}
