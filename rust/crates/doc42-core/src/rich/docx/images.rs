//! OOXML inline picture relationships and DrawingML geometry.
use super::*;
use std::collections::BTreeMap;
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
fn add_before_end(xml: &mut String, addition: &str) -> Result<(), Error> {
    let i = xml.rfind("</").ok_or_else(|| bad("invalid package XML"))?;
    xml.insert_str(i, addition);
    Ok(())
}
fn extension(mime: &str) -> Result<&'static str, Error> {
    match mime {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        _ => Err(Error::Unsupported(format!("DOCX picture type {mime}"))),
    }
}
pub(super) fn paragraph(image: &Image) -> Result<String, Error> {
    let ext = extension(&image.mime)?;
    let cx = u64::from(image.width_twips) * 635;
    let cy = u64::from(image.height_twips) * 635;
    let id = image.id;
    let alt = esc(&image.alt);
    Ok(format!("<w:p><w:r><w:drawing><wp:inline><wp:extent cx=\"{cx}\" cy=\"{cy}\"/><wp:docPr id=\"{id}\" name=\"DOC42 Picture {id}\" descr=\"{alt}\"/><a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"image{id}.{ext}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed=\"rIdDOC42Image{id}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{cx}\" cy=\"{cy}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"))
}
pub(super) fn add_parts(
    parts: &mut BTreeMap<String, Vec<u8>>,
    blocks: &[Block],
) -> Result<(), Error> {
    let pictures: Vec<_> = blocks
        .iter()
        .filter_map(|b| {
            if let Block::Image(i) = b {
                Some(i)
            } else {
                None
            }
        })
        .collect();
    if pictures.is_empty() {
        return Ok(());
    }
    let mut rels=String::from_utf8(parts.get("word/_rels/document.xml.rels").cloned().unwrap_or_else(||b"<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_vec())).map_err(bad)?;
    let mut ct = String::from_utf8(
        parts
            .get("[Content_Types].xml")
            .cloned()
            .ok_or_else(|| bad("missing content types"))?,
    )
    .map_err(bad)?;
    let mut added = std::collections::HashSet::new();
    for image in pictures {
        let ext = extension(&image.mime)?;
        let rel = format!("rIdDOC42Image{}", image.id);
        let file = format!("word/media/imageDOC42{}.{ext}", image.id);
        let existing_rel = rels.contains(&format!("Id=\"{rel}\""));
        if existing_rel || parts.contains_key(&file) {
            if existing_rel && parts.get(&file) == Some(&image.data) {
                continue;
            }
            return Err(Error::Unsupported("picture relationship collision".into()));
        }
        add_before_end(&mut rels,&format!("<Relationship Id=\"{rel}\" Type=\"{R}/image\" Target=\"media/imageDOC42{}.{ext}\"/>",image.id))?;
        parts.insert(file, image.data.clone());
        if added.insert(ext) && !ct.contains(&format!("Extension=\"{ext}\"")) {
            add_before_end(
                &mut ct,
                &format!(
                    "<Default Extension=\"{ext}\" ContentType=\"{}\"/>",
                    image.mime
                ),
            )?;
        }
    }
    parts.insert("word/_rels/document.xml.rels".into(), rels.into_bytes());
    parts.insert("[Content_Types].xml".into(), ct.into_bytes());
    Ok(())
}
pub(super) fn read(
    p: roxmltree::Node<'_, '_>,
    id: u64,
    parts: &BTreeMap<String, Vec<u8>>,
) -> Result<Image, Error> {
    if p.descendants().any(|n| n.has_tag_name((W, "t"))) {
        return Err(Error::Unsupported("inline picture mixed with text".into()));
    }
    let drawing = p
        .descendants()
        .find(|n| n.has_tag_name((W, "drawing")))
        .ok_or_else(|| bad("missing drawing"))?;
    let inline = drawing
        .children()
        .find(|n| n.has_tag_name((WP, "inline")))
        .ok_or_else(|| Error::Unsupported("floating DOCX picture".into()))?;
    let extent = inline
        .children()
        .find(|n| n.has_tag_name((WP, "extent")))
        .ok_or_else(|| bad("picture extent"))?;
    let dimension = |name: &str| -> Result<u32, Error> {
        let v = extent
            .attribute(name)
            .ok_or_else(|| bad("picture extent"))?
            .parse::<u64>()
            .map_err(bad)?;
        u32::try_from((v + 317) / 635).map_err(|_| Error::Limit)
    };
    let blip = inline
        .descendants()
        .find(|n| n.has_tag_name((A, "blip")))
        .ok_or_else(|| bad("picture blip"))?;
    let rid = blip
        .attribute((R, "embed"))
        .ok_or_else(|| Error::Unsupported("linked DOCX picture".into()))?;
    let rels = parts
        .get("word/_rels/document.xml.rels")
        .ok_or_else(|| bad("missing picture relationship"))?;
    let rels = std::str::from_utf8(rels).map_err(bad)?;
    let rels = roxmltree::Document::parse(rels).map_err(bad)?;
    let rel = rels
        .descendants()
        .find(|n| {
            n.is_element()
                && n.tag_name().name() == "Relationship"
                && n.attribute("Id") == Some(rid)
        })
        .ok_or_else(|| bad("missing picture relationship"))?;
    if rel.attribute("TargetMode").is_some() {
        return Err(Error::Unsupported("external image".into()));
    }
    let target = rel
        .attribute("Target")
        .ok_or_else(|| bad("picture target"))?;
    if !target.starts_with("media/") || target.contains("..") {
        return Err(Error::Unsupported("picture target".into()));
    }
    let path = format!("word/{target}");
    let data = parts
        .get(&path)
        .ok_or_else(|| bad("missing picture data"))?
        .clone();
    let mime = if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if data.starts_with(&[0xff, 0xd8, 0xff]) {
        "image/jpeg"
    } else {
        return Err(Error::Unsupported("picture encoding".into()));
    };
    let alt = inline
        .children()
        .find(|n| n.has_tag_name((WP, "docPr")))
        .and_then(|n| n.attribute("descr"))
        .unwrap_or("")
        .to_owned();
    Ok(Image {
        id,
        mime: mime.into(),
        data,
        width_twips: dimension("cx")?,
        height_twips: dimension("cy")?,
        alt,
    })
}
