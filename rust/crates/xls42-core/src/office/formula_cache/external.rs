//! Refresh ordinary scalar caches after an imported workbook changes, without rebuilding its document objects.
use super::*;
type Results = BTreeMap<String, formula::Calculation>;
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const ODF_TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const ODF_OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const ODF_TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
pub(super) fn refresh(bytes: Vec<u8>, book: &Workbook, kind: &str) -> Result<Vec<u8>, Error> {
    let now = chrono::Local::now().naive_local();
    let mut results = Results::new();
    for sheet in &book.history.current().sheets {
        let mut state = book.history.snapshot();
        state.active = sheet.id;
        let eval = Workbook {
            history: SnapshotHistory::new(state, 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        };
        results.insert(sheet.name.clone(), formula::calculation_at(&eval, now));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
        .map_err(|_| Error::Invalid("Invalid Office cache package"))?;
    let mut names = std::collections::BTreeSet::new();
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|_| Error::Invalid("Invalid Office entry"))?
            .name()
            .to_owned();
        if !names.insert(name) {
            return Err(Error::Invalid("Duplicate Office package entry"));
        }
    }
    let mut replacements = BTreeMap::new();
    if kind == "xlsx" {
        let workbook =
            zip_text(&mut archive, "xl/workbook.xml")?.ok_or(Error::Invalid("Missing workbook"))?;
        let rels = zip_text(&mut archive, "xl/_rels/workbook.xml.rels")?
            .ok_or(Error::Invalid("Missing workbook relationships"))?;
        let (workbook, rels) = (xml(&workbook)?, xml(&rels)?);
        let mut targets = BTreeMap::new();
        for rel in rels
            .descendants()
            .filter(|n| n.has_tag_name("Relationship"))
        {
            if rel.attribute("TargetMode") == Some("External")
                || rel
                    .attribute("Type")
                    .is_none_or(|t| !t.ends_with("/worksheet"))
            {
                continue;
            }
            if let (Some(id), Some(target)) = (rel.attribute("Id"), rel.attribute("Target")) {
                let path = if target.starts_with('/') {
                    target.trim_start_matches('/').to_string()
                } else {
                    format!("xl/{target}")
                };
                if !path.split('/').any(|p| p == "..") {
                    targets.insert(id, path);
                }
            }
        }
        for sheet in workbook
            .descendants()
            .filter(|n| n.has_tag_name((MAIN, "sheet")))
        {
            let name = sheet
                .attribute("name")
                .ok_or(Error::Invalid("Missing sheet name"))?;
            let Some(values) = results.get(name) else {
                return Err(Error::Invalid("Unknown formula cache sheet"));
            };
            let id = sheet
                .attributes()
                .find(|a| a.name() == "id" && a.namespace().is_some())
                .ok_or(Error::Invalid("Missing sheet relationship"))?
                .value();
            let path = targets
                .get(id)
                .ok_or(Error::Invalid("Unknown worksheet target"))?;
            let source =
                zip_text(&mut archive, path)?.ok_or(Error::Invalid("Missing worksheet"))?;
            replacements.insert(
                path.clone(),
                xlsx(&source, &values.values, true)?.into_bytes(),
            );
        }
    } else {
        let source =
            zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("Missing ODS content"))?;
        replacements.insert("content.xml".into(), ods(&source, &results)?.into_bytes());
    }
    let declaration = if kind == "xlsx" {
        "[Content_Types].xml"
    } else {
        "META-INF/manifest.xml"
    };
    if let Some(source) = zip_text(&mut archive, declaration)? {
        replacements.insert(
            declaration.into(),
            project_declaration(source.as_bytes(), kind, false)?,
        );
    }
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let entry = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("Invalid Office entry"))?;
        let name = entry.name().to_string();
        if name == EXTRA {
            continue;
        }
        if let Some(value) = replacements.remove(&name) {
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(entry.compression()),
                )
                .map_err(|_| Error::Invalid("Cannot write formula cache"))?;
            writer.write_all(&value)?;
        } else {
            writer
                .raw_copy_file(entry)
                .map_err(|_| Error::Invalid("Cannot retain Office entry"))?;
        }
    }
    if !replacements.is_empty() {
        return Err(Error::Invalid("Formula cache target missing"));
    }
    let output = writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot finish Office caches"))?
        .into_inner();
    if output.len() > LIMIT {
        return Err(Error::Limit);
    }
    let mut check = zip::ZipArchive::new(Cursor::new(output.as_slice()))
        .map_err(|_| Error::Invalid("Invalid refreshed Office package"))?;
    digest(&mut check)?;
    dynamic_arrays::refresh(output, book, kind, &results)
}
fn supported(value: Option<&formula::Value>) -> Option<&formula::Value> {
    value.filter(|v| !matches!(v, formula::Value::Error("#ERROR!" | "#NAME?" | "#LIMIT!")))
}
fn cache_error(code: &str) -> &str {
    match code {
        "#CYCLE!" => "#REF!",
        "#ERROR!" => "#NAME?",
        "#LIMIT!" => "#NUM!",
        code => code,
    }
}
pub(super) fn xlsx(
    source: &str,
    values: &BTreeMap<(usize, usize), formula::Value>,
    clear_unsupported: bool,
) -> Result<String, Error> {
    let document = xml(source)?;
    let mut edits = Vec::new();
    for cell in document
        .descendants()
        .filter(|n| n.has_tag_name((MAIN, "c")))
    {
        let formulas: Vec<_> = cell
            .children()
            .filter(|n| n.has_tag_name((MAIN, "f")))
            .collect();
        if formulas.len() > 1 {
            return Err(Error::Invalid("Duplicate cell formulas"));
        }
        let address = cell
            .attribute("r")
            .and_then(a1)
            .ok_or(Error::Invalid("Invalid formula address"))?;
        let Some(original) = values.get(&address) else {
            continue;
        };
        if formulas.is_empty()
            && (clear_unsupported || !matches!(original, formula::Value::Error(_)))
        {
            continue;
        }
        let value = if clear_unsupported {
            supported(Some(original))
        } else {
            Some(original)
        };
        let p = prefix(&source[cell.range()]);
        let (kind, cached) = match value {
            Some(formula::Value::Number(n)) => ("", Some(n.to_string())),
            Some(formula::Value::Boolean(b)) => ("b", Some(u8::from(*b).to_string())),
            Some(formula::Value::Text(s)) => ("str", Some(cached_text_xml(s))),
            Some(formula::Value::Error(e)) => ("e", Some(escape_xml(cache_error(e)))),
            None | Some(formula::Value::Array(_)) => ("", None),
        };
        if let Some(t) = cell.attribute_node("t") {
            edits.push((
                t.range().start,
                t.range().end,
                if kind.is_empty() {
                    String::new()
                } else {
                    format!("t=\"{kind}\"")
                },
            ));
        } else if !kind.is_empty() {
            let at = cell.range().start
                + source[cell.range()]
                    .find('>')
                    .ok_or(Error::Invalid("Invalid formula cell"))?;
            edits.push((at, at, format!(" t=\"{kind}\"")));
        }
        for old in cell
            .children()
            .filter(|n| n.has_tag_name((MAIN, "v")) || n.has_tag_name((MAIN, "is")))
        {
            edits.push((old.range().start, old.range().end, String::new()));
        }
        if let Some(value) = cached {
            let at = formulas.first().map(|f| f.range().end).unwrap_or_else(|| {
                cell.range().start + source[cell.range()].find('>').unwrap() + 1
            });
            edits.push((at, at, format!("<{p}v>{value}</{p}v>")));
        }
    }
    Ok(apply_edits(source.into(), edits))
}
fn ods(source: &str, results: &Results) -> Result<String, Error> {
    let document = xml(source)?;
    let mut edits = Vec::new();
    for table in document.descendants().filter(|n| {
        n.has_tag_name((ODF_TABLE, "table"))
            && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let name = table
            .attribute((ODF_TABLE, "name"))
            .ok_or(Error::Invalid("Missing ODS sheet name"))?;
        let Some(values) = results.get(name) else {
            return Err(Error::Invalid("Unknown ODS formula sheet"));
        };
        let mut row = 0usize;
        for node in table.descendants().filter(|n| {
            n.has_tag_name((ODF_TABLE, "table-row"))
                && n.ancestors()
                    .skip(1)
                    .find(|a| a.has_tag_name((ODF_TABLE, "table")))
                    == Some(table)
        }) {
            let repeated = repeat(node, "number-rows-repeated")?;
            let mut col = 0usize;
            for cell in node.children().filter(|n| {
                n.has_tag_name((ODF_TABLE, "table-cell"))
                    || n.has_tag_name((ODF_TABLE, "covered-table-cell"))
            }) {
                let count = repeat(cell, "number-columns-repeated")?;
                if cell.attribute((ODF_TABLE, "formula")).is_some() {
                    let value = if repeated == 1 && count == 1 {
                        supported(values.values.get(&(row, col)))
                    } else {
                        None
                    };
                    ods_cell(source, cell, value, &mut edits)?;
                }
                col = col.checked_add(count).ok_or(Error::Limit)?;
            }
            row = row.checked_add(repeated).ok_or(Error::Limit)?;
        }
    }
    Ok(apply_edits(source.into(), edits))
}
fn repeat(node: roxmltree::Node<'_, '_>, name: &str) -> Result<usize, Error> {
    match node.attribute((ODF_TABLE, name)) {
        None => Ok(1),
        Some(s) => s
            .parse::<usize>()
            .ok()
            .filter(|v| *v > 0 && *v <= 1_048_576)
            .ok_or(Error::Limit),
    }
}
fn ods_cell(
    source: &str,
    cell: roxmltree::Node<'_, '_>,
    value: Option<&formula::Value>,
    edits: &mut Vec<(usize, usize, String)>,
) -> Result<(), Error> {
    let office = cell
        .lookup_prefix(ODF_OFFICE)
        .ok_or(Error::Invalid("Missing ODS value namespace"))?;
    let plain = value.map(|v| v.clone().display()).filter(|s| {
        s.chars().all(|c| {
            matches!(c, '\t' | '\n' | '\r') || c >= ' ' && !matches!(c, '\u{fffe}' | '\u{ffff}')
        })
    });
    let attrs = match (value, plain.as_ref()) {
        (Some(formula::Value::Number(n)), Some(_)) => {
            format!(" {office}:value-type=\"float\" {office}:value=\"{n}\"")
        }
        (Some(formula::Value::Boolean(b)), Some(_)) => {
            format!(" {office}:value-type=\"boolean\" {office}:boolean-value=\"{b}\"")
        }
        (Some(_), Some(s)) => format!(
            " {office}:value-type=\"string\" {office}:string-value=\"{}\"",
            escape_xml(s)
                .replace('\n', "&#10;")
                .replace('\r', "&#13;")
                .replace('\t', "&#9;")
        ),
        _ => String::new(),
    };
    for attr in cell.attributes().filter(|a| {
        a.namespace() == Some(ODF_OFFICE)
            && matches!(
                a.name(),
                "value-type"
                    | "value"
                    | "string-value"
                    | "boolean-value"
                    | "date-value"
                    | "time-value"
                    | "currency"
            )
    }) {
        edits.push((attr.range().start, attr.range().end, String::new()));
    }
    for paragraph in cell.children().filter(|n| n.has_tag_name((ODF_TEXT, "p"))) {
        edits.push((
            paragraph.range().start,
            paragraph.range().end,
            String::new(),
        ));
    }
    let range = cell.range();
    let raw = &source[range.clone()];
    let opening = raw
        .find('>')
        .ok_or(Error::Invalid("Invalid ODS formula cell"))?;
    let paragraph = plain
        .map(|s| {
            format!(
                "<text:p xmlns:text=\"{ODF_TEXT}\">{}</text:p>",
                escape_xml(&s)
                    .replace('\r', "&#13;")
                    .replace('\n', "<text:line-break/>")
                    .replace('\t', "<text:tab/>")
            )
        })
        .unwrap_or_default();
    if raw.as_bytes().get(opening.wrapping_sub(1)) == Some(&b'/') {
        let prefix = prefix(raw);
        let at = range.start + opening - 1;
        edits.push((
            at,
            at + 2,
            format!("{attrs}>{paragraph}</{prefix}table-cell>"),
        ));
    } else {
        edits.push((range.start + opening, range.start + opening, attrs));
        edits.push((
            range.start + opening + 1,
            range.start + opening + 1,
            paragraph,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use calamine::{Data, Reader};
    fn external_bytes(book: &Workbook, kind: &str) -> Vec<u8> {
        let bytes = if kind == "xlsx" {
            book.xlsx_bytes().unwrap()
        } else {
            book.ods_bytes().unwrap()
        };
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            if file.name() != EXTRA {
                writer.raw_copy_file(file).unwrap();
            }
        }
        writer.finish().unwrap().into_inner()
    }
    #[test]
    fn external_input_edit_refreshes_typed_saved_values_history_and_cross_sheet_dependencies() {
        let mut b = Workbook::new(24, 8).unwrap();
        b.set_cell(0, 0, "1".into()).unwrap();
        b.set_cell(1, 0, "2".into()).unwrap();
        for (r, f) in [
            "=SUM(A1:A2)",
            "=IF(A1>2,\"0007\",\"0001\")",
            "=A1>2",
            "=IF(A1>2,1/0,42)",
            "=IF(A1>2,\"\",\"nonempty\")",
            "=\"X<&>\"&A1",
            "=IF(A1>2,DATE(2024,1,1),DATE(2020,1,1))",
            "=\"line1\"&CHAR(10)&\"line2\"",
        ]
        .into_iter()
        .enumerate()
        {
            b.set_cell(r, 1, f.into()).unwrap();
        }
        let first = b.active_sheet();
        let other = b.add_sheet("Other").unwrap();
        b.set_cell(0, 0, "='Sheet1'!A1*10".into()).unwrap();
        b.select_sheet(first).unwrap();
        for kind in ["xlsx", "ods"] {
            let bytes = external_bytes(&b, kind);
            let original = bytes.clone();
            let mut imported = Workbook::from_office_bytes(&bytes, kind).unwrap();
            imported.set_cell(0, 0, "4".into()).unwrap();
            let saved = if kind == "xlsx" {
                imported.xlsx_bytes().unwrap()
            } else {
                imported.ods_bytes().unwrap()
            };
            let mut reader =
                calamine::open_workbook_auto_from_rs(Cursor::new(saved.as_slice())).unwrap();
            let values = reader.worksheet_range("Sheet1").unwrap();
            for (r, wanted) in [
                Data::Float(6.0),
                Data::String("0007".into()),
                Data::Bool(true),
                Data::Error(calamine::CellErrorType::Div0),
                Data::String(String::new()),
                Data::String("X<&>4".into()),
                Data::Float(45292.0),
                Data::String("line1\nline2".into()),
            ]
            .into_iter()
            .enumerate()
            {
                let actual = values.get_value((r as u32, 1)).unwrap();
                if let (Some(a), Some(b)) = (
                    calamine::DataType::as_f64(actual),
                    calamine::DataType::as_f64(&wanted),
                ) {
                    assert_eq!(a, b, "{kind} B{}", r + 1);
                } else if kind == "ods" && r == 3 {
                    assert!(
                        matches!(actual,Data::String(s)if s=="#DIV/0!"),
                        "{actual:?}"
                    );
                } else {
                    assert_eq!(actual, &wanted, "{kind} B{}", r + 1);
                }
            }
            assert_eq!(
                calamine::DataType::as_f64(
                    reader
                        .worksheet_range("Other")
                        .unwrap()
                        .get_value((0, 0))
                        .unwrap()
                ),
                Some(40.0)
            );
            imported.undo();
            let undo = if kind == "xlsx" {
                imported.xlsx_bytes().unwrap()
            } else {
                imported.ods_bytes().unwrap()
            };
            let mut reader = calamine::open_workbook_auto_from_rs(Cursor::new(undo)).unwrap();
            assert_eq!(
                calamine::DataType::as_f64(
                    reader
                        .worksheet_range("Sheet1")
                        .unwrap()
                        .get_value((0, 1))
                        .unwrap()
                ),
                Some(3.0)
            );
            imported.redo();
            assert_eq!(imported.display_grid()[0][1], "6");
            let reopened = Workbook::from_office_bytes(&saved, kind).unwrap();
            assert_eq!(reopened.display_grid()[0][1], "6");
            assert!(reopened.sheets().iter().any(|s| s.0 == other));
            assert_eq!(bytes, original);
            let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
            assert!(archive.by_name(EXTRA).is_err());
            let part = if kind == "xlsx" {
                "[Content_Types].xml"
            } else {
                "META-INF/manifest.xml"
            };
            assert!(!zip_text(&mut archive, part)
                .unwrap()
                .unwrap()
                .contains(EXTRA));
        }
    }
    #[test]
    fn xlsx_caches_preserve_formula_metadata_and_clear_unsupported_results() {
        let source = format!(
            r#"<s:worksheet xmlns:s="{MAIN}" xmlns:q="urn:test"><s:sheetData><s:row r="1"><s:c r="A1" s="3" t="n" q:keep="yes"><s:f t="shared" si="2">UNKNOWN(1)</s:f><s:v>123456</s:v><s:extLst><q:unchanged/></s:extLst></s:c><s:c r="B1" t="str"><s:f>1+1</s:f><s:v>old</s:v></s:c></s:row></s:sheetData></s:worksheet>"#
        );
        let values = BTreeMap::from([
            ((0, 0), formula::Value::Error("#ERROR!")),
            ((0, 1), formula::Value::Text("_x0041_<&>\u{1}".into())),
        ]);
        let output = xlsx(&source, &values, true).unwrap();
        let doc = xml(&output).unwrap();
        let cells: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name((MAIN, "c")))
            .collect();
        assert!(cells[0].attribute("t").is_none());
        assert!(!cells[0].children().any(|n| n.has_tag_name((MAIN, "v"))));
        assert_eq!(cells[0].attribute(("urn:test", "keep")), Some("yes"));
        assert_eq!(cells[0].attribute("s"), Some("3"));
        assert!(output.contains("<s:f t=\"shared\" si=\"2\">UNKNOWN(1)</s:f>"));
        assert!(output.contains("<q:unchanged/>"));
        assert_eq!(
            cells[1]
                .children()
                .find(|n| n.has_tag_name((MAIN, "v")))
                .unwrap()
                .text(),
            Some("_x005F_x0041_<&>_x0001_")
        );
    }
    #[test]
    fn ods_repeated_or_unsupported_results_are_cleared_without_copying_objects() {
        let source = format!(
            r#"<o:document-content xmlns:o="{ODF_OFFICE}" xmlns:t="{ODF_TABLE}" xmlns:p="{ODF_TEXT}" xmlns:q="urn:test"><o:body><o:spreadsheet><t:table t:name="Sheet1"><t:table-row><t:table-cell t:formula="of:=1+1" o:value-type="float" o:value="987" t:number-columns-repeated="2"><p:p>987</p:p><q:keep id="one"/></t:table-cell><t:table-cell t:formula="of:=UNKNOWN()" o:value-type="float" o:value="654"/><t:table-cell t:formula="of:=2+2" o:value-type="string"><p:p>old</p:p></t:table-cell></t:table-row></t:table></o:spreadsheet></o:body></o:document-content>"#
        );
        let results = BTreeMap::from([(
            "Sheet1".into(),
            formula::Calculation {
                values: BTreeMap::from([
                    ((0, 0), formula::Value::Number(2.0)),
                    ((0, 2), formula::Value::Error("#ERROR!")),
                    ((0, 3), formula::Value::Number(4.0)),
                ]),
                spills: BTreeMap::new(),
            },
        )]);
        let output = ods(&source, &results).unwrap();
        let doc = xml(&output).unwrap();
        let cells: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name((ODF_TABLE, "table-cell")))
            .collect();
        assert_eq!(
            cells[0].attribute((ODF_TABLE, "number-columns-repeated")),
            Some("2")
        );
        assert!(cells[0].attribute((ODF_OFFICE, "value-type")).is_none());
        assert!(cells[1].attribute((ODF_OFFICE, "value-type")).is_none());
        assert_eq!(cells[2].attribute((ODF_OFFICE, "value")), Some("4"));
        assert_eq!(output.matches("<q:keep id=\"one\"/>").count(), 1);
        assert!(output.contains("of:=UNKNOWN()"));
    }
}
