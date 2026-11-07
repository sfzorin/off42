//! The native contains-text filter uses ordinary ODF database-range conditions.
//! https://docs.oasis-open.org/office/OpenDocument/v1.3/OpenDocument-v1.3-part3-schema.html
use super::*;
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
pub(super) fn database_range(
    prefix: &str,
    name: &str,
    sheet: &str,
    r: Range,
    filter: Option<&SheetFilter>,
) -> String {
    let name = escape_xml(name);
    let sheet = sheet.replace('\'', "''");
    let address = escape_xml(&format!(
        "'{sheet}'.${}${}:'{sheet}'.${}${}",
        col_name(r.left),
        r.top + 1,
        col_name(r.right),
        r.bottom + 1
    ));
    let condition=filter.map(|f|format!("<{prefix}filter><{prefix}filter-condition {prefix}field-number=\"{}\" {prefix}case-sensitive=\"false\" {prefix}data-type=\"text\" {prefix}operator=\"contains\" {prefix}value=\"{}\"/></{prefix}filter>",f.column-r.left,escape_xml(&f.contains))).unwrap_or_default();
    format!("<{prefix}database-range {prefix}name=\"{name}\" {prefix}target-range-address=\"{address}\" {prefix}contains-header=\"true\" {prefix}display-filter-buttons=\"true\">{condition}</{prefix}database-range>")
}
pub(super) fn import_ods(
    document: &roxmltree::Document<'_>,
    state: &mut State,
) -> Result<(), Error> {
    for range in document
        .descendants()
        .filter(|n| n.has_tag_name((TABLE, "database-range")))
    {
        if attr(range, "contains-header") == Some("false")
            || attr(range, "orientation") == Some("column")
        {
            continue;
        }
        let Some(filter) = range.children().find(|n| n.has_tag_name((TABLE, "filter"))) else {
            continue;
        };
        let children: Vec<_> = filter.children().filter(|n| n.is_element()).collect();
        if children.len() != 1 || !children[0].has_tag_name((TABLE, "filter-condition")) {
            continue;
        }
        let c = children[0];
        if attr(c, "operator") != Some("contains")
            || attr(c, "data-type").is_some_and(|s| s != "text")
            || attr(c, "case-sensitive").is_some_and(|s| matches!(s, "true" | "1"))
            || c.children().any(|n| n.is_element())
        {
            continue;
        }
        let Some((name, r)) = attr(range, "target-range-address").and_then(address) else {
            continue;
        };
        let Some(relative) = attr(c, "field-number").and_then(|s| s.parse::<usize>().ok()) else {
            continue;
        };
        let Some(column) = r.left.checked_add(relative) else {
            return Err(Error::Limit);
        };
        let Some(contains) = attr(c, "value") else {
            continue;
        };
        let Some(sheet) = state.sheets.iter_mut().find(|s| s.name == name) else {
            continue;
        };
        if r.bottom >= MAX_ROWS || r.right >= MAX_COLS {
            return Err(Error::Limit);
        }
        sheet.rows = sheet.rows.max(r.bottom + 1);
        sheet.cols = sheet.cols.max(r.right + 1);
        let filter = SheetFilter {
            range: r,
            column,
            contains: contains.into(),
        };
        if !crate::layout::valid_filter(&filter, sheet) || sheet.filter.is_some() {
            continue;
        }
        sheet.filter = Some(filter);
    }
    Ok(())
}
fn address(input: &str) -> Option<(String, Range)> {
    fn endpoint(input: &str) -> Option<(String, usize, usize)> {
        let (sheet, cell) = input.rsplit_once('.')?;
        let sheet = sheet.trim_start_matches('$');
        let name = if sheet.starts_with('\'') && sheet.ends_with('\'') && sheet.len() >= 2 {
            sheet[1..sheet.len() - 1].replace("''", "'")
        } else {
            sheet.into()
        };
        let (end, row, col, _, _) = parse_a1_endpoint(cell, 0)?;
        if end != cell.len() {
            return None;
        }
        Some((name, row - 1, col - 1))
    }
    let (first, last) = input.split_once(':')?;
    let (name, top, left) = endpoint(first)?;
    let (other, bottom, right) = endpoint(last)?;
    if (!other.is_empty() && name != other) || top > bottom || left > right {
        return None;
    }
    Some((
        name,
        Range {
            top,
            left,
            bottom,
            right,
        },
    ))
}

fn attr<'a, 'input>(node: roxmltree::Node<'a, 'input>, key: &str) -> Option<&'a str> {
    node.attribute((TABLE, key))
}

pub(super) fn import_xlsx(
    document: &roxmltree::Document<'_>,
    sheet: &mut crate::Sheet,
) -> Result<(), Error> {
    const NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let Some(filter) = document
        .descendants()
        .find(|n| n.has_tag_name((NS, "autoFilter")))
    else {
        return Ok(());
    };
    let columns: Vec<_> = filter.children().filter(|n| n.is_element()).collect();
    if columns.len() != 1 || !columns[0].has_tag_name((NS, "filterColumn")) {
        return Ok(());
    }
    let col = columns[0];
    let children: Vec<_> = col.children().filter(|n| n.is_element()).collect();
    if children.len() != 1 || !children[0].has_tag_name((NS, "customFilters")) {
        return Ok(());
    }
    let conditions: Vec<_> = children[0].children().filter(|n| n.is_element()).collect();
    if conditions.len() != 1 || !conditions[0].has_tag_name((NS, "customFilter")) {
        return Ok(());
    }
    let condition = conditions[0];
    if condition
        .attribute("operator")
        .is_some_and(|v| v != "equal")
    {
        return Ok(());
    }
    let Some(contains) = condition.attribute("val").and_then(contains_pattern) else {
        return Ok(());
    };
    let Some((a, b)) = filter
        .attribute("ref")
        .and_then(|s| s.split_once(':'))
        .and_then(|(a, b)| Some((a1(a)?, a1(b)?)))
    else {
        return Ok(());
    };
    let Some(relative) = col.attribute("colId").and_then(|s| s.parse::<usize>().ok()) else {
        return Ok(());
    };
    let Some(column) = a.1.checked_add(relative) else {
        return Err(Error::Limit);
    };
    if b.0 >= MAX_ROWS || b.1 >= MAX_COLS {
        return Err(Error::Limit);
    }
    sheet.rows = sheet.rows.max(b.0 + 1);
    sheet.cols = sheet.cols.max(b.1 + 1);
    let filter = SheetFilter {
        range: Range {
            top: a.0,
            left: a.1,
            bottom: b.0,
            right: b.1,
        },
        column,
        contains,
    };
    if crate::layout::valid_filter(&filter, sheet) {
        sheet.filter = Some(filter);
    }
    Ok(())
}
fn contains_pattern(value: &str) -> Option<String> {
    let inner = value.strip_prefix('*')?.strip_suffix('*')?;
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '~' => {
                let escaped = chars.next()?;
                if !matches!(escaped, '~' | '*' | '?') {
                    return None;
                }
                out.push(escaped);
            }
            '*' | '?' => return None,
            _ => out.push(ch),
        }
    }
    (!out.is_empty()).then_some(out)
}
pub(super) fn separate_xlsx_hidden(
    state: &mut State,
    cache: &BTreeMap<(SheetId, usize, usize), String>,
) {
    for sheet in &mut state.sheets {
        let Some(filter) = &sheet.filter else {
            continue;
        };
        let needle = filter.contains.to_lowercase();
        sheet.view.hidden_rows.retain(|row| {
            *row <= filter.range.top
                || *row > filter.range.bottom
                || cache
                    .get(&(sheet.id, *row, filter.column))
                    .map(String::as_str)
                    .or_else(|| {
                        sheet
                            .cells
                            .get(&(*row, filter.column))
                            .map(|c| c.text.raw())
                    })
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&needle)
        });
    }
}

pub(super) fn hidden_rows(state: &State) -> BTreeMap<SheetId, std::collections::BTreeSet<usize>> {
    let now = chrono::Local::now().naive_local();
    let mut result = BTreeMap::new();
    for sheet in state.sheets.iter().filter(|s| s.filter.is_some()) {
        let mut eval_state = state.clone();
        eval_state.active = sheet.id;
        let eval = Workbook {
            history: SnapshotHistory::new(eval_state, 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        };
        result.insert(
            sheet.id,
            formula::filter_hidden_rows_at(&eval, now, sheet.filter.as_ref().unwrap()),
        );
    }
    result
}
pub(super) fn patch_xlsx_visibility(
    text: &str,
    sheet: &crate::Sheet,
    hidden: &std::collections::BTreeSet<usize>,
) -> Result<String, Error> {
    let Some(filter) = &sheet.filter else {
        return Ok(text.into());
    };
    let document = xml(text)?;
    let data = document
        .root_element()
        .children()
        .find(|n| n.has_tag_name("sheetData"))
        .ok_or(Error::Invalid("Missing sheet data"))?;
    let mut edits = Vec::new();
    let mut absent = Vec::new();
    let rows: BTreeMap<_, _> = data
        .children()
        .filter(|n| n.has_tag_name("row"))
        .filter_map(|n| Some((n.attribute("r")?.parse::<usize>().ok()?.checked_sub(1)?, n)))
        .collect();
    let prefix = prefix(&text[data.range()]);
    for row in filter.range.top + 1..=filter.range.bottom {
        let hide = hidden.contains(&row) || sheet.view.hidden_rows.contains(&row);
        if let Some(node) = rows.get(&row) {
            if node
                .attribute("hidden")
                .is_some_and(|v| matches!(v, "1" | "true"))
                != hide
            {
                let range = node.range();
                let value = crate::xlsx_styles::attrs(
                    &text[range.clone()],
                    &[("hidden", if hide { "1" } else { "0" }.into())],
                )?;
                edits.push((range, value));
            }
        } else if hide {
            absent.push((
                row,
                format!("<{prefix}row r=\"{}\" hidden=\"1\"/>", row + 1),
            ));
        }
    }
    for (row, value) in absent {
        let pos = rows
            .range(row + 1..)
            .next()
            .map(|(_, n)| n.range().start)
            .unwrap_or_else(|| data.range().end - (prefix.len() + "</sheetData>".len()));
        if text[data.range()].trim_end().ends_with("/>") {
            return Err(Error::Invalid("Filtered source has no sheet rows"));
        }
        edits.push((pos..pos, value));
    }
    edits.sort_by_key(|(r, _)| (r.start, r.end));
    let mut output = text.to_owned();
    for (range, value) in edits.into_iter().rev() {
        output.replace_range(range, &value);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn without_private(bytes: Vec<u8>) -> Vec<u8> {
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
    fn standard_filters_survive_without_private_data_and_reapply_after_external_input_edits() {
        let mut b = Workbook::new(12, 6).unwrap();
        for (row, (number, tag)) in [
            ("Amount", "Tag"),
            ("1", "keep"),
            ("2", "keep"),
            ("3", "skip"),
            ("4", "keep"),
        ]
        .into_iter()
        .enumerate()
        {
            b.set_cell(row, 0, number.into()).unwrap();
            b.set_cell(row, 1, tag.into()).unwrap();
        }
        b.set_cell(0, 2, "=SUBTOTAL(9,A2:A5)".into()).unwrap();
        b.set_cell(1, 2, "=SUBTOTAL(109,A2:A5)".into()).unwrap();
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 4,
                right: 1,
            },
            column: 1,
            contains: "keep".into(),
        }))
        .unwrap();
        let mut view = b.sheet_view().clone();
        view.hidden_rows.insert(2);
        b.set_sheet_view(view).unwrap();
        for kind in ["xlsx", "ods"] {
            let bytes = without_private(if kind == "xlsx" {
                b.xlsx_bytes().unwrap()
            } else {
                b.ods_bytes().unwrap()
            });
            let mut external = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(external.sheet_filter(), b.sheet_filter(), "{kind}");
            assert_eq!(
                external.sheet_view().hidden_rows,
                b.sheet_view().hidden_rows,
                "{kind}"
            );
            assert_eq!(external.display_grid()[0][2], "7");
            assert_eq!(external.display_grid()[1][2], "5");
            external.set_cell(3, 1, "keep".into()).unwrap();
            assert_eq!(external.display_grid()[0][2], "10");
            assert_eq!(external.display_grid()[1][2], "8");
            external.undo();
            assert_eq!(external.display_grid()[0][2], "7");
            external.redo();
            assert_eq!(external.display_grid()[0][2], "10");
            let saved = if kind == "xlsx" {
                external.xlsx_bytes().unwrap()
            } else {
                external.ods_bytes().unwrap()
            };
            if kind == "ods" {
                let ods = spreadsheet_ods::read_ods_buf(&saved).unwrap();
                assert_eq!(
                    ods.sheet(0).row_visible(3),
                    spreadsheet_ods::sheet::Visibility::Visible
                );
                assert_eq!(
                    ods.sheet(0).row_visible(2),
                    spreadsheet_ods::sheet::Visibility::Collapsed
                );
            } else {
                let mut zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
                let text = zip_text(&mut zip, "xl/worksheets/sheet1.xml")
                    .unwrap()
                    .unwrap();
                let doc = xml(&text).unwrap();
                assert_eq!(
                    doc.descendants()
                        .find(|n| n.has_tag_name("row") && n.attribute("r") == Some("4"))
                        .unwrap()
                        .attribute("hidden"),
                    Some("0")
                );
                assert_eq!(
                    doc.descendants()
                        .find(|n| n.has_tag_name("row") && n.attribute("r") == Some("3"))
                        .unwrap()
                        .attribute("hidden"),
                    Some("1")
                );
            }
            let reopened = Workbook::from_office_bytes(&saved, kind).unwrap();
            assert_eq!(reopened.sheet_filter(), b.sheet_filter(), "{kind}");
            assert_eq!(reopened.display_grid()[0][2], "10");
            assert_eq!(reopened.display_grid()[1][2], "8");
        }
    }
    #[test]
    fn quoted_sheet_addresses_and_literal_wildcards_keep_exact_filter_identity() {
        let r = Range {
            top: 1,
            left: 3,
            bottom: 9,
            right: 5,
        };
        let f = SheetFilter {
            range: r,
            column: 4,
            contains: "a*?~<&".into(),
        };
        let text = format!(
            "<table:database-ranges xmlns:table=\"{TABLE}\">{}</table:database-ranges>",
            database_range("table:", "filter", "O'Brien.v1", r, Some(&f))
        );
        let document = xml(&text).unwrap();
        let mut b = Workbook::new(12, 8).unwrap();
        let id = b.active_sheet();
        b.rename_sheet(id, "O'Brien.v1").unwrap();
        let mut state = b.history.snapshot();
        import_ods(&document, &mut state).unwrap();
        assert_eq!(state.sheet().filter, Some(f));
        assert_eq!(contains_pattern("*a~*~?~~<&*"), Some("a*?~<&".into()));
        assert!(contains_pattern("*a?b*").is_none());
        assert!(contains_pattern("*a~*").is_none());
    }
}
