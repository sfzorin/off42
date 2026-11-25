use super::{
    append_element_content, escape, target, valid_notes, xml, Cursor, ZipArchive, MAX_TEXT,
};
use roxmltree::{Document, Node};

const P: &str = "http://schemas.openxmlformats.org/presentationml/2006/main";
const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const PKG: &str = "http://schemas.openxmlformats.org/package/2006/relationships";
const GROUP: &str =
    "<p:nvGrpSpPr><p:cNvPr id=\"1\" name=\"\"/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr/>";
const PRESENTATION: &str = "ppt/presentation.xml";
pub(super) const PRESENTATION_RELS: &str = "ppt/_rels/presentation.xml.rels";

pub(super) struct Master {
    path: String,
    body_index: usize,
    image_index: Option<usize>,
    theme: Option<String>,
}

impl Master {
    pub(super) fn plan(
        package: &mut ZipArchive<Cursor<&[u8]>>,
        presentation: Node<'_, '_>,
        relationships: Node<'_, '_>,
        theme: Option<String>,
    ) -> Option<Self> {
        if !presentation.has_tag_name((P, "presentation"))
            || !relationships.has_tag_name((PKG, "Relationships"))
        {
            return None;
        }
        let mut lists = presentation
            .children()
            .filter(|n| n.has_tag_name((P, "notesMasterIdLst")));
        let list = lists.next();
        let mut links = relationships.children().filter(|n| {
            n.has_tag_name((PKG, "Relationship"))
                && n.attribute("Type")
                    == Some(concat!(
                        "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
                        "/notesMaster"
                    ))
        });
        let link = links.next();
        if lists.next().is_some() || links.next().is_some() {
            return None;
        }
        match (list, link) {
            (None, None) => Some(Self {
                path: unused_part(package, "notesMasters", "off42NotesMaster"),
                body_index: 1,
                image_index: Some(2),
                theme: Some(theme?),
            }),
            (Some(list), Some(link))
                if link
                    .attribute("TargetMode")
                    .is_none_or(|mode| mode == "Internal") =>
            {
                let mut ids = list
                    .children()
                    .filter(|n| n.has_tag_name((P, "notesMasterId")));
                let id = ids.next()?.attribute((R, "id"))?;
                if ids.next().is_some() || link.attribute("Id") != Some(id) {
                    return None;
                }
                let path = target(PRESENTATION, link.attribute("Target")?)?;
                let bytes = super::read(package, &path).ok()?;
                let document = xml(&bytes).ok()?;
                if !document.root_element().has_tag_name((P, "notesMaster")) {
                    return None;
                }
                let index = |kind| -> Option<Option<usize>> {
                    let mut items = document.descendants().filter(|node| {
                        node.has_tag_name((P, "ph")) && node.attribute("type") == Some(kind)
                    });
                    let value = match items.next() {
                        Some(node) => {
                            Some(node.attribute("idx").unwrap_or("0").parse::<u32>().ok()? as usize)
                        }
                        None => None,
                    };
                    items.next().is_none().then_some(value)
                };
                Some(Self {
                    path,
                    body_index: index("body")??,
                    image_index: index("sldImg")?,
                    theme: None,
                })
            }
            _ => None,
        }
    }

    pub(super) fn add_to_presentation(
        &self,
        presentation: &[u8],
        relationships: &[u8],
        added: &mut Vec<(String, Vec<u8>)>,
        overrides: &mut String,
    ) -> Result<Option<(Vec<u8>, Vec<u8>)>, String> {
        let Some(theme) = &self.theme else {
            return Ok(None);
        };
        let root = xml(presentation)?;
        let root = root.root_element();
        let offset = root
            .children()
            .find(|n| n.has_tag_name((P, "sldMasterIdLst")))
            .map(|node| node.range().end)
            .or_else(|| {
                root.children()
                    .find(|n| n.is_element())
                    .map(|node| node.range().start)
            })
            .ok_or("PPTX presentation elements are missing")?;
        let document = xml(relationships)?;
        let id = unused_id(document.root_element(), "off42NotesMaster");
        let mut presentation =
            String::from_utf8(presentation.to_vec()).map_err(|error| error.to_string())?;
        presentation.insert_str(offset, &format!("<p:notesMasterIdLst xmlns:p=\"{P}\" xmlns:r=\"{R}\"><p:notesMasterId r:id=\"{id}\"/></p:notesMasterIdLst>"));
        let mut relationships =
            String::from_utf8(relationships.to_vec()).map_err(|error| error.to_string())?;
        append_element_content(
            &mut relationships,
            &relationship(
                &id,
                "notesMaster",
                self.path
                    .strip_prefix("ppt/")
                    .ok_or("Invalid notes master path")?,
            ),
        )?;
        overrides.push_str(&content_type(&self.path, "notesMaster"));
        added.push((self.path.clone(), master()));
        added.push((
            part_relationships(&self.path),
            relationship_document(&relationship("theme", "theme", &format!("../../{theme}"))),
        ));
        Ok(Some((
            presentation.into_bytes(),
            relationships.into_bytes(),
        )))
    }

    pub(super) fn add_to_slide(
        &self,
        path: String,
        slide_path: &str,
        notes: &str,
        slide_rels: Option<&[u8]>,
        added: &mut Vec<(String, Vec<u8>)>,
        overrides: &mut String,
    ) -> Result<Vec<u8>, String> {
        let (directory, _) = slide_path
            .rsplit_once('/')
            .ok_or("Invalid PPTX slide path")?;
        let root = "../".repeat(directory.split('/').count());
        let id = match slide_rels {
            Some(bytes) => {
                let document = xml(bytes)?;
                let root = document.root_element();
                if !root.has_tag_name((PKG, "Relationships")) {
                    return Err("Unsupported PPTX slide relationships".into());
                }
                unused_id(root, "off42Notes")
            }
            None => "off42Notes".into(),
        };
        let link = relationship(&id, "notesSlide", &format!("{root}{path}"));
        let rels = match slide_rels {
            Some(bytes) => {
                let mut updated =
                    String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string())?;
                append_element_content(&mut updated, &link)?;
                updated.into_bytes()
            }
            None => relationship_document(&link),
        };
        let links = relationship("master", "notesMaster", &format!("../../{}", self.path))
            + &relationship("slide", "slide", &format!("../../{slide_path}"));
        overrides.push_str(&content_type(&path, "notesSlide"));
        added.push((part_relationships(&path), relationship_document(&links)));
        added.push((
            path,
            slide_with_indices(notes, self.body_index, self.image_index),
        ));
        Ok(rels)
    }
}

fn part_relationships(path: &str) -> String {
    let (dir, name) = path.rsplit_once('/').expect("generated PPTX part path");
    format!("{dir}/_rels/{name}.rels")
}

pub(super) fn unused_part(package: &ZipArchive<Cursor<&[u8]>>, dir: &str, stem: &str) -> String {
    let mut path = format!("ppt/{dir}/{stem}.xml");
    let mut index = 2;
    while package.index_for_name(&path).is_some()
        || package.index_for_name(&part_relationships(&path)).is_some()
    {
        path = format!("ppt/{dir}/{stem}_{index}.xml");
        index += 1;
    }
    path
}

fn unused_id(root: Node<'_, '_>, base: &str) -> String {
    let mut id = base.to_owned();
    while root
        .children()
        .any(|node| node.attribute("Id") == Some(id.as_str()))
    {
        id.push('_');
    }
    id
}

fn relationship(id: &str, kind: &str, destination: &str) -> String {
    format!(
        "<Relationship xmlns=\"{PKG}\" Id=\"{id}\" Type=\"{R}/{kind}\" Target=\"{}\"/>",
        escape(destination)
    )
}

fn relationship_document(contents: &str) -> Vec<u8> {
    format!("<Relationships xmlns=\"{PKG}\">{contents}</Relationships>").into_bytes()
}

fn content_type(path: &str, kind: &str) -> String {
    format!("<Override xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\" PartName=\"/{path}\" ContentType=\"application/vnd.openxmlformats-officedocument.presentationml.{kind}+xml\"/>")
}

fn body<'a, 'input>(document: &'a Document<'input>) -> Result<Node<'a, 'input>, String> {
    if !document.root_element().has_tag_name((P, "notes")) {
        return Err("Unsupported PPTX notes part".into());
    }
    let mut bodies = document
        .descendants()
        .filter(|n| n.has_tag_name((P, "sp")))
        .filter(|shape| {
            shape
                .children()
                .find(|n| n.has_tag_name((P, "nvSpPr")))
                .and_then(|nv| nv.children().find(|n| n.has_tag_name((P, "nvPr"))))
                .and_then(|nv| nv.children().find(|n| n.has_tag_name((P, "ph"))))
                .is_some_and(|ph| ph.attribute("type") == Some("body"))
        })
        .flat_map(|shape| shape.children().filter(|n| n.has_tag_name((P, "txBody"))));
    let body = bodies.next().ok_or("PPTX speaker-notes body is missing")?;
    if bodies.next().is_some() {
        return Err("PPTX contains multiple speaker-notes bodies".into());
    }
    Ok(body)
}

fn paragraph(node: Node<'_, '_>, notes: &mut String) -> bool {
    for part in node.descendants().skip(1) {
        if part.has_tag_name((A, "t")) {
            notes.push_str(part.text().unwrap_or_default());
        } else if part.has_tag_name((A, "br")) {
            notes.push('\n');
        }
        if notes.len() > MAX_TEXT {
            return false;
        }
    }
    true
}

fn simple(body: Node<'_, '_>) -> bool {
    fn metadata(node: Node<'_, '_>) -> bool {
        node.attributes().all(|a| {
            a.namespace().is_none() && matches!(a.name(), "lang" | "altLang" | "dirty" | "smtClean")
        }) && !node.children().any(|n| n.is_element())
    }
    body.children().filter(|n| n.is_element()).all(|node| {
        if node.has_tag_name((A, "bodyPr")) || node.has_tag_name((A, "lstStyle")) {
            return true;
        }
        if !node.has_tag_name((A, "p")) || node.attributes().len() != 0 {
            return false;
        }
        let mut runs = 0;
        node.children().filter(|n| n.is_element()).all(|child| {
            if child.has_tag_name((A, "endParaRPr")) {
                return metadata(child);
            }
            if !child.has_tag_name((A, "r")) {
                return false;
            }
            runs += 1;
            runs == 1
                && child.children().filter(|n| n.is_element()).all(|part| {
                    part.has_tag_name((A, "t")) || part.has_tag_name((A, "rPr")) && metadata(part)
                })
        })
    })
}

pub(super) fn read(data: &[u8]) -> Result<(String, bool), String> {
    let document = xml(data)?;
    let body = body(&document)?;
    let mut notes = String::new();
    for (index, node) in body
        .children()
        .filter(|n| n.has_tag_name((A, "p")))
        .enumerate()
    {
        if index > 0 {
            notes.push('\n');
        }
        if !paragraph(node, &mut notes) {
            return Err("PPTX speaker notes exceed the size limit".into());
        }
    }
    if !valid_notes(&notes) {
        return Err("Unsupported PPTX speaker-notes text".into());
    }
    Ok((notes, simple(body)))
}

fn paragraphs(notes: &str) -> String {
    notes
        .split('\n')
        .map(|line| {
            if line.is_empty() {
                format!("<a:p xmlns:a=\"{A}\"/>")
            } else {
                format!(
                    "<a:p xmlns:a=\"{A}\"><a:r><a:t>{}</a:t></a:r></a:p>",
                    escape(line)
                )
            }
        })
        .collect()
}

pub(super) fn patch(bytes: &[u8], notes: &str) -> Result<Vec<u8>, String> {
    let document = xml(bytes)?;
    let body = body(&document)?;
    if !simple(body) {
        return Err("This PPTX speaker-notes formatting cannot be safely edited".into());
    }
    let mut text = std::str::from_utf8(bytes)
        .map_err(|error| error.to_string())?
        .to_owned();
    let nodes: Vec<_> = body
        .children()
        .filter(|n| n.has_tag_name((A, "p")))
        .collect();
    let replacement = paragraphs(notes);
    if let (Some(first), Some(last)) = (nodes.first(), nodes.last()) {
        text.replace_range(first.range().start..last.range().end, &replacement);
    } else {
        let range = body.range();
        let mut body = text[range.clone()].to_owned();
        append_element_content(&mut body, &replacement)?;
        text.replace_range(range, &body);
    }
    Ok(text.into_bytes())
}

fn placeholder(
    id: usize,
    name: &str,
    kind: &str,
    index: usize,
    properties: &str,
    body: &str,
) -> String {
    format!("<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name}\"/><p:cNvSpPr/><p:nvPr><p:ph type=\"{kind}\" idx=\"{index}\"/></p:nvPr></p:nvSpPr><p:spPr>{properties}</p:spPr>{body}</p:sp>")
}

fn notes_body(notes: &str) -> String {
    format!(
        "<p:txBody><a:bodyPr/><a:lstStyle/>{}</p:txBody>",
        paragraphs(notes)
    )
}

pub(super) fn slide(notes: &str) -> Vec<u8> {
    slide_with_indices(notes, 1, Some(2))
}

fn slide_with_indices(notes: &str, body_index: usize, image_index: Option<usize>) -> Vec<u8> {
    let image = image_index
        .map(|index| placeholder(2, "Slide Image", "sldImg", index, "", ""))
        .unwrap_or_default();
    let body = placeholder(
        3,
        "Speaker Notes",
        "body",
        body_index,
        "",
        &notes_body(notes),
    );
    format!("<p:notes xmlns:p=\"{P}\" xmlns:a=\"{A}\"><p:cSld><p:spTree>{GROUP}{image}{body}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:notes>").into_bytes()
}

pub(super) fn master() -> Vec<u8> {
    let geometry = |y, h| {
        format!("<a:xfrm><a:off x=\"685800\" y=\"{y}\"/><a:ext cx=\"5486400\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>")
    };
    let image = placeholder(
        2,
        "Slide Image",
        "sldImg",
        2,
        &geometry(914400, 3086100),
        "",
    );
    let body = placeholder(
        3,
        "Speaker Notes",
        "body",
        1,
        &geometry(4800600, 3600450),
        &notes_body(""),
    );
    let colors = "bg1=\"lt1\" tx1=\"dk1\" bg2=\"lt2\" tx2=\"dk2\" accent1=\"accent1\" accent2=\"accent2\" accent3=\"accent3\" accent4=\"accent4\" accent5=\"accent5\" accent6=\"accent6\" hlink=\"hlink\" folHlink=\"folHlink\"";
    format!("<p:notesMaster xmlns:p=\"{P}\" xmlns:a=\"{A}\"><p:cSld><p:spTree>{GROUP}{image}{body}</p:spTree></p:cSld><p:clrMap {colors}/></p:notesMaster>").into_bytes()
}

pub(super) fn relationships(slide: usize) -> Vec<u8> {
    format!("<Relationships xmlns=\"{PKG}\"><Relationship Id=\"master\" Type=\"{R}/notesMaster\" Target=\"../notesMasters/notesMaster1.xml\"/><Relationship Id=\"slide\" Type=\"{R}/slide\" Target=\"../slides/slide{slide}.xml\"/></Relationships>").into_bytes()
}

pub(super) fn master_relationships() -> Vec<u8> {
    format!("<Relationships xmlns=\"{PKG}\"><Relationship Id=\"theme\" Type=\"{R}/theme\" Target=\"../theme/theme1.xml\"/></Relationships>").into_bytes()
}
