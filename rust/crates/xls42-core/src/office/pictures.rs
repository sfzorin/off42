//! Targeted picture edits; unrelated ZIP entries and drawing objects are preserved.
use super::*;
use std::collections::{BTreeMap, BTreeSet};
const REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const DRAW: &str = "http://schemas.openxmlformats.org/drawingml/2006/spreadsheetDrawing";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const ODF_DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
type Parts = BTreeMap<String, Vec<u8>>;
fn package(bytes: &[u8]) -> Result<Parts, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid picture package"))?;
    if zip.len() > 10_000 {
        return Err(Error::Limit);
    }
    let mut parts = Parts::new();
    let mut total = 0u64;
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|_| Error::Invalid("Invalid package entry"))?;
        total = total.checked_add(file.size()).ok_or(Error::Limit)?;
        if total > LIMIT as u64 {
            return Err(Error::Limit);
        }
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;
        if parts.insert(file.name().into(), data).is_some() {
            return Err(Error::Invalid("Duplicate package entry"));
        }
    }
    Ok(parts)
}
fn text<'a>(parts: &'a Parts, name: &str) -> Result<&'a str, Error> {
    std::str::from_utf8(
        parts
            .get(name)
            .ok_or(Error::Invalid("Missing picture XML"))?,
    )
    .map_err(|_| Error::Invalid("Invalid picture XML encoding"))
}
fn save(bytes: &[u8], mut parts: Parts) -> Result<Vec<u8>, Error> {
    let mut zip =
        zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("Invalid package"))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut file = zip
            .by_index(i)
            .map_err(|_| Error::Invalid("Invalid package entry"))?;
        let name = file.name().to_owned();
        if name == EXTRA {
            continue;
        }
        let Some(data) = parts.remove(&name) else {
            continue;
        };
        let mut before = Vec::new();
        file.read_to_end(&mut before)?;
        drop(file);
        if data == before {
            writer
                .raw_copy_file(
                    zip.by_index(i)
                        .map_err(|_| Error::Invalid("Invalid package entry"))?,
                )
                .map_err(|_| Error::Invalid("Cannot preserve package entry"))?;
        } else {
            write_part(&mut writer, &name, &data)?;
        }
    }
    for (name, data) in parts {
        if name != EXTRA {
            write_part(&mut writer, &name, &data)?;
        }
    }
    let data = writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot finish picture package"))?
        .into_inner();
    if data.len() > LIMIT {
        return Err(Error::Limit);
    }
    Ok(data)
}
fn write_part(
    writer: &mut zip::ZipWriter<Cursor<Vec<u8>>>,
    name: &str,
    data: &[u8],
) -> Result<(), Error> {
    writer
        .start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(|_| Error::Invalid("Cannot write picture part"))?;
    writer.write_all(data)?;
    Ok(())
}
fn resolve(base: &str, target: &str) -> Option<String> {
    if target.contains(['\\', ':', '?', '#', '%']) {
        return None;
    }
    let mut out: Vec<_> = if target.starts_with('/') {
        Vec::new()
    } else {
        base.rsplit_once('/')
            .map_or("", |v| v.0)
            .split('/')
            .filter(|v| !v.is_empty())
            .collect()
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            _ => out.push(part),
        }
    }
    Some(out.join("/"))
}
fn rel_path(part: &str) -> String {
    let (dir, name) = part.rsplit_once('/').unwrap_or(("", part));
    format!("{dir}/_rels/{name}.rels")
        .trim_start_matches('/')
        .into()
}
fn relation(parts: &Parts, owner: &str, id: &str, kind: &str) -> Option<String> {
    let doc = xml(text(parts, &rel_path(owner)).ok()?).ok()?;
    let rel = doc.root_element().children().find(|n| {
        n.has_tag_name("Relationship")
            && n.attribute("Id") == Some(id)
            && n.attribute("TargetMode") != Some("External")
            && n.attribute("Type").is_some_and(|v| v.ends_with(kind))
    })?;
    resolve(owner, rel.attribute("Target")?)
}
fn sheets(parts: &Parts) -> Result<BTreeMap<String, String>, Error> {
    let doc = xml(text(parts, "xl/workbook.xml")?)?;
    doc.descendants()
        .filter(|n| n.has_tag_name("sheet"))
        .map(|node| {
            let name = node
                .attribute("name")
                .ok_or(Error::Invalid("Missing sheet name"))?;
            let id = node
                .attribute((REL, "id"))
                .ok_or(Error::Invalid("Missing sheet relationship"))?;
            let path = relation(parts, "xl/workbook.xml", id, "/worksheet")
                .ok_or(Error::Invalid("Invalid sheet relationship"))?;
            Ok((name.into(), path))
        })
        .collect()
}
fn attr<'a, 'b: 'a>(node: roxmltree::Node<'a, 'b>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}
fn number(node: roxmltree::Node<'_, '_>, name: &str) -> Option<f64> {
    node.children()
        .find(|n| n.has_tag_name(name))?
        .text()?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}
fn import_image(
    state: &mut State,
    index: usize,
    data: Vec<u8>,
    geometry: [f64; 6],
    description: String,
    source: String,
) -> Result<(), Error> {
    let (pixel_width, pixel_height) = match crate::pictures::validate_png(&data) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };
    let [row, col, offset_x, offset_y, width_points, height_points] = geometry;
    if row.fract() != 0.0
        || col.fract() != 0.0
        || row < 0.0
        || col < 0.0
        || row >= MAX_ROWS as f64
        || col >= MAX_COLS as f64
    {
        return Ok(());
    }
    let sheet = &mut state.sheets[index];
    sheet.rows = sheet.rows.max(row as usize + 1);
    sheet.cols = sheet.cols.max(col as usize + 1);
    let picture = SheetImage {
        id: state.next_id,
        row: row as usize,
        column: col as usize,
        offset_x,
        offset_y,
        pixel_width,
        pixel_height,
        width_points,
        height_points,
        data: data.into(),
        description,
        source: Some(source),
    };
    if !crate::pictures::valid(&picture, sheet) {
        return Ok(());
    }
    if sheet.images.len() >= 1000 {
        return Err(Error::Limit);
    }
    sheet.images.push(picture);
    state.next_id = state.next_id.checked_add(1).ok_or(Error::Limit)?;
    let pictures = state.sheets.iter().flat_map(|sheet| &sheet.images);
    if pictures.clone().map(|p| p.data.len()).sum::<usize>() > crate::pictures::MAX_PICTURE_BYTES
        || pictures
            .map(|p| u64::from(p.pixel_width) * u64::from(p.pixel_height))
            .sum::<u64>()
            > 16_000_000
    {
        return Err(Error::Limit);
    }
    Ok(())
}
pub(super) fn import(bytes: &[u8], kind: &str, state: &mut State) -> Result<(), Error> {
    if !matches!(kind, "xlsx" | "ods") {
        return Ok(());
    }
    let parts = package(bytes)?;
    if kind == "xlsx" {
        import_xlsx(&parts, state)
    } else {
        import_ods(&parts, state)
    }
}
fn import_xlsx(parts: &Parts, state: &mut State) -> Result<(), Error> {
    let paths = sheets(parts)?;
    for index in 0..state.sheets.len() {
        let path = paths
            .get(&state.sheets[index].name)
            .ok_or(Error::Invalid("Missing picture sheet"))?;
        let doc = xml(text(parts, path)?)?;
        let Some(node) = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("drawing"))
        else {
            continue;
        };
        let Some(drawing) = node
            .attribute((REL, "id"))
            .and_then(|id| relation(parts, path, id, "/drawing"))
        else {
            continue;
        };
        let doc = xml(text(parts, &drawing)?)?;
        for anchor in doc.root_element().children().filter(|n| {
            n.has_tag_name((DRAW, "oneCellAnchor")) || n.has_tag_name((DRAW, "twoCellAnchor"))
        }) {
            let Some(pic) = anchor.children().find(|n| n.has_tag_name((DRAW, "pic"))) else {
                continue;
            };
            if pic.descendants().any(|n| {
                n.has_tag_name("srcRect")
                    || n.has_tag_name("effectLst") && n.has_children()
                    || n.has_tag_name("xfrm")
                        && (n.attribute("rot").is_some_and(|v| v != "0")
                            || n.attribute("flipH") == Some("1")
                            || n.attribute("flipV") == Some("1"))
            }) {
                continue;
            }
            let Some(identity) = pic.descendants().find(|n| n.has_tag_name("cNvPr")) else {
                continue;
            };
            let Some(shape) = identity.attribute("id") else {
                continue;
            };
            let Some(from) = anchor.children().find(|n| n.has_tag_name("from")) else {
                continue;
            };
            let Some((row, col)) = number(from, "row").zip(number(from, "col")) else {
                continue;
            };
            let ext = anchor
                .children()
                .find(|n| n.has_tag_name("ext"))
                .or_else(|| {
                    pic.descendants()
                        .find(|n| n.has_tag_name("xfrm"))
                        .and_then(|n| n.children().find(|n| n.has_tag_name("ext")))
                });
            let Some((w, h)) = ext.and_then(|n| {
                n.attribute("cx")
                    .and_then(|s| s.parse::<f64>().ok())
                    .zip(n.attribute("cy").and_then(|s| s.parse::<f64>().ok()))
            }) else {
                continue;
            };
            let target = pic
                .descendants()
                .find(|n| n.has_tag_name("blip"))
                .and_then(|n| n.attribute((REL, "embed")))
                .and_then(|id| relation(parts, &drawing, id, "/image"));
            let Some(data) = target.as_ref().and_then(|path| parts.get(path)) else {
                continue;
            };
            import_image(
                state,
                index,
                data.clone(),
                [
                    row,
                    col,
                    number(from, "colOff").unwrap_or(0.0) / 12700.0,
                    number(from, "rowOff").unwrap_or(0.0) / 12700.0,
                    w / 12700.0,
                    h / 12700.0,
                ],
                identity.attribute("descr").unwrap_or("").into(),
                format!("{drawing}#{shape}"),
            )?;
        }
    }
    Ok(())
}
fn points(input: &str) -> Option<f64> {
    [
        ("pt", 1.0),
        ("in", 72.0),
        ("cm", 72.0 / 2.54),
        ("mm", 72.0 / 25.4),
        ("px", 0.75),
    ]
    .iter()
    .find_map(|(unit, scale)| {
        input
            .strip_suffix(unit)
            .and_then(|v| v.parse::<f64>().ok())
            .map(|v| v * scale)
    })
}
fn odf_cells<'a, 'b: 'a>(
    table: roxmltree::Node<'a, 'b>,
) -> Result<Vec<(usize, usize, roxmltree::Node<'a, 'b>)>, Error> {
    let mut cells = Vec::new();
    let mut row = 0usize;
    for node in table.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table-row"))
            && n.ancestors().find(|n| n.has_tag_name((TABLE, "table"))) == Some(table)
    }) {
        let count = attr(node, "number-rows-repeated")
            .map_or(Ok(1), |v| v.parse::<usize>())
            .map_err(|_| Error::Invalid("Invalid repeated rows"))?;
        let mut col = 0usize;
        for cell in node.children().filter(|n| {
            n.has_tag_name((TABLE, "table-cell")) || n.has_tag_name((TABLE, "covered-table-cell"))
        }) {
            let repeat = attr(cell, "number-columns-repeated")
                .map_or(Ok(1), |v| v.parse::<usize>())
                .map_err(|_| Error::Invalid("Invalid repeated columns"))?;
            if row < MAX_ROWS && col < MAX_COLS && count == 1 && repeat == 1 {
                cells.push((row, col, cell));
            }
            col = col.checked_add(repeat).ok_or(Error::Limit)?;
        }
        row = row.checked_add(count).ok_or(Error::Limit)?;
    }
    Ok(cells)
}
fn import_ods(parts: &Parts, state: &mut State) -> Result<(), Error> {
    use base64::Engine;
    let doc = xml(text(parts, "content.xml")?)?;
    let tables: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((TABLE, "table")))
        .collect();
    for (index, table) in tables.into_iter().enumerate().take(state.sheets.len()) {
        for (row, col, cell) in odf_cells(table)? {
            for frame in cell
                .children()
                .filter(|n| n.has_tag_name((ODF_DRAW, "frame")))
            {
                if attr(frame, "transform").is_some() {
                    continue;
                }
                let Some(name) = attr(frame, "name") else {
                    continue;
                };
                let Some(node) = frame
                    .children()
                    .find(|n| n.has_tag_name((ODF_DRAW, "image")))
                else {
                    continue;
                };
                let data = if let Some(reference) = attr(node, "href") {
                    resolve("content.xml", reference).and_then(|path| parts.get(&path).cloned())
                } else {
                    node.children()
                        .find(|n| n.has_tag_name("binary-data"))
                        .and_then(|n| n.text())
                        .filter(|data| data.len() <= 24 * 1024 * 1024)
                        .and_then(|data| {
                            let data = data.split_whitespace().collect::<String>();
                            base64::engine::general_purpose::STANDARD
                                .decode(&data)
                                .or_else(|_| {
                                    base64::engine::general_purpose::STANDARD_NO_PAD.decode(&data)
                                })
                                .ok()
                        })
                };
                let Some(data) = data else {
                    continue;
                };
                let Some((width, height)) = attr(frame, "width")
                    .and_then(points)
                    .zip(attr(frame, "height").and_then(points))
                else {
                    continue;
                };
                let description = frame
                    .children()
                    .find(|n| n.has_tag_name("desc"))
                    .and_then(|n| n.text())
                    .unwrap_or("");
                import_image(
                    state,
                    index,
                    data,
                    [
                        row as f64,
                        col as f64,
                        attr(frame, "x").and_then(points).unwrap_or(0.0),
                        attr(frame, "y").and_then(points).unwrap_or(0.0),
                        width,
                        height,
                    ],
                    description.into(),
                    format!("ods:{index}:{name}"),
                )?;
            }
        }
    }
    Ok(())
}
fn append(document: &str, content: &str) -> Result<String, Error> {
    let doc = xml(document)?;
    let root = doc.root_element();
    let range = root.range();
    let mut out = document.to_owned();
    if document[range.clone()].trim_end().ends_with("/>") {
        let end = document[..range.end]
            .rfind("/>")
            .ok_or(Error::Invalid("Invalid XML root"))?;
        let name = document[range.start + 1..]
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .ok_or(Error::Invalid("Missing XML root name"))?;
        out.replace_range(end..range.end, &format!(">{content}</{name}>"));
    } else {
        let end = document[..range.end]
            .rfind("</")
            .ok_or(Error::Invalid("Invalid XML root"))?;
        out.insert_str(end, content);
    }
    Ok(out)
}
fn add_relation(parts: &mut Parts, owner: &str, target: &str, kind: &str) -> Result<String, Error> {
    let path = rel_path(owner);
    let source = parts.get(&path).map(|data| std::str::from_utf8(data).map(str::to_owned)).transpose().map_err(|_| Error::Invalid("Invalid relationship encoding"))?.unwrap_or_else(|| "<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"/>".into());
    let d = xml(&source)?;
    let ids: BTreeSet<_> = d
        .root_element()
        .children()
        .filter_map(|n| n.attribute("Id"))
        .collect();
    let mut sequence = 1;
    while ids.contains(format!("off42Picture{sequence}").as_str()) {
        sequence += 1;
    }
    let id = format!("off42Picture{sequence}");
    let result = append(&source, &format!("<Relationship xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\" Id=\"{id}\" Type=\"{REL}/{kind}\" Target=\"{}\"/>", escape_xml(target)))?;
    parts.insert(path, result.into_bytes());
    Ok(id)
}
fn media(parts: &mut Parts, picture: &SheetImage, prefix: &str) -> String {
    let hash = Sha256::digest(&picture.data);
    let name = format!(
        "{prefix}/off42-{}.png",
        hash.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    parts.insert(name.clone(), picture.data.to_vec());
    name
}
fn anchor(p: &SheetImage, id: u64, rel: &str) -> String {
    let emu = |v: f64| (v * 12700.0).round() as u64;
    let (w, h) = (emu(p.width_points), emu(p.height_points));
    format!("<xdr:oneCellAnchor xmlns:xdr=\"{DRAW}\" xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\" xmlns:r=\"{REL}\"><xdr:from><xdr:col>{}</xdr:col><xdr:colOff>{}</xdr:colOff><xdr:row>{}</xdr:row><xdr:rowOff>{}</xdr:rowOff></xdr:from><xdr:ext cx=\"{w}\" cy=\"{h}\"/><xdr:pic><xdr:nvPicPr><xdr:cNvPr id=\"{id}\" name=\"off42-picture-{}\" descr=\"{}\"/><xdr:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></xdr:cNvPicPr></xdr:nvPicPr><xdr:blipFill><a:blip r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></xdr:blipFill><xdr:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></xdr:spPr></xdr:pic><xdr:clientData/></xdr:oneCellAnchor>", p.column, emu(p.offset_x), p.row, emu(p.offset_y), p.id, escape_xml(&p.description))
}
pub(super) fn patch(
    bytes: &[u8],
    kind: &str,
    old: &State,
    current: &State,
) -> Result<Vec<u8>, Error> {
    if current.sheets.iter().all(|s| {
        old.sheets
            .iter()
            .find(|o| o.id == s.id)
            .is_some_and(|o| o.images == s.images)
    }) {
        return Ok(bytes.to_vec());
    }
    let mut parts = package(bytes)?;
    match kind {
        "xlsx" => patch_xlsx(&mut parts, old, current)?,
        "ods" => patch_ods(&mut parts, old, current)?,
        _ => return Err(Error::Invalid("This format does not support pictures")),
    }
    save(bytes, parts)
}
fn patch_xlsx(parts: &mut Parts, old: &State, current: &State) -> Result<(), Error> {
    let paths = sheets(parts)?;
    for sheet in &current.sheets {
        let before = old.sheets.iter().find(|s| s.id == sheet.id);
        let old_images: &[SheetImage] = before.map_or(&[], |s| &s.images);
        if old_images == sheet.images {
            continue;
        }
        let path = paths
            .get(&sheet.name)
            .ok_or(Error::Invalid("Missing picture worksheet"))?
            .clone();
        let source = text(parts, &path)?.to_owned();
        let doc = xml(&source)?;
        let node = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("drawing"));
        let drawing = node
            .and_then(|n| n.attribute((REL, "id")))
            .and_then(|id| relation(parts, &path, id, "/drawing"));
        let drawing = match drawing {
            Some(path) => path,
            None if node.is_some() => {
                return Err(Error::Invalid("Unsupported drawing relationship"))
            }
            None => {
                let mut sequence = 1;
                while parts.contains_key(&format!("xl/drawings/off42-{sequence}.xml")) {
                    sequence += 1;
                }
                let drawing = format!("xl/drawings/off42-{sequence}.xml");
                parts.insert(
                    drawing.clone(),
                    format!("<xdr:wsDr xmlns:xdr=\"{DRAW}\"/>").into_bytes(),
                );
                let id = add_relation(parts, &path, &format!("/{drawing}"), "drawing")?;
                let node = format!("<drawing xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"{REL}\" r:id=\"{id}\"/>");
                let boundary = doc.root_element().children().find(|n| {
                    n.is_element()
                        && matches!(
                            n.tag_name().name(),
                            "legacyDrawing"
                                | "legacyDrawingHF"
                                | "picture"
                                | "oleObjects"
                                | "controls"
                                | "webPublishItems"
                                | "tableParts"
                                | "extLst"
                        )
                });
                let changed = if let Some(boundary) = boundary {
                    let mut out = source.clone();
                    out.insert_str(boundary.range().start, &node);
                    out
                } else {
                    append(&source, &node)?
                };
                parts.insert(path.clone(), changed.into_bytes());
                let types = text(parts, "[Content_Types].xml")?.to_owned();
                parts.insert("[Content_Types].xml".into(), append(&types, &format!("<Override xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\" PartName=\"/{drawing}\" ContentType=\"application/vnd.openxmlformats-officedocument.drawing+xml\"/>"))?.into_bytes());
                drawing
            }
        };
        let source = text(parts, &drawing)?.to_owned();
        let doc = xml(&source)?;
        let mut edits = Vec::new();
        for p in old_images {
            if sheet.images.contains(p) {
                continue;
            }
            let Some(id) = p
                .source
                .as_ref()
                .and_then(|s| s.strip_prefix(&format!("{drawing}#")))
            else {
                continue;
            };
            let node = doc
                .root_element()
                .children()
                .find(|n| {
                    n.descendants()
                        .any(|n| n.has_tag_name("cNvPr") && n.attribute("id") == Some(id))
                })
                .ok_or(Error::Invalid("Original picture anchor missing"))?;
            edits.push((node.range().start, node.range().end, String::new()));
        }
        let mut changed = apply_edits(source.clone(), edits);
        let mut shape = doc
            .descendants()
            .filter(|n| n.has_tag_name("cNvPr"))
            .filter_map(|n| n.attribute("id").and_then(|v| v.parse::<u64>().ok()))
            .max()
            .unwrap_or(0);
        for p in &sheet.images {
            if !old_images.contains(p) {
                shape = shape.checked_add(1).ok_or(Error::Limit)?;
                let file = media(parts, p, "xl/media");
                let id = add_relation(parts, &drawing, &format!("/{file}"), "image")?;
                changed = append(&changed, &anchor(p, shape, &id))?;
            }
        }
        parts.insert(drawing, changed.into_bytes());
    }
    let types = text(parts, "[Content_Types].xml")?.to_owned();
    if !xml(&types)?.root_element().children().any(|n| {
        n.has_tag_name("Default")
            && n.attribute("Extension")
                .is_some_and(|v| v.eq_ignore_ascii_case("png"))
    }) {
        parts.insert("[Content_Types].xml".into(), append(&types, "<Default xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\" Extension=\"png\" ContentType=\"image/png\"/>")?.into_bytes());
    }
    Ok(())
}
fn with_repeat(
    document: &str,
    node: roxmltree::Node<'_, '_>,
    count: usize,
    name: &str,
) -> Result<String, Error> {
    let range = node.range();
    let mut raw = document[range.clone()].to_owned();
    if let Some(attribute) = node.attributes().find(|a| a.name() == name) {
        let r = if count == 1 {
            attribute.range()
        } else {
            attribute.range_value()
        };
        raw.replace_range(
            r.start - range.start..r.end - range.start,
            &if count == 1 {
                String::new()
            } else {
                count.to_string()
            },
        );
    } else if count != 1 {
        // The count attribute is only added to known table-row/table-cell elements.
        let end = raw.find('>').ok_or(Error::Invalid("Invalid ODS element"))?;
        let end = if raw[..end].ends_with('/') {
            end - 1
        } else {
            end
        };
        let namespace = node.lookup_prefix(TABLE).unwrap_or("table");
        raw.insert_str(end, &format!(" {namespace}:{name}=\"{count}\""));
    }
    Ok(raw)
}
fn append_fragment(raw: &str, content: &str) -> Result<String, Error> {
    let mut out = raw.to_owned();
    if let Some(end) = raw.trim_end().strip_suffix("/>").map(str::len) {
        let name = raw
            .trim_start_matches('<')
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .ok_or(Error::Invalid("Invalid ODS element"))?;
        out.replace_range(end..end + 2, &format!(">{content}</{name}>"));
    } else {
        let end = raw
            .rfind("</")
            .ok_or(Error::Invalid("Invalid ODS element"))?;
        out.insert_str(end, content);
    }
    Ok(out)
}
fn patch_odf_row(
    document: &str,
    node: roxmltree::Node<'_, '_>,
    targets: &BTreeMap<usize, String>,
) -> Result<String, Error> {
    let mut col = 0usize;
    let mut edits = Vec::new();
    for cell in node.children().filter(|n| {
        n.has_tag_name((TABLE, "table-cell")) || n.has_tag_name((TABLE, "covered-table-cell"))
    }) {
        let repeat = attr(cell, "number-columns-repeated")
            .map_or(Ok(1), |v| v.parse::<usize>())
            .map_err(|_| Error::Invalid("Invalid ODS repeated cell"))?;
        let end = col.checked_add(repeat).ok_or(Error::Limit)?;
        let inside: Vec<_> = targets.range(col..end).collect();
        if !inside.is_empty() {
            if cell.has_tag_name((TABLE, "covered-table-cell")) {
                return Err(Error::Invalid(
                    "Choose the top-left cell of a merged range for a picture",
                ));
            }
            if repeat > 1
                && cell
                    .descendants()
                    .any(|n| n.tag_name().namespace() == Some(ODF_DRAW))
            {
                return Err(Error::Invalid(
                    "Repeated drawing cells cannot yet be separated",
                ));
            }
            let mut replacement = String::new();
            let mut next = col;
            for (&target, content) in inside {
                if target > next {
                    replacement.push_str(&with_repeat(
                        document,
                        cell,
                        target - next,
                        "number-columns-repeated",
                    )?);
                }
                replacement.push_str(&append_fragment(
                    &with_repeat(document, cell, 1, "number-columns-repeated")?,
                    content,
                )?);
                next = target + 1;
            }
            if next < end {
                replacement.push_str(&with_repeat(
                    document,
                    cell,
                    end - next,
                    "number-columns-repeated",
                )?);
            }
            edits.push((
                cell.range().start - node.range().start,
                cell.range().end - node.range().start,
                replacement,
            ));
        }
        col = end;
    }
    let row = &document[node.range()];
    let mut changed = apply_edits(row.to_owned(), edits);
    let mut suffix = String::new();
    for (&target, content) in targets.range(col..) {
        if target > col {
            suffix.push_str(&format!(
                "<table:table-cell xmlns:table=\"{TABLE}\" table:number-columns-repeated=\"{}\"/>",
                target - col
            ));
        }
        suffix.push_str(&format!(
            "<table:table-cell xmlns:table=\"{TABLE}\">{content}</table:table-cell>"
        ));
        col = target + 1;
    }
    if !suffix.is_empty() {
        changed = append_fragment(&changed, &suffix)?;
    }
    // Retain one copy of a repeated row after its cells have been expanded.
    if let Some(attribute) = node
        .attributes()
        .find(|a| a.name() == "number-rows-repeated")
    {
        // This attribute precedes all child replacements, so its offsets remain valid.
        let range = attribute.range();
        changed.replace_range(
            range.start - node.range().start..range.end - node.range().start,
            "",
        );
    }
    Ok(changed)
}
fn patch_odf_table(
    document: &str,
    table: roxmltree::Node<'_, '_>,
    targets: &BTreeMap<usize, BTreeMap<usize, String>>,
) -> Result<String, Error> {
    let mut row = 0usize;
    let mut edits = Vec::new();
    for node in table.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table-row"))
            && n.ancestors().find(|n| n.has_tag_name((TABLE, "table"))) == Some(table)
    }) {
        let repeat = attr(node, "number-rows-repeated")
            .map_or(Ok(1), |v| v.parse::<usize>())
            .map_err(|_| Error::Invalid("Invalid ODS repeated row"))?;
        let end = row.checked_add(repeat).ok_or(Error::Limit)?;
        let inside: Vec<_> = targets.range(row..end).collect();
        if !inside.is_empty() {
            if repeat > 1
                && node
                    .descendants()
                    .any(|n| n.tag_name().namespace() == Some(ODF_DRAW))
            {
                return Err(Error::Invalid(
                    "Repeated drawing rows cannot yet be separated",
                ));
            }
            let mut replacement = String::new();
            let mut next = row;
            for (&target, cells) in inside {
                if target > next {
                    replacement.push_str(&with_repeat(
                        document,
                        node,
                        target - next,
                        "number-rows-repeated",
                    )?);
                }
                replacement.push_str(&patch_odf_row(document, node, cells)?);
                next = target + 1;
            }
            if next < end {
                replacement.push_str(&with_repeat(
                    document,
                    node,
                    end - next,
                    "number-rows-repeated",
                )?);
            }
            edits.push((
                node.range().start - table.range().start,
                node.range().end - table.range().start,
                replacement,
            ));
        }
        row = end;
    }
    let mut changed = apply_edits(document[table.range()].to_owned(), edits);
    let mut suffix = String::new();
    for (&target, cells) in targets.range(row..) {
        if target > row {
            suffix.push_str(&format!("<table:table-row xmlns:table=\"{TABLE}\" table:number-rows-repeated=\"{}\"><table:table-cell/></table:table-row>", target-row));
        }
        suffix.push_str(&format!("<table:table-row xmlns:table=\"{TABLE}\">"));
        let mut col = 0;
        for (&target_col, content) in cells {
            if target_col > col {
                suffix.push_str(&format!(
                    "<table:table-cell table:number-columns-repeated=\"{}\"/>",
                    target_col - col
                ));
            }
            suffix.push_str(&format!("<table:table-cell>{content}</table:table-cell>"));
            col = target_col + 1;
        }
        suffix.push_str("</table:table-row>");
        row = target + 1;
    }
    if !suffix.is_empty() {
        changed = append_fragment(&changed, &suffix)?;
    }
    Ok(changed)
}
fn patch_ods(parts: &mut Parts, old: &State, current: &State) -> Result<(), Error> {
    let source = text(parts, "content.xml")?.to_owned();
    let doc = xml(&source)?;
    let tables: Vec<_> = doc
        .descendants()
        .filter(|n| {
            n.has_tag_name((TABLE, "table"))
                && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
        })
        .collect();
    let mut removals = Vec::new();
    for table in &tables {
        let Some(sheet) = current
            .sheets
            .iter()
            .find(|s| Some(s.name.as_str()) == attr(*table, "name"))
        else {
            continue;
        };
        // Package tables have already been reordered. Provenance refers to the
        // imported baseline order, so resolve it through the stable sheet ID.
        let Some((index, before)) = old
            .sheets
            .iter()
            .enumerate()
            .find(|(_, s)| s.id == sheet.id)
        else {
            continue;
        };
        for picture in &before.images {
            if sheet.images.contains(picture) {
                continue;
            }
            let Some(name) = picture
                .source
                .as_ref()
                .and_then(|s| s.strip_prefix(&format!("ods:{index}:")))
            else {
                continue;
            };
            let frame = table
                .descendants()
                .find(|n| n.has_tag_name((ODF_DRAW, "frame")) && attr(*n, "name") == Some(name))
                .ok_or(Error::Invalid("Original ODS picture is missing"))?;
            removals.push((frame.range().start, frame.range().end, String::new()));
        }
    }
    let source = apply_edits(source.clone(), removals);
    let doc = xml(&source)?;
    let mut edits = Vec::new();
    let mut manifest_additions = BTreeSet::new();
    for table in doc.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table"))
            && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let Some(sheet) = current
            .sheets
            .iter()
            .find(|s| Some(s.name.as_str()) == attr(table, "name"))
        else {
            continue;
        };
        let before = old.sheets.iter().find(|s| s.id == sheet.id);
        let mut targets: BTreeMap<usize, BTreeMap<usize, String>> = BTreeMap::new();
        for p in &sheet.images {
            if before.is_some_and(|s| s.images.contains(p)) {
                continue;
            }
            let file = media(parts, p, "Pictures");
            manifest_additions.insert(file.clone());
            let frame = format!("<draw:frame xmlns:draw=\"{ODF_DRAW}\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" draw:name=\"off42-picture-{}\" svg:x=\"{}pt\" svg:y=\"{}pt\" svg:width=\"{}pt\" svg:height=\"{}pt\"><svg:desc>{}</svg:desc><draw:image xlink:href=\"{file}\" xlink:type=\"simple\"/></draw:frame>", p.id, p.offset_x, p.offset_y, p.width_points, p.height_points, escape_xml(&p.description));
            targets
                .entry(p.row)
                .or_default()
                .entry(p.column)
                .or_default()
                .push_str(&frame);
        }
        if !targets.is_empty() {
            edits.push((
                table.range().start,
                table.range().end,
                patch_odf_table(&source, table, &targets)?,
            ));
        }
    }
    let output = apply_edits(source.clone(), edits);
    xml(&output)?;
    parts.insert("content.xml".into(), output.into_bytes());
    let manifest = text(parts, "META-INF/manifest.xml")?.to_owned();
    let doc = xml(&manifest)?;
    let existing: BTreeSet<_> = doc
        .root_element()
        .children()
        .filter_map(|n| attr(n, "full-path"))
        .collect();
    let additions = manifest_additions.iter().filter(|name| !existing.contains(name.as_str())).map(|name| format!("<manifest:file-entry xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:full-path=\"{name}\" manifest:media-type=\"image/png\"/>")).collect::<String>();
    if !additions.is_empty() {
        parts.insert(
            "META-INF/manifest.xml".into(),
            append(&manifest, &additions)?.into_bytes(),
        );
    }
    Ok(())
}

// The ODS library normalizes frame child order and link attributes. Transplant the
// original drawing subtrees before checking preservation, rather than accepting a
// lossy or merely equivalent reserialization of opaque picture properties.
pub(super) fn preserve_odf_drawings(
    before: &str,
    after: &str,
    current: &State,
) -> Result<String, Error> {
    if current.sheets.iter().any(|s| s.source_structure) {
        return Ok(after.into());
    }
    let source = xml(before)?;
    let target = xml(after)?;
    let mut originals: BTreeMap<(String, usize, usize), String> = BTreeMap::new();
    for table in source.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table"))
            && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let name = attr(table, "name").ok_or(Error::Invalid("Missing ODS sheet name"))?;
        for (row, col, cell) in odf_cells(table)? {
            let mut raw = String::new();
            for node in cell
                .children()
                .filter(|n| n.tag_name().namespace() == Some(ODF_DRAW))
            {
                let mut drawing = before[node.range()].to_owned();
                let mut declarations = String::new();
                for ns in node.namespaces() {
                    if let Some(prefix) = ns.name() {
                        if target.root_element().lookup_namespace_uri(Some(prefix))
                            != Some(ns.uri())
                        {
                            let token = format!("xmlns:{prefix}=");
                            let opening_end = drawing
                                .find('>')
                                .ok_or(Error::Invalid("Invalid ODS drawing"))?;
                            if !drawing[..opening_end].contains(&token) {
                                declarations.push_str(&format!(
                                    " xmlns:{prefix}=\"{}\"",
                                    escape_xml(ns.uri())
                                ));
                            }
                        }
                    }
                }
                if !declarations.is_empty() {
                    let end = drawing
                        .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
                        .ok_or(Error::Invalid("Invalid ODS drawing"))?;
                    drawing.insert_str(end, &declarations);
                }
                raw.push_str(&drawing);
            }
            if !raw.is_empty() {
                originals.insert((name.into(), row, col), raw);
            }
        }
    }
    let mut edits = Vec::new();
    for table in target.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table"))
            && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let name = attr(table, "name").ok_or(Error::Invalid("Missing ODS sheet name"))?;
        for (row, col, cell) in odf_cells(table)? {
            let Some(original) = originals.remove(&(name.into(), row, col)) else {
                continue;
            };
            let mut raw = after[cell.range()].to_owned();
            let removals = cell
                .children()
                .filter(|n| n.tag_name().namespace() == Some(ODF_DRAW))
                .map(|n| {
                    (
                        n.range().start - cell.range().start,
                        n.range().end - cell.range().start,
                        String::new(),
                    )
                })
                .collect();
            raw = apply_edits(raw, removals);
            raw = append_fragment(&raw, &original)?;
            edits.push((cell.range().start, cell.range().end, raw));
        }
    }
    if !originals.is_empty() {
        return Err(Error::Invalid(
            "An ODS drawing anchor could not be preserved",
        ));
    }
    let output = apply_edits(after.into(), edits);
    xml(&output)?;
    Ok(output)
}
