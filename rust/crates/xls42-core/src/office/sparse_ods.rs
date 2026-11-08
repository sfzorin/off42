//! ODF value records with bounded repetition; empty gaps never become a dense matrix.
use super::*;
pub(super) const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const OFFICE: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const TEXT: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
const CALC: &str = "urn:org:documentfoundation:names:experimental:calc:xmlns:calcext:1.0";
fn count(node: roxmltree::Node<'_, '_>, ns: &str, key: &str) -> Result<usize, Error> {
    node.attribute((ns, key))
        .unwrap_or("1")
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0 && *n <= 1_048_576)
        .ok_or(Error::Limit)
}
pub(super) fn tables<'a, 'input>(
    doc: &'a roxmltree::Document<'input>,
) -> impl Iterator<Item = roxmltree::Node<'a, 'input>> {
    doc.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table"))
            && n.parent()
                .is_some_and(|p| p.has_tag_name((OFFICE, "spreadsheet")))
    })
}
pub(super) fn visit<'a, 'input>(
    table: roxmltree::Node<'a, 'input>,
    budget: &mut usize,
    mut cell: impl FnMut(usize, usize, roxmltree::Node<'a, 'input>) -> Result<(), Error>,
) -> Result<(), Error> {
    let mut row = 0usize;
    for node in table.descendants().filter(|n| {
        n.has_tag_name((TABLE, "table-row"))
            && n.ancestors()
                .skip(1)
                .find(|p| p.has_tag_name((TABLE, "table")))
                == Some(table)
    }) {
        let rr = count(node, TABLE, "number-rows-repeated")?;
        let mut col = 0usize;
        for n in node.children().filter(|n| {
            n.has_tag_name((TABLE, "table-cell")) || n.has_tag_name((TABLE, "covered-table-cell"))
        }) {
            let cc = count(n, TABLE, "number-columns-repeated")?;
            let meaningful = n
                .attributes()
                .any(|a| !(a.namespace() == Some(TABLE) && a.name() == "number-columns-repeated"))
                || n.children().any(|c| c.is_element());
            if meaningful {
                if row.checked_add(rr).is_none_or(|v| v > MAX_ROWS)
                    || col.checked_add(cc).is_none_or(|v| v > MAX_COLS)
                {
                    return Err(Error::Limit);
                }
                *budget = budget
                    .checked_add(rr.checked_mul(cc).ok_or(Error::Limit)?)
                    .ok_or(Error::Limit)?;
                if *budget > 1_000_000 {
                    return Err(Error::Limit);
                }
                for r in row..row + rr {
                    for c in col..col + cc {
                        cell(r, c, n)?;
                    }
                }
            }
            col = col.checked_add(cc).ok_or(Error::Limit)?;
            if col > 16_384 {
                return Err(Error::Limit);
            }
        }
        row = row.checked_add(rr).ok_or(Error::Limit)?;
        if row > 1_048_576 {
            return Err(Error::Limit);
        }
    }
    Ok(())
}
fn append(out: &mut String, text: &str) -> Result<(), Error> {
    if out
        .len()
        .checked_add(text.len())
        .is_none_or(|n| n > MAX_TEXT)
    {
        return Err(Error::Limit);
    }
    out.push_str(text);
    Ok(())
}
fn text_node(node: roxmltree::Node<'_, '_>, out: &mut String, depth: usize) -> Result<(), Error> {
    if depth > 128 {
        return Err(Error::Limit);
    }
    if node.is_text() {
        return append(out, node.text().unwrap_or(""));
    }
    if node.has_tag_name((OFFICE, "annotation")) {
        return Ok(());
    }
    if node.has_tag_name((TEXT, "s")) {
        let n = count(node, TEXT, "c")?;
        if out.len().checked_add(n).is_none_or(|n| n > MAX_TEXT) {
            return Err(Error::Limit);
        }
        out.extend(std::iter::repeat_n(' ', n));
        return Ok(());
    }
    if node.has_tag_name((TEXT, "tab")) {
        return append(out, "\t");
    }
    if node.has_tag_name((TEXT, "line-break")) {
        return append(out, "\n");
    }
    for child in node.children() {
        text_node(child, out, depth + 1)?;
    }
    Ok(())
}
fn value(node: roxmltree::Node<'_, '_>) -> Result<Option<CellContent>, Error> {
    let content = if let Some(raw) = node.attribute((OFFICE, "value")) {
        let n = raw
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or(Error::Invalid("invalid ODS number"))?;
        CellContent::Number(TextValue::from_raw(n.to_string()))
    } else if let Some(raw) = node.attribute((OFFICE, "boolean-value")) {
        CellContent::Boolean(TextValue::from_raw(
            match raw {
                "true" | "TRUE" | "1" => "true",
                "false" | "FALSE" | "0" => "false",
                _ => return Err(Error::Invalid("invalid ODS boolean")),
            }
            .into(),
        ))
    } else if let Some(raw) = node
        .attribute((OFFICE, "date-value"))
        .or_else(|| node.attribute((OFFICE, "time-value")))
    {
        CellContent::Date(TextValue::from_raw(raw.into()))
    } else {
        let error = node.attribute((CALC, "value-type")) == Some("error");
        let mut text = node
            .attribute((OFFICE, "string-value"))
            .unwrap_or("")
            .to_owned();
        let paragraphs = node
            .children()
            .filter(|n| n.has_tag_name((TEXT, "p")))
            .collect::<Vec<_>>();
        if node.attribute((OFFICE, "string-value")).is_none() || error && text.is_empty() {
            for (i, p) in paragraphs.iter().enumerate() {
                if i > 0 {
                    append(&mut text, "\n")?;
                }
                text_node(*p, &mut text, 0)?;
            }
        }
        if node.attribute((OFFICE, "value-type")) != Some("string")
            && node.attribute((OFFICE, "string-value")).is_none()
            && paragraphs.is_empty()
            && !error
        {
            return Ok(None);
        }
        if error {
            CellContent::Error(TextValue::from_raw(text))
        } else {
            CellContent::Text(TextValue::from_raw(text))
        }
    };
    Ok(Some(content))
}
pub(super) fn ods(
    bytes: &[u8],
) -> Result<(Vec<Sheet>, BTreeMap<(SheetId, usize, usize), String>), Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid ODS archive"))?;
    let text = zip_text(&mut zip, "content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
    let doc = xml(&text)?;
    let mut sheets = Vec::new();
    let mut cache = BTreeMap::new();
    let mut total = 0usize;
    let mut budget = 0usize;
    for table in tables(&doc) {
        if sheets.len() >= MAX_SHEETS {
            return Err(Error::Limit);
        }
        let name = table
            .attribute((TABLE, "name"))
            .ok_or(Error::Invalid("missing ODS sheet name"))?;
        let mut sheet = Sheet::new(sheets.len() as u64 + 1, name.into(), 20, 8);
        visit(table, &mut budget, |r, c, node| {
            let height = count(node, TABLE, "number-rows-spanned")?;
            let width = count(node, TABLE, "number-columns-spanned")?;
            let rows = r
                .checked_add(height)
                .filter(|n| *n <= MAX_ROWS)
                .ok_or(Error::Limit)?;
            let cols = c
                .checked_add(width)
                .filter(|n| *n <= MAX_COLS)
                .ok_or(Error::Limit)?;
            sheet.rows = sheet.rows.max(rows);
            sheet.cols = sheet.cols.max(cols);
            let value = value(node)?;
            if let Some(value) = &value {
                total = total.checked_add(value.raw().len()).ok_or(Error::Limit)?;
            }
            let content = if let Some(raw) = node.attribute((TABLE, "formula")) {
                if let Some(value) = value {
                    cache.insert((sheet.id, r, c), value.raw().into());
                }
                let formula = ods_formula_to_a1(raw).unwrap_or_else(|| {
                    if raw.starts_with('=') {
                        raw.into()
                    } else {
                        format!("={raw}")
                    }
                });
                total = total.checked_add(formula.len()).ok_or(Error::Limit)?;
                Some(CellContent::Formula(TextValue::from_raw(formula)))
            } else {
                value
            };
            if total > MAX_TEXT {
                return Err(Error::Limit);
            }
            if let Some(text) = content {
                sheet.cells.insert((r, c), Cell { text, style: 0 });
            }
            Ok(())
        })?;
        sheets.push(sheet);
    }
    if sheets.is_empty() {
        return Err(Error::Invalid("missing ODS sheets"));
    }
    Ok((sheets, cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn package(rows: &str) -> Vec<u8> {
        let mut source = spreadsheet_ods::WorkBook::default();
        source.push_sheet(spreadsheet_ods::Sheet::new("Data"));
        let original = spreadsheet_ods::write_ods_buf(&mut source, Vec::new()).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(original.as_slice())).unwrap();
        let mut envelope = String::new();
        zip.by_name("content.xml")
            .unwrap()
            .read_to_string(&mut envelope)
            .unwrap();
        let doc = xml(&envelope).unwrap();
        let range = doc
            .descendants()
            .find(|n| n.has_tag_name((OFFICE, "body")))
            .unwrap()
            .range();
        let content = format!(
            "{}<office:body><office:spreadsheet><table:table xmlns:calcext=\"{CALC}\" table:name=\"Data\">{rows}</table:table></office:spreadsheet></office:body>{}",
            &envelope[..range.start],
            &envelope[range.end..]
        );
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            if entry.name() == "content.xml" {
                out.start_file("content.xml", zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(content.as_bytes()).unwrap();
            } else {
                out.raw_copy_file(entry).unwrap();
            }
        }
        out.start_file(
            "Pictures/opaque.bin",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        out.write_all(b"unchanged opaque data").unwrap();
        out.finish().unwrap().into_inner()
    }
    #[test]
    fn independent_odf_repetition_text_errors_and_distant_formula_keep_history_and_unknown_parts() {
        let rows = format!(
            r#"<table:table-row><table:table-cell office:value-type="float" office:value="2"/><table:table-cell office:value-type="string" table:number-columns-repeated="2"><text:p>=literal<text:s text:c="2"/>PCB<text:tab/>42<text:line-break/>next</text:p><text:p>tail</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="2"><table:table-cell office:value-type="boolean" office:boolean-value="true"/><table:table-cell office:value-type="string" calcext:value-type="error"><text:p>#N/A</text:p></table:table-cell></table:table-row><table:table-row table:number-rows-repeated="{}"><table:table-cell/></table:table-row><table:table-row><table:table-cell table:number-columns-repeated="{}"/><table:table-cell office:value-type="float" office:value="6" table:formula="of:=[.A1]*3"/></table:table-row>"#,
            MAX_ROWS - 4,
            MAX_COLS - 1
        );
        let original = package(&rows);
        let mut b = Workbook::from_office_bytes(&original, "ods").unwrap();
        assert_eq!((b.rows(), b.cols()), (MAX_ROWS, MAX_COLS));
        assert_eq!(b.cell(0, 1), Some("=literal  PCB\t42\nnext\ntail"));
        assert_eq!(b.cell(0, 1), b.cell(0, 2));
        assert!(matches!(b.content(0, 1), Some(CellContent::Text(_))));
        assert!(matches!(b.content(2, 0), Some(CellContent::Boolean(_))));
        assert!(matches!(b.content(2, 1), Some(CellContent::Error(_))));
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "6");
        b.set_cell(0, 0, "4".into()).unwrap();
        let saved = b.ods_bytes().unwrap();
        let mut package = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut content = String::new();
        package
            .by_name("content.xml")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert!(content.contains("<text:p>=literal<text:s text:c=\"2\"/>PCB<text:tab/>42<text:line-break/>next</text:p><text:p>tail</text:p>"),"Unedited rich text must remain byte-for-byte");
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.display_cells(), b.display_cells());
        assert_eq!(
            reopened.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)],
            "12"
        );
        let mut zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let mut opaque = Vec::new();
        zip.by_name("Pictures/opaque.bin")
            .unwrap()
            .read_to_end(&mut opaque)
            .unwrap();
        assert_eq!(opaque, b"unchanged opaque data");
        assert!(b.undo());
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "6");
        assert!(b.redo());
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "12");
        assert_eq!(b.external_source.as_deref(), Some(original.as_slice()));
    }
    #[test]
    fn value_only_edits_clear_formulas_and_roundtrip_dates_booleans_errors_and_literal_text() {
        let original = package(
            r#"<table:table-row><table:table-cell table:formula="of:=1+2" office:value-type="float" office:value="3"/></table:table-row>"#,
        );
        let mut b = Workbook::from_office_bytes(&original, "ods").unwrap();
        for (c, v) in [
            (0, CellContent::Text(TextValue::from_raw("=literal".into()))),
            (
                1,
                CellContent::Date(TextValue::from_raw("2025-01-02".into())),
            ),
            (2, CellContent::Boolean(TextValue::from_raw("true".into()))),
            (3, CellContent::Error(TextValue::from_raw("#N/A".into()))),
        ] {
            b.set_content(0, c, v).unwrap();
        }
        for (r,c) in [(0,6),(5,6),(8,3),(8,7)] {b.set_cell(r,c,format!("gap-{r}-{c}")).unwrap();}
        let saved = b.ods_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        for (r,c)in[(0,6),(5,6),(8,3),(8,7)]{assert_eq!(reopened.cell(r,c),Some(format!("gap-{r}-{c}").as_str()));}
        assert_eq!(reopened.cell(0, 0), Some("=literal"));
        assert!(matches!(reopened.content(0, 0), Some(CellContent::Text(_))));
        assert!(
            matches!(reopened.content(0,1),Some(CellContent::Date(v))if v.raw().starts_with("2025-01-02"))
        );
        assert!(matches!(
            reopened.content(0, 2),
            Some(CellContent::Boolean(_))
        ));
        assert!(matches!(reopened.content(0,3),Some(CellContent::Error(v))if v.raw()=="#N/A"));
        b.set_cell(0, 0, String::new()).unwrap();
        let cleared = Workbook::from_office_bytes(&b.ods_bytes().unwrap(), "ods").unwrap();
        assert_eq!(cleared.cell(0, 0), Some(""));
        assert!(cleared.content(0, 0).is_none());
    }
    #[test]
    fn invalid_and_excessive_repeats_are_refused_before_layout_expansion() {
        for rows in [
            r#"<table:table-row table:number-rows-repeated="0"><table:table-cell/></table:table-row>"#,
            r#"<table:table-row><table:table-cell table:number-columns-repeated="0"/></table:table-row>"#,
            r#"<table:table-row table:number-rows-repeated="1048577"><table:table-cell/></table:table-row>"#,
        ] {
            assert!(ods(&package(rows)).is_err());
        }
        let rows = format!(
            r#"<table:table-row table:number-rows-repeated="{}"><table:table-cell table:number-columns-repeated="{}" office:value-type="string"><text:p>x</text:p></table:table-cell></table:table-row>"#,
            1_048_576, 16_384
        );
        assert!(matches!(ods(&package(&rows)), Err(Error::Limit)));
    }
}
