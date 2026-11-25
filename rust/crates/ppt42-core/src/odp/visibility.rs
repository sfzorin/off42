use super::{append_element_content, esc, xml_name_end, Range};
use roxmltree::{Document, Node};

const STYLE: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const DRAW: &str = "urn:oasis:names:tc:opendocument:xmlns:drawing:1.0";
const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const PRESENTATION: &str = "urn:oasis:names:tc:opendocument:xmlns:presentation:1.0";

fn page_style<'a, 'input>(
    doc: &'a Document<'input>,
    scope: &str,
    name: &str,
) -> Option<Node<'a, 'input>> {
    doc.root_element()
        .children()
        .find(|node| node.has_tag_name((OFFICE, scope)))?
        .children()
        .find(|node| {
            node.has_tag_name((STYLE, "style"))
                && node.attribute((STYLE, "name")) == Some(name)
                && node.attribute((STYLE, "family")) == Some("drawing-page")
        })
}

fn properties<'a, 'input>(style: Node<'a, 'input>) -> Option<Node<'a, 'input>> {
    style
        .children()
        .find(|node| node.has_tag_name((STYLE, "drawing-page-properties")))
}

pub(super) fn read(page: Node<'_, '_>, common: Option<&Document<'_>>) -> (bool, bool) {
    let direct_hidden = page.attribute((PRESENTATION, "visibility")) == Some("hidden");
    let Some(name) = page.attribute((DRAW, "style-name")) else {
        return (direct_hidden, true);
    };
    let local = page_style(page.document(), "automatic-styles", name);
    let shared = common.and_then(|doc| page_style(doc, "styles", name));
    let editable = local.map_or(shared.is_some(), |style| {
        !style.descendants().any(|node| {
            node.attribute(("http://www.w3.org/XML/1998/namespace", "id"))
                .is_some()
        })
    });
    let mut name = name;
    for _ in 0..32 {
        let Some(style) = page_style(page.document(), "automatic-styles", name)
            .or_else(|| common.and_then(|doc| page_style(doc, "styles", name)))
        else {
            break;
        };
        if let Some(visibility) =
            properties(style).and_then(|p| p.attribute((PRESENTATION, "visibility")))
        {
            return (direct_hidden || visibility == "hidden", editable);
        }
        let Some(parent) = style.attribute((STYLE, "parent-style-name")) else {
            break;
        };
        name = parent;
    }
    (direct_hidden, editable)
}

fn namespaced_attr(node: Node<'_, '_>, uri: &str, name: &str, value: &str) -> String {
    if let Some(prefix) = node.lookup_prefix(uri).filter(|prefix| !prefix.is_empty()) {
        return format!(" {prefix}:{name}=\"{value}\"");
    }
    let prefix = (0..)
        .map(|index| format!("off42ns{index}"))
        .find(|prefix| node.lookup_namespace_uri(Some(prefix)).is_none())
        .expect("unused namespace prefix");
    format!(" xmlns:{prefix}=\"{uri}\" {prefix}:{name}=\"{value}\"")
}

pub(super) fn set_reference(
    page: Node<'_, '_>,
    name: &str,
    source: &str,
    patches: &mut Vec<(Range<usize>, String)>,
) -> Result<(), String> {
    if let Some(attribute) = page.attribute_node((DRAW, "style-name")) {
        patches.push((attribute.range_value(), esc(name)));
    } else {
        let at = xml_name_end(page, source)?;
        patches.push((at..at, namespaced_attr(page, DRAW, "style-name", name)));
    }
    Ok(())
}

pub(super) fn remove_legacy_hidden(page: Node<'_, '_>, patches: &mut Vec<(Range<usize>, String)>) {
    if let Some(attribute) = page.attribute_node((PRESENTATION, "visibility")) {
        if attribute.value() == "hidden" {
            patches.push((attribute.range(), String::new()));
        }
    }
}

pub(super) fn override_style(
    page: Node<'_, '_>,
    name: &str,
    hidden: bool,
    source: &str,
) -> Result<String, String> {
    let value = if hidden { "hidden" } else { "visible" };
    let props = format!("<style:drawing-page-properties xmlns:style=\"{STYLE}\" xmlns:presentation=\"{PRESENTATION}\" presentation:visibility=\"{value}\"/>");
    let prior = page.attribute((DRAW, "style-name"));
    let local = prior.and_then(|name| page_style(page.document(), "automatic-styles", name));
    let Some(style) = local else {
        let parent = prior
            .map(|name| format!(" style:parent-style-name=\"{}\"", esc(name)))
            .unwrap_or_default();
        return Ok(format!("<style:style xmlns:style=\"{STYLE}\" style:name=\"{name}\" style:family=\"drawing-page\"{parent}>{props}</style:style>"));
    };
    let base = style.range().start;
    let mut copy = source[style.range()].to_owned();
    if let Some(properties) = properties(style) {
        if let Some(attribute) = properties.attribute_node((PRESENTATION, "visibility")) {
            let range = attribute.range_value();
            copy.replace_range(range.start - base..range.end - base, value);
        } else {
            let at = xml_name_end(properties, source)? - base;
            copy.insert_str(
                at,
                &namespaced_attr(properties, PRESENTATION, "visibility", value),
            );
        }
    } else {
        append_element_content(&mut copy, &props)?;
    }
    let range = style
        .attribute_node((STYLE, "name"))
        .ok_or("ODP style has no name")?
        .range_value();
    copy.replace_range(range.start - base..range.end - base, name);
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::super::{export, export_overlay, import_with_source, zip_read, Presentation};
    use super::*;
    use std::io::{Cursor, Write};
    use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

    #[test]
    fn hiding_one_page_clones_shared_style_and_preserves_other_content() {
        let mut deck = Presentation::new();
        deck.add().unwrap();
        let native = export(&deck).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(native.as_slice())).unwrap();
        let xml = String::from_utf8(zip_read(&mut archive, "content.xml").unwrap()).unwrap();
        let shared_xml = xml
            .replace(
                "draw:style-name=\"suite42_page_1\"",
                "draw:style-name=\"suite42_page_0\"",
            )
            .replacen(
                "<style:drawing-page-properties ",
                "<style:drawing-page-properties draw:fill=\"solid\" draw:fill-color=\"#123456\" ",
                1,
            );
        let mut fixture = ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..archive.len() {
            let file = archive.by_index(index).unwrap();
            if file.name() == "content.xml" {
                fixture
                    .start_file(file.name(), SimpleFileOptions::default())
                    .unwrap();
                fixture.write_all(shared_xml.as_bytes()).unwrap();
            } else {
                fixture.raw_copy_file(file).unwrap();
            }
        }
        fixture
            .start_file("opaque.bin", SimpleFileOptions::default())
            .unwrap();
        fixture.write_all(b"retained opaque data").unwrap();
        let original = fixture.finish().unwrap().into_inner();
        let (mut opened, source) = import_with_source(&original).unwrap();
        assert!(source.can_edit_visibility(opened.slides()[0].id));
        assert!(!source.can_edit_timing(opened.slides()[0].id));
        opened.set_hidden(true).unwrap();
        let hidden = export_overlay(&opened, &source).unwrap();
        let (mut reopened, reopened_source) = import_with_source(&hidden).unwrap();
        assert!(reopened.slides()[0].hidden && !reopened.slides()[1].hidden);
        let mut output = ZipArchive::new(Cursor::new(hidden.as_slice())).unwrap();
        let changed = String::from_utf8(zip_read(&mut output, "content.xml").unwrap()).unwrap();
        assert_eq!(
            zip_read(&mut output, "opaque.bin").unwrap(),
            b"retained opaque data"
        );
        let input_doc = Document::parse(&shared_xml).unwrap();
        let output_doc = Document::parse(&changed).unwrap();
        let shared_style = |doc: &Document<'_>| {
            page_style(doc, "automatic-styles", "suite42_page_0")
                .unwrap()
                .range()
        };
        assert_eq!(
            &shared_xml[shared_style(&input_doc)],
            &changed[shared_style(&output_doc)]
        );
        let pages = output_doc
            .descendants()
            .filter(|n| n.has_tag_name((DRAW, "page")))
            .collect::<Vec<_>>();
        let new_style = pages[0].attribute((DRAW, "style-name")).unwrap();
        let props =
            properties(page_style(&output_doc, "automatic-styles", new_style).unwrap()).unwrap();
        assert_eq!(props.attribute((DRAW, "fill-color")), Some("#123456"));
        assert_eq!(
            props.attribute((PRESENTATION, "visibility")),
            Some("hidden")
        );
        assert_eq!(
            pages[1].attribute((DRAW, "style-name")),
            Some("suite42_page_0")
        );
        assert!(opened.undo());
        assert!(!opened.slides()[0].hidden);
        assert!(opened.redo());
        assert!(opened.slides()[0].hidden);
        reopened.set_hidden(false).unwrap();
        let visible = export_overlay(&reopened, &reopened_source).unwrap();
        assert!(!import_with_source(&visible).unwrap().0.slides()[0].hidden);
    }
}
