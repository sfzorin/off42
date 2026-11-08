//! Persist the selected worksheet without rebuilding data, styles, other views or unknown package entries.
use super::*;
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const C: &str = "urn:oasis:names:tc:opendocument:xmlns:config:1.0";
const M: &str = "urn:oasis:names:tc:opendocument:xmlns:manifest:1.0";
fn config<'a, 'input>(
    parent: roxmltree::Node<'a, 'input>,
    tag: &str,
    name: &str,
) -> Option<roxmltree::Node<'a, 'input>> {
    parent
        .children()
        .find(|n| n.has_tag_name((C, tag)) && n.attribute((C, "name")) == Some(name))
}
fn primary_view<'a, 'input>(
    doc: &'a roxmltree::Document<'input>,
) -> Option<roxmltree::Node<'a, 'input>> {
    let settings = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name((O, "settings")))?;
    let set = config(settings, "config-item-set", "ooo:view-settings")?;
    let views = config(set, "config-item-map-indexed", "Views")?;
    views
        .children()
        .find(|n| n.has_tag_name((C, "config-item-map-entry")))
}
pub(super) fn ods_active(bytes: &[u8]) -> Result<Option<String>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid ODS package"))?;
    let Some(text) = zip_text(&mut archive, "settings.xml")? else {
        return Ok(None);
    };
    let doc = xml(&text)?;
    Ok(primary_view(&doc)
        .and_then(|view| config(view, "config-item", "ActiveTable"))
        .map(|n| n.text().unwrap_or("").to_owned()))
}
fn xlsx(text: &str, index: usize) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    if !root.has_tag_name((S, "workbook")) {
        return Err(Error::Invalid("invalid XLSX workbook root"));
    }
    let views = root.children().find(|n| n.has_tag_name((S, "bookViews")));
    if let Some(view) =
        views.and_then(|n| n.children().find(|c| c.has_tag_name((S, "workbookView"))))
    {
        if let Some(attr) = view.attribute_node("activeTab") {
            if attr.value() == index.to_string() {
                return Ok(text.into());
            }
            let mut out = text.to_owned();
            out.replace_range(attr.range_value(), &index.to_string());
            return Ok(out);
        }
        let changed = dynamic_arrays::xml_nodes::edit(
            text,
            view,
            &[("", "activeTab", Some(index.to_string()))],
            &[],
            "",
        )?;
        let mut out = text.to_owned();
        out.replace_range(view.range(), &changed);
        return Ok(out);
    }
    if let Some(views) = views {
        let p = prefix(&text[views.range()]);
        return append_xml_children(
            text,
            views,
            &format!("<{p}workbookView activeTab=\"{index}\"/>"),
        );
    }
    let p = prefix(&text[root.range()]);
    let insert = format!("<{p}bookViews><{p}workbookView activeTab=\"{index}\"/></{p}bookViews>");
    let at = root
        .children()
        .find(|n| n.has_tag_name((S, "sheets")))
        .ok_or(Error::Invalid("missing XLSX sheets"))?
        .range()
        .start;
    let mut out = text.to_owned();
    out.insert_str(at, &insert);
    Ok(out)
}
fn xlsx_sheet(text: &str, selected: bool) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    if !root.has_tag_name((S, "worksheet")) {
        return Err(Error::Invalid("invalid XLSX worksheet root"));
    }
    let views = root.children().find(|n| n.has_tag_name((S, "sheetViews")));
    if let Some(view) = views.and_then(|v| {
        v.children().find(|n| {
            n.has_tag_name((S, "sheetView")) && n.attribute("workbookViewId").unwrap_or("0") == "0"
        })
    }) {
        let current = view
            .attribute("tabSelected")
            .is_some_and(|s| matches!(s, "1" | "true"));
        if current == selected {
            return Ok(text.into());
        }
        if let Some(attr) = view.attribute_node("tabSelected") {
            let mut out = text.to_owned();
            out.replace_range(attr.range_value(), if selected { "1" } else { "0" });
            return Ok(out);
        }
        let changed = dynamic_arrays::xml_nodes::edit(
            text,
            view,
            &[("", "tabSelected", Some("1".into()))],
            &[],
            "",
        )?;
        let mut out = text.to_owned();
        out.replace_range(view.range(), &changed);
        return Ok(out);
    }
    if !selected {
        return Ok(text.into());
    }
    if let Some(views) = views {
        let p = prefix(&text[views.range()]);
        return append_xml_children(
            text,
            views,
            &format!(r#"<{p}sheetView workbookViewId="0" tabSelected="1"/>"#),
        );
    }
    let p = prefix(&text[root.range()]);
    let added = format!(
        r#"<{p}sheetViews><{p}sheetView workbookViewId="0" tabSelected="1"/></{p}sheetViews>"#
    );
    let at = root
        .children()
        .find(|n| {
            n.is_element() && matches!(n.tag_name().name(), "sheetFormatPr" | "cols" | "sheetData")
        })
        .ok_or(Error::Invalid("missing XLSX sheet data"))?
        .range()
        .start;
    let mut out = text.to_owned();
    out.insert_str(at, &added);
    Ok(out)
}
fn active_item(name: &str) -> String {
    format!(
        "<config:config-item xmlns:config=\"{C}\" config:name=\"ActiveTable\" config:type=\"string\">{}</config:config-item>",
        escape_xml(name)
    )
}
fn entry(name: &str) -> String {
    format!(
        "<config:config-item-map-entry xmlns:config=\"{C}\">{}</config:config-item-map-entry>",
        active_item(name)
    )
}
fn indexed(name: &str) -> String {
    format!(
        "<config:config-item-map-indexed xmlns:config=\"{C}\" config:name=\"Views\">{}</config:config-item-map-indexed>",
        entry(name)
    )
}
fn setting_set(name: &str) -> String {
    format!(
        "<config:config-item-set xmlns:config=\"{C}\" xmlns:ooo=\"http://openoffice.org/2004/office\" config:name=\"ooo:view-settings\">{}</config:config-item-set>",
        indexed(name)
    )
}
fn ods(text: Option<&str>, name: &str, version: &str) -> Result<String, Error> {
    let Some(text) = text else {
        return Ok(format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?><office:document-settings xmlns:office=\"{O}\" office:version=\"{}\"><office:settings>{}</office:settings></office:document-settings>",
            escape_xml(version),
            setting_set(name)
        ));
    };
    let doc = xml(text)?;
    let root = doc.root_element();
    if !root.has_tag_name((O, "document-settings")) {
        return Err(Error::Invalid("invalid ODS settings root"));
    }
    if let Some(view) = primary_view(&doc) {
        if let Some(item) = config(view, "config-item", "ActiveTable") {
            if item.text() == Some(name) && item.attribute((C, "type")) == Some("string") {
                return Ok(text.into());
            }
            let mut changed = dynamic_arrays::xml_nodes::edit(
                text,
                item,
                &[(C, "type", Some("string".into()))],
                &[],
                "",
            )?;
            let opening = dynamic_arrays::xml_nodes::opening(&changed)?;
            let closing = changed
                .rfind("</")
                .ok_or(Error::Invalid("invalid ODS active-table item"))?;
            changed.replace_range(opening + 1..closing, &escape_xml(name));
            let mut out = text.to_owned();
            out.replace_range(item.range(), &changed);
            return Ok(out);
        }
        return append_xml_children(text, view, &active_item(name));
    }
    let Some(settings) = root.children().find(|n| n.has_tag_name((O, "settings"))) else {
        return append_xml_children(
            text,
            root,
            &format!(
                "<office:settings xmlns:office=\"{O}\">{}</office:settings>",
                setting_set(name)
            ),
        );
    };
    let Some(set) = config(settings, "config-item-set", "ooo:view-settings") else {
        return append_xml_children(text, settings, &setting_set(name));
    };
    let Some(views) = config(set, "config-item-map-indexed", "Views") else {
        return append_xml_children(text, set, &indexed(name));
    };
    append_xml_children(text, views, &entry(name))
}
fn manifest(text: &str) -> Result<String, Error> {
    let doc = xml(text)?;
    let root = doc.root_element();
    if !root.has_tag_name((M, "manifest")) {
        return Err(Error::Invalid("invalid ODS manifest"));
    }
    if root.children().any(|n| {
        n.has_tag_name((M, "file-entry")) && n.attribute((M, "full-path")) == Some("settings.xml")
    }) {
        return Ok(text.into());
    }
    append_xml_children(
        text,
        root,
        &format!(
            "<manifest:file-entry xmlns:manifest=\"{M}\" manifest:full-path=\"settings.xml\" manifest:media-type=\"text/xml\"/>"
        ),
    )
}
pub(super) fn patch(
    bytes: &[u8],
    kind: &str,
    old: &State,
    current: &State,
) -> Result<Vec<u8>, Error> {
    let old_index = old
        .sheets
        .iter()
        .position(|s| s.id == old.active)
        .ok_or(Error::Invalid("missing prior active sheet"))?;
    let index = current
        .sheets
        .iter()
        .position(|s| s.id == current.active)
        .ok_or(Error::Invalid("missing current active sheet"))?;
    if kind == "xlsx" && old_index == index && old.active == current.active
        || kind == "ods" && old.sheet().name == current.sheet().name
    {
        return Ok(bytes.to_vec());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid workbook package"))?;
    let mut entries = BTreeMap::new();
    if kind == "xlsx" {
        let text = zip_text(&mut archive, "xl/workbook.xml")?
            .ok_or(Error::Invalid("missing XLSX workbook"))?;
        let changed = xlsx(&text, index)?;
        if changed != text {
            entries.insert("xl/workbook.xml".into(), changed.into_bytes());
        }
        if old.active != current.active {
            let targets = dynamic_arrays::targets(&mut archive)?;
            for sheet in &current.sheets {
                let path = targets
                    .get(&sheet.name)
                    .ok_or(Error::Invalid("missing XLSX view target"))?;
                let text = zip_text(&mut archive, path)?
                    .ok_or(Error::Invalid("missing XLSX worksheet view"))?;
                let changed = xlsx_sheet(&text, sheet.id == current.active)?;
                if changed != text {
                    entries.insert(path.clone(), changed.into_bytes());
                }
            }
        }
    } else if kind == "ods" {
        let text = zip_text(&mut archive, "settings.xml")?;
        let content =
            zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
        let d = xml(&content)?;
        let version = d.root_element().attribute((O, "version")).unwrap_or("1.3");
        let changed = ods(text.as_deref(), &current.sheet().name, version)?;
        if text.as_deref() != Some(changed.as_str()) {
            entries.insert("settings.xml".into(), changed.into_bytes());
            let text = zip_text(&mut archive, "META-INF/manifest.xml")?
                .ok_or(Error::Invalid("missing ODS manifest"))?;
            let changed = manifest(&text)?;
            if changed != text {
                entries.insert("META-INF/manifest.xml".into(), changed.into_bytes());
            }
        }
    }
    dynamic_arrays::rewrite_zip(bytes, entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn parts(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = BTreeMap::new();
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            out.insert(entry.name().into(), data);
        }
        out
    }
    fn outside_sheet_views(bytes: &[u8]) -> String {
        let text = std::str::from_utf8(bytes).unwrap();
        let doc = xml(text).unwrap();
        let mut out = text.to_owned();
        let mut remove = doc
            .root_element()
            .children()
            .filter(|n| n.has_tag_name((S, "sheetViews")))
            .map(|n| n.range())
            .collect::<Vec<_>>();
        remove.sort_by_key(|r| std::cmp::Reverse(r.start));
        for r in remove {
            out.replace_range(r, "");
        }
        out
    }
    fn independent(bytes: &[u8], kind: &str, missing: bool) -> Vec<u8> {
        let mut source = parts(bytes);
        source.remove(EXTRA);
        let declaration = if kind == "xlsx" {
            "[Content_Types].xml"
        } else {
            "META-INF/manifest.xml"
        };
        source.insert(
            declaration.into(),
            project_declaration(&source[declaration], kind, false).unwrap(),
        );
        if missing && kind == "xlsx" {
            let text = String::from_utf8(source["xl/workbook.xml"].clone()).unwrap();
            let d = xml(&text).unwrap();
            let range = d
                .descendants()
                .find(|n| n.has_tag_name((S, "bookViews")))
                .unwrap()
                .range();
            let mut changed = text.clone();
            changed.replace_range(range, "");
            source.insert("xl/workbook.xml".into(), changed.into_bytes());
        } else if missing {
            source.remove("settings.xml");
            let text = String::from_utf8(source[declaration].clone()).unwrap();
            let d = xml(&text).unwrap();
            let mut remove = d
                .descendants()
                .filter(|n| {
                    n.has_tag_name((M, "file-entry"))
                        && n.attribute((M, "full-path")) == Some("settings.xml")
                })
                .map(|n| n.range())
                .collect::<Vec<_>>();
            remove.sort_by_key(|r| std::cmp::Reverse(r.start));
            let mut changed = text.clone();
            for r in remove {
                changed.replace_range(r, "");
            }
            source.insert(declaration.into(), changed.into_bytes());
        }
        if missing && kind == "xlsx" {
            for (name, data) in &mut source {
                if name.starts_with("xl/worksheets/") && name.ends_with(".xml") {
                    *data = outside_sheet_views(data).into_bytes();
                }
            }
        }
        source.insert(
            "custom/opaque.bin".into(),
            b"do not alter this data".to_vec(),
        );
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        // ODF's uncompressed mimetype stays first.
        if let Some(mime) = source.remove("mimetype") {
            writer
                .start_file(
                    "mimetype",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(&mime).unwrap();
        }
        for (name, data) in source {
            writer
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn external_tab_only_save_preserves_data_unknown_parts_and_complete_history_with_or_without_view_metadata()
     {
        let mut source = Workbook::new(20, 8).unwrap();
        source.set_cell(0, 0, "FIRST".into()).unwrap();
        source.add_sheet("Второй & 'лист' 2").unwrap();
        source.set_cell(1, 1, "SECOND".into()).unwrap();
        for (kind, native) in [
            ("xlsx", source.xlsx_bytes().unwrap()),
            ("ods", source.ods_bytes().unwrap()),
        ] {
            for missing in [false, true] {
                let original = independent(&native, kind, missing);
                let before = parts(&original);
                let mut book = Workbook::from_office_bytes(&original, kind).unwrap();
                let initial = if missing { 1 } else { 2 };
                assert_eq!(book.active_sheet(), initial);
                let inputs = book
                    .cell_inputs()
                    .map(|(s, r, c, t)| (s, r, c, t.to_owned()))
                    .collect::<Vec<_>>();
                let next = if initial == 1 { 2 } else { 1 };
                book.select_sheet(next).unwrap();
                let saved = if kind == "xlsx" {
                    book.xlsx_bytes()
                } else {
                    book.ods_bytes()
                }
                .unwrap();
                let after = parts(&saved);
                for (name, data) in &before {
                    if kind == "xlsx"
                        && name.starts_with("xl/worksheets/")
                        && name.ends_with(".xml")
                    {
                        assert_eq!(
                            outside_sheet_views(&after[name]),
                            outside_sheet_views(data),
                            "Worksheet data changed during tab selection"
                        );
                        continue;
                    }

                    if name
                        != if kind == "xlsx" {
                            "xl/workbook.xml"
                        } else {
                            "settings.xml"
                        }
                        && !(missing && name == "META-INF/manifest.xml")
                    {
                        assert_eq!(
                            after.get(name),
                            Some(data),
                            "{kind} missing={missing} altered {name}"
                        );
                    }
                }
                assert!(after.keys().all(
                    |name| before.contains_key(name) || kind == "ods" && name == "settings.xml"
                ));
                assert!(!after.contains_key(EXTRA));
                let reopened = Workbook::from_office_bytes(&saved, kind).unwrap();
                assert_eq!(reopened.active_sheet(), next);
                assert_eq!(
                    reopened
                        .cell_inputs()
                        .map(|(s, r, c, t)| (s, r, c, t.to_owned()))
                        .collect::<Vec<_>>(),
                    inputs
                );
                assert_eq!(book.external_source.as_deref(), Some(original.as_slice()));
                assert!(book.undo());
                assert_eq!(book.active_sheet(), initial);
                let undo = if kind == "xlsx" {
                    book.xlsx_bytes()
                } else {
                    book.ods_bytes()
                }
                .unwrap();
                assert_eq!(undo, original);
                assert!(book.redo());
                assert_eq!(book.active_sheet(), next);
                let redo = if kind == "xlsx" {
                    book.xlsx_bytes()
                } else {
                    book.ods_bytes()
                }
                .unwrap();
                assert_eq!(parts(&redo), after);
            }
        }
    }
    #[test]
    fn worksheet_primary_selection_preserves_panes_cells_and_secondary_view_flags() {
        let text = format!(
            r#"<worksheet xmlns="{S}"><sheetViews><sheetView workbookViewId="0" tabSelected="0" showGridLines="0"><pane xSplit="2" state="frozen"/><selection activeCell="B3" sqref="B3"/></sheetView><sheetView workbookViewId="1" tabSelected="1"/></sheetViews><sheetData><row r="1"><c r="A1"><v>42</v></c></row></sheetData></worksheet>"#
        );
        let changed = xlsx_sheet(&text, true).unwrap();
        assert_eq!(
            changed,
            text.replacen("tabSelected=\"0\"", "tabSelected=\"1\"", 1)
        );
        assert_eq!(xlsx_sheet(&changed, false).unwrap(), text);
        for body in [
            "<sheetData/>",
            "<sheetViews><sheetView workbookViewId=\"1\" tabSelected=\"1\"/></sheetViews><sheetData/>",
        ] {
            let text = format!("<worksheet xmlns=\"{S}\">{body}</worksheet>");
            let changed = xlsx_sheet(&text, true).unwrap();
            let d = xml(&changed).unwrap();
            let primary = d
                .descendants()
                .find(|n| {
                    n.has_tag_name((S, "sheetView")) && n.attribute("workbookViewId") == Some("0")
                })
                .unwrap();
            assert_eq!(primary.attribute("tabSelected"), Some("1"));
            assert_eq!(
                outside_sheet_views(changed.as_bytes()),
                outside_sheet_views(text.as_bytes())
            );
        }
    }
    #[test]
    fn existing_primary_settings_keep_other_views_and_unknown_attributes() {
        let text = format!(
            "<office:document-settings xmlns:office=\"{O}\" xmlns:q=\"{C}\"><office:settings><q:config-item-set q:name=\"ooo:view-settings\"><q:config-item-map-indexed q:name=\"Views\"><q:config-item-map-entry><q:config-item q:name=\"ActiveTable\" q:type=\"string\" extra=\"keep\">Old</q:config-item><q:config-item q:name=\"ShowGrid\" q:type=\"boolean\">false</q:config-item></q:config-item-map-entry><q:config-item-map-entry><q:config-item q:name=\"ActiveTable\" q:type=\"string\">Second view</q:config-item></q:config-item-map-entry></q:config-item-map-indexed></q:config-item-set></office:settings></office:document-settings>"
        );
        let changed = ods(Some(&text), "Лист & 'новый'", "1.3").unwrap();
        let doc = xml(&changed).unwrap();
        let primary = primary_view(&doc).unwrap();
        let active = config(primary, "config-item", "ActiveTable").unwrap();
        assert_eq!(active.text(), Some("Лист & 'новый'"));
        assert_eq!(active.attribute("extra"), Some("keep"));
        assert_eq!(
            config(primary, "config-item", "ShowGrid").unwrap().text(),
            Some("false")
        );
        assert!(changed.contains(">Second view</q:config-item>"));
        let text = format!(
            "<workbook xmlns=\"{S}\"><bookViews><workbookView activeTab=\"0\" showHorizontalScroll=\"0\"/><workbookView activeTab=\"1\"/></bookViews><sheets/></workbook>"
        );
        let changed = xlsx(&text, 2).unwrap();
        assert!(changed.contains("activeTab=\"2\" showHorizontalScroll=\"0\""));
        assert!(changed.contains("<workbookView activeTab=\"1\"/>"));
    }
}
