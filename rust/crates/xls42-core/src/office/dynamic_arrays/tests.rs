use super::*;
pub(super) fn external(bytes: Vec<u8>, kind: &str) -> (Vec<u8>, Workbook) {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut item = archive.by_index(i).unwrap();
        if item.name() == EXTRA {
            continue;
        }
        if matches!(item.name(), "[Content_Types].xml" | "META-INF/manifest.xml") {
            let mut bytes = Vec::new();
            item.read_to_end(&mut bytes).unwrap();
            let value = project_declaration(&bytes, kind, false).unwrap();
            out.start_file(item.name(), zip::write::SimpleFileOptions::default())
                .unwrap();
            out.write_all(&value).unwrap();
        } else {
            out.raw_copy_file(item).unwrap();
        }
    }
    let bytes = out.finish().unwrap().into_inner();
    let book = Workbook::from_office_bytes(&bytes, kind).unwrap();
    (bytes, book)
}
#[test]
fn standard_arrays_import_as_formulas_with_disposable_typed_children() {
    let mut b = Workbook::new(20, 16).unwrap();
    for (r, n) in ["3", "1", "2"].iter().enumerate() {
        b.set_cell(r, 0, (*n).into()).unwrap();
    }
    for (r, c, f) in [
        (0, 2, "=SORT(A1:A3)"),
        (0, 3, "=SEQUENCE(2,3)"),
        (0, 7, "=TEXTSPLIT(\"one,two;three\",\",\",\";\")"),
        (5, 7, "x"),
        (4, 7, "=SEQUENCE(2)"),
    ] {
        b.set_cell(r, c, f.into()).unwrap();
    }
    let expected = b.display_grid();
    for kind in ["xlsx", "ods"] {
        let bytes = if kind == "xlsx" {
            b.xlsx_bytes().unwrap()
        } else {
            b.ods_bytes().unwrap()
        };
        let (original, mut imported) = external(bytes, kind);
        for (r, c) in [
            (0, 2),
            (1, 2),
            (2, 2),
            (0, 3),
            (1, 5),
            (0, 7),
            (0, 8),
            (1, 7),
            (1, 8),
            (4, 7),
        ] {
            assert_eq!(
                imported.display_grid()[r][c],
                expected[r][c],
                "{kind} ({r},{c})"
            );
        }
        assert_eq!(imported.cell(1, 2), Some(""));
        assert_eq!(imported.cell(1, 8), Some(""));
        assert_eq!(imported.display_grid()[1][8], "#N/A");
        assert_eq!(imported.display_grid()[4][7], "#SPILL!");
        assert_eq!(
            imported.external_source.as_deref(),
            Some(original.as_slice())
        );
        imported.set_cell(0, 0, "4".into()).unwrap();
        assert_eq!(imported.display_grid()[2][2], "4");
        imported.undo();
        assert_eq!(imported.display_grid()[2][2], "3");
        imported.redo();
        assert_eq!(imported.display_grid()[2][2], "4");
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    fn cached(bytes: &[u8], kind: &str, r: usize, c: usize) -> Option<String> {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        if kind == "xlsx" {
            let text = zip_text(&mut zip, "xl/worksheets/sheet1.xml")
                .unwrap()
                .unwrap();
            let doc = xml(&text).unwrap();
            let node = doc.descendants().find(|n| {
                n.has_tag_name((S, "c")) && n.attribute("r").and_then(a1) == Some((r, c))
            })?;
            if node.attribute("t") == Some("inlineStr") {
                return Some(
                    node.descendants()
                        .filter(|n| n.has_tag_name((S, "t")))
                        .filter_map(|n| n.text())
                        .collect(),
                );
            }
            return node
                .children()
                .find(|n| n.has_tag_name((S, "v")))
                .and_then(|n| n.text())
                .map(str::to_string);
        }
        let text = zip_text(&mut zip, "content.xml").unwrap().unwrap();
        let doc = xml(&text).unwrap();
        let table = doc
            .descendants()
            .find(|n| n.has_tag_name((T, "table")))
            .unwrap();
        let mut row = 0;
        for node in table
            .descendants()
            .filter(|n| n.has_tag_name((T, "table-row")))
        {
            let rr = node
                .attribute((T, "number-rows-repeated"))
                .unwrap_or("1")
                .parse::<usize>()
                .unwrap();
            if (row..row + rr).contains(&r) {
                let mut col = 0;
                for cell in node
                    .children()
                    .filter(|n| n.has_tag_name((T, "table-cell")))
                {
                    let cc = cell
                        .attribute((T, "number-columns-repeated"))
                        .unwrap_or("1")
                        .parse::<usize>()
                        .unwrap();
                    if (col..col + cc).contains(&c) {
                        return ["value", "string-value", "boolean-value"]
                            .iter()
                            .find_map(|key| cell.attribute((O, *key)))
                            .map(str::to_string)
                            .or_else(|| {
                                cell.children()
                                    .find(|n| n.has_tag_name("p"))
                                    .and_then(|n| n.text())
                                    .map(str::to_string)
                            });
                    }
                    col += cc;
                }
            }
            row += rr;
        }
        None
    }
    #[test]
    fn external_array_cache_resize_block_remove_and_reopen_preserve_complete_values() {
        let mut b = Workbook::new(20, 16).unwrap();
        for (r, n) in ["3", "1", "2"].iter().enumerate() {
            b.set_cell(r, 0, (*n).into()).unwrap();
        }
        b.set_cell(0, 2, "=SORT(A1:A3)".into()).unwrap();
        b.set_cell(0, 3, "=SEQUENCE(2,3)".into()).unwrap();
        for kind in ["xlsx", "ods"] {
            let source = if kind == "xlsx" {
                b.xlsx_bytes().unwrap()
            } else {
                b.ods_bytes().unwrap()
            };
            let (_, mut doc) = super::external(source, kind);
            let save = |b: &Workbook| {
                if kind == "xlsx" {
                    b.xlsx_bytes().unwrap()
                } else {
                    b.ods_bytes().unwrap()
                }
            };
            doc.set_cell(0, 0, "4".into()).unwrap();
            let saved = save(&doc);
            assert_eq!(cached(&saved, kind, 2, 2).as_deref(), Some("4"));
            let mut doc = Workbook::from_office_bytes(&saved, kind).unwrap();
            assert_eq!(doc.display_grid()[2][2], "4");
            doc.set_cell(0, 3, "=SEQUENCE(3,2)".into()).unwrap();
            let resized = save(&doc);
            for (r, c, v) in [
                (0, 3, "1"),
                (0, 4, "2"),
                (1, 3, "3"),
                (1, 4, "4"),
                (2, 3, "5"),
                (2, 4, "6"),
            ] {
                assert_eq!(
                    cached(&resized, kind, r, c).as_deref(),
                    Some(v),
                    "{kind} {r},{c}"
                );
            }
            assert_eq!(cached(&resized, kind, 0, 5), None);
            assert_eq!(cached(&resized, kind, 1, 5), None);
            let mut doc = Workbook::from_office_bytes(&resized, kind).unwrap();
            assert_eq!(doc.cell(2, 4), Some(""));
            let baseline = doc.display_grid();
            doc.set_cell(1, 3, "stop".into()).unwrap();
            let blocked = save(&doc);
            assert_eq!(cached(&blocked, kind, 0, 3).as_deref(), Some("#SPILL!"));
            assert_eq!(cached(&blocked, kind, 1, 3).as_deref(), Some("stop"));
            for (r, c) in [(0, 4), (1, 4), (2, 3), (2, 4)] {
                assert_eq!(
                    cached(&blocked, kind, r, c),
                    None,
                    "{kind} old child {r},{c}"
                );
            }
            doc.undo();
            assert_eq!(doc.display_grid(), baseline);
            let undone = save(&doc);
            assert_eq!(cached(&undone, kind, 2, 4).as_deref(), Some("6"));
            doc.redo();
            assert_eq!(doc.display_grid()[0][3], "#SPILL!");
            doc.undo();
            doc.set_cell(0, 3, "=10+2".into()).unwrap();
            let removed = save(&doc);
            assert_eq!(cached(&removed, kind, 0, 3).as_deref(), Some("12"));
            for (r, c) in [(0, 4), (1, 3), (1, 4), (2, 3), (2, 4)] {
                assert_eq!(cached(&removed, kind, r, c), None);
            }
            let reopened = Workbook::from_office_bytes(&removed, kind).unwrap();
            assert_eq!(reopened.display_grid()[0][3], "12");
            assert_eq!(reopened.display_grid()[2][4], "");
            let mut zip = zip::ZipArchive::new(Cursor::new(removed.as_slice())).unwrap();
            assert!(zip.by_name(EXTRA).is_err());
        }
    }
}
#[cfg(test)]
mod registration_tests {
    use super::*;
    #[test]
    fn adding_arrays_to_plain_external_files_registers_standard_metadata_and_literal_text() {
        let mut b = Workbook::new(20, 10).unwrap();
        b.set_cell(0, 0, "1".into()).unwrap();
        for kind in ["xlsx", "ods"] {
            let raw = if kind == "xlsx" {
                b.xlsx_bytes().unwrap()
            } else {
                b.ods_bytes().unwrap()
            };
            let (_, mut doc) = super::external(raw, kind);
            doc.set_cell(0, 3, "=SEQUENCE(3,2)".into()).unwrap();
            doc.set_cell(5, 2, "=TEXTSPLIT(\"=1+2,0007\",\",\")".into())
                .unwrap();
            let saved = if kind == "xlsx" {
                doc.xlsx_bytes().unwrap()
            } else {
                doc.ods_bytes().unwrap()
            };
            let reopened = Workbook::from_office_bytes(&saved, kind).unwrap();
            assert_eq!(reopened.display_grid()[2][4], "6");
            assert_eq!(&reopened.display_grid()[5][2..4], &["=1+2", "0007"]);
            assert_eq!(reopened.cell(2, 4), Some(""));
            assert_eq!(reopened.cell(5, 3), Some(""));
            let mut zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
            assert!(zip.by_name(EXTRA).is_err());
            if kind == "xlsx" {
                let meta = zip_text(&mut zip, "xl/metadata.xml").unwrap().unwrap();
                assert_eq!(metadata::indices(&meta).unwrap().len(), 1);
                let sheet = zip_text(&mut zip, "xl/worksheets/sheet1.xml")
                    .unwrap()
                    .unwrap();
                assert!(sheet.contains("_xlfn.SEQUENCE(3,2)"));
                assert!(sheet.contains("ref=\"D1:E3\""));
                let types = zip_text(&mut zip, "[Content_Types].xml").unwrap().unwrap();
                assert!(types.contains("/xl/metadata.xml"));
                let rels = zip_text(&mut zip, "xl/_rels/workbook.xml.rels")
                    .unwrap()
                    .unwrap();
                assert!(rels.contains("/sheetMetadata"));
            }
        }
    }
    #[test]
    fn ods_cache_changes_materialize_repeats_without_copying_unknown_objects() {
        let source = format!(
            r#"<o:document-content xmlns:o="{O}" xmlns:t="{T}" xmlns:p="urn:oasis:names:tc:opendocument:xmlns:text:1.0" xmlns:q="urn:test"><o:body><o:spreadsheet><t:table t:name="Sheet1"><t:table-row t:number-rows-repeated="5"><t:table-cell t:number-columns-repeated="5" t:style-name="Keep" q:attr="x"><q:keep id="one"/></t:table-cell></t:table-row><t:table-row/></t:table></o:spreadsheet></o:body></o:document-content>"#
        );
        let changes = BTreeMap::from([(
            "Sheet1".into(),
            BTreeMap::from([
                (
                    (1, 2),
                    Update {
                        cache: Some(Some(formula::Value::Text("line1\nline2".into()))),
                        ..Update::default()
                    },
                ),
                (
                    (5, 1),
                    Update {
                        cache: Some(Some(formula::Value::Number(7.0))),
                        ..Update::default()
                    },
                ),
            ]),
        )]);
        let output = ods::patch(&source, &changes).unwrap();
        let doc = xml(&output).unwrap();
        let rows: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name((T, "table-row")))
            .collect();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[2].attribute((T, "number-rows-repeated")), Some("3"));
        let edited: Vec<_> = rows[1]
            .children()
            .filter(|n| n.has_tag_name((T, "table-cell")))
            .collect();
        assert_eq!(edited.len(), 3);
        assert_eq!(
            edited[1].attribute((O, "string-value")),
            Some("line1\nline2")
        );
        assert_eq!(edited[1].attribute((T, "style-name")), Some("Keep"));
        assert_eq!(edited[1].attribute(("urn:test", "attr")), Some("x"));
        assert_eq!(
            edited[1]
                .children()
                .filter(|n| n.has_tag_name(("urn:test", "keep")))
                .count(),
            1
        );
        assert_eq!(
            rows[3]
                .children()
                .find(|n| n.attribute((O, "value")) == Some("7"))
                .unwrap()
                .attribute((O, "value-type")),
            Some("float")
        );
    }
}
#[cfg(test)]
mod legacy_tests {
    use super::*;
    #[test]
    fn legacy_ods_matrix_inputs_remain_guarded_while_other_cells_stay_editable() {
        let mut b = Workbook::new(20, 8).unwrap();
        b.set_cell(0, 0, "=SEQUENCE(2)".into()).unwrap();
        let bytes = b.ods_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let content = zip_text(&mut zip, "content.xml")
            .unwrap()
            .unwrap()
            .replace("COM.MICROSOFT.SEQUENCE(2)", "1+1");
        let bytes = rewrite_zip(
            &bytes,
            BTreeMap::from([("content.xml".into(), content.into_bytes())]),
        )
        .unwrap();
        let (_, mut b) = super::external(bytes, "ods");
        let baseline = b.display_grid();
        assert!(b.set_cell(0, 0, "3".into()).is_err());
        assert!(b.set_cell(1, 0, "4".into()).is_err());
        assert_eq!(b.display_grid(), baseline);
        assert!(!b.is_dirty());
        b.set_cell(0, 1, "5".into()).unwrap();
        let saved = b.ods_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.cell(0, 1), Some("5"));
    }
}
#[cfg(test)]
mod metadata_retention_tests {
    use super::*;
    #[test]
    fn original_dynamic_metadata_record_survives_input_and_formula_edits() {
        let mut b = Workbook::new(20, 8).unwrap();
        b.set_cell(0, 0, "2".into()).unwrap();
        b.set_cell(0, 3, "=SEQUENCE(A1,2)".into()).unwrap();
        let source = b.xlsx_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let meta = zip_text(&mut zip, "xl/metadata.xml")
            .unwrap()
            .unwrap()
            .replace(
                "<cellMetadata count=\"1\"><bk><rc t=\"1\" v=\"0\"/></bk></cellMetadata>",
                &format!(
                    "<cellMetadata count=\"4\">{}</cellMetadata>",
                    "<bk><rc t=\"1\" v=\"0\"/></bk>".repeat(4)
                ),
            );
        let sheet = zip_text(&mut zip, "xl/worksheets/sheet1.xml")
            .unwrap()
            .unwrap()
            .replace("cm=\"1\"", "cm=\"4\"");
        let source = rewrite_zip(
            &source,
            BTreeMap::from([
                ("xl/metadata.xml".into(), meta.as_bytes().to_vec()),
                ("xl/worksheets/sheet1.xml".into(), sheet.into_bytes()),
            ]),
        )
        .unwrap();
        let (_, mut b) = super::external(source, "xlsx");
        for (r, c, value) in [(0, 0, "3"), (0, 3, "=SEQUENCE(4,2)")] {
            b.set_cell(r, c, value.into()).unwrap();
            let saved = b.xlsx_bytes().unwrap();
            let mut zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
            assert_eq!(
                zip_text(&mut zip, "xl/metadata.xml").unwrap().unwrap(),
                meta
            );
            let sheet = zip_text(&mut zip, "xl/worksheets/sheet1.xml")
                .unwrap()
                .unwrap();
            let doc = xml(&sheet).unwrap();
            assert_eq!(
                doc.descendants()
                    .find(|n| n.has_tag_name((S, "c")) && n.attribute("r") == Some("D1"))
                    .unwrap()
                    .attribute("cm"),
                Some("4")
            );
        }
    }
}

#[test]
fn overlapping_arrays_and_total_import_work_are_bounded() {
    let mut sheet = Sheet::new(1, "Test".into(), 1000, 100);
    let mut spans = vec![
        Span {
            anchor: (0, 0),
            range: Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 1,
            },
            dynamic: true,
            cm: Some(1),
        },
        Span {
            anchor: (0, 1),
            range: Range {
                top: 0,
                left: 1,
                bottom: 0,
                right: 1,
            },
            dynamic: true,
            cm: Some(1),
        },
    ];
    assert!(matches!(
        clear_children(&mut sheet, &spans),
        Err(Error::Invalid(_))
    ));
    spans = vec![Span {
        anchor: (0, 0),
        range: Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        },
        dynamic: true,
        cm: Some(1),
    }];
    spans.extend(std::iter::repeat_n(
        Span {
            anchor: (1, 0),
            range: Range {
                top: 1,
                left: 0,
                bottom: 999,
                right: 99,
            },
            dynamic: false,
            cm: None,
        },
        11,
    ));
    assert!(matches!(
        clear_children(&mut sheet, &spans),
        Err(Error::Limit)
    ));
}
