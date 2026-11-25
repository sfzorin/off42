//! Native spreadsheet table metadata, with headers kept in workbook cells.
use super::*;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpreadsheetTable {
    pub id: u64,
    pub name: String,
    pub range: Range,
    pub total_row: bool,
}

pub(super) fn validate_tables(state: &State) -> Result<(), Error> {
    let mut names = std::collections::BTreeSet::new();
    for sheet in &state.sheets {
        if sheet.tables.len() > 128 {
            return Err(Error::Limit);
        }
        for table in &sheet.tables {
            if table.id == 0
                || table.id >= state.next_id
                || !table.range.valid(sheet)
                || table.range.bottom <= table.range.top + usize::from(table.total_row)
                || table.name.is_empty()
                || table.name.len() > 255
                || matches!(table.name.to_ascii_uppercase().as_str(), "R" | "C")
                || parse_a1_endpoint(&table.name, 0).is_some_and(|v| v.0 == table.name.len())
                || !table.name.bytes().enumerate().all(|(i, c)| {
                    c.is_ascii_alphabetic() || c == b'_' || i > 0 && c.is_ascii_digit()
                })
                || !names.insert(table.name.to_ascii_lowercase())
                || sheet.merges.iter().any(|r| r.overlaps(table.range))
            {
                return Err(Error::Invalid("Invalid table name or range"));
            }
            let mut headers = std::collections::BTreeSet::new();
            for col in table.range.left..=table.range.right {
                let Some(cell) = sheet.cells.get(&(table.range.top, col)) else {
                    return Err(Error::Invalid("Table headers must be nonempty unique text"));
                };
                let CellContent::Text(text) = &cell.text else {
                    return Err(Error::Invalid("Table headers must be nonempty unique text"));
                };
                if text.raw().trim().is_empty()
                    || text.raw().contains(['[', ']'])
                    || !headers.insert(text.raw().to_lowercase())
                {
                    return Err(Error::Invalid("Table headers must be nonempty unique text"));
                }
            }
        }
        for (index, table) in sheet.tables.iter().enumerate() {
            if sheet
                .tables
                .iter()
                .skip(index + 1)
                .any(|other| table.range.overlaps(other.range))
            {
                return Err(Error::Invalid("Tables cannot overlap"));
            }
        }
    }
    Ok(())
}

pub(super) fn reconcile_headers(old: &State, current: &mut State) -> Result<(), Error> {
    validate_tables(current)?;
    let mut changes = Vec::new();
    for old_sheet in &old.sheets {
        let Some(new_sheet) = current.sheets.iter().find(|s| s.id == old_sheet.id) else {
            continue;
        };
        for old_table in &old_sheet.tables {
            let Some(table) = new_sheet.tables.iter().find(|t| t.id == old_table.id) else {
                continue;
            };
            for col in old_table.range.left..=old_table.range.right {
                let Some(previous) = old_sheet.cells.get(&(old_table.range.top, col)) else {
                    continue;
                };
                let Some(next) = new_sheet.cells.get(&(table.range.top, col)) else {
                    continue;
                };
                if previous.text.raw() != next.text.raw() {
                    changes.push((
                        format!("{}[{}]", table.name, previous.text.raw()),
                        format!("{}[{}]", table.name, next.text.raw()),
                    ));
                }
            }
        }
    }
    if !changes.is_empty() {
        for sheet in &mut current.sheets {
            for cell in sheet.cells.values_mut() {
                if matches!(cell.text, CellContent::Formula(_)) {
                    let mut value = cell.text.raw().to_owned();
                    for (index, (from, _)) in changes.iter().enumerate() {
                        value = replace_formula_token(&value, from, &format!("\u{1}{index}\u{2}"));
                    }
                    for (index, (_, to)) in changes.iter().enumerate() {
                        value = value.replace(&format!("\u{1}{index}\u{2}"), to);
                    }
                    cell.text = CellContent::from_raw(value);
                }
            }
        }
        if current.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
    }
    Ok(())
}
pub(super) fn replace_formula_token(input: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut i = 0;
    let bytes = input.as_bytes();
    let mut quoted = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            out.push('"');
            i += 1;
            if quoted && bytes.get(i) == Some(&b'"') {
                out.push('"');
                i += 1;
            } else {
                quoted = !quoted;
            }
            continue;
        }
        let boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric() && bytes[i - 1] != b'_';
        if !quoted
            && boundary
            && input
                .get(i..i + from.len())
                .is_some_and(|part| part.eq_ignore_ascii_case(from))
        {
            out.push_str(to);
            i += from.len();
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}
impl Workbook {
    pub(super) fn validate_external_table_edit(&self, state: &State) -> Result<(), Error> {
        let Some(baseline) = self.external_baseline.as_ref() else {
            return Ok(());
        };
        if self.external_format.as_deref() != Some("xlsx") {
            return Ok(());
        }
        for source in &baseline.sheets {
            if let Some(sheet) = state.sheets.iter().find(|s| s.id == source.id) {
                for range in &source.protected_formula_ranges {
                    if source
                        .cells
                        .keys()
                        .chain(sheet.cells.keys())
                        .any(|&(row, col)| {
                            row >= range.top
                                && row <= range.bottom
                                && col >= range.left
                                && col <= range.right
                                && source.cells.get(&(row, col)).map(|c| &c.text)
                                    != sheet.cells.get(&(row, col)).map(|c| &c.text)
                        })
                    {
                        return Err(Error::Invalid(
                            "Editing shared or array formulas is not yet supported safely",
                        ));
                    }
                    if source.merges.iter().chain(&sheet.merges).any(|merge| {
                        merge.overlaps(*range)
                            && source.merges.contains(merge) != sheet.merges.contains(merge)
                    }) {
                        return Err(Error::Invalid(
                            "Merging shared or array formulas is not supported",
                        ));
                    }
                }
                if source.shared_formula_cells.iter().any(|coord| {
                    source.cells.get(coord).map(|c| &c.text)
                        != sheet.cells.get(coord).map(|c| &c.text)
                }) {
                    self.validate_external_rename(state)?;
                }
                for range in &source.opaque_table_headers {
                    for col in range.left..=range.right {
                        if source.cells.get(&(range.top, col)).map(|c| &c.text)
                            != sheet.cells.get(&(range.top, col)).map(|c| &c.text)
                        {
                            return Err(Error::Invalid("Editing headers of unsupported external tables cannot yet preserve metadata"));
                        }
                    }
                }
            }
        }
        let changed = baseline.sheets.iter().any(|source| {
            state
                .sheets
                .iter()
                .find(|s| s.id == source.id)
                .is_some_and(|sheet| {
                    source.tables.iter().any(|table| {
                        sheet
                            .tables
                            .iter()
                            .find(|t| t.id == table.id)
                            .is_some_and(|next| {
                                (0..=table.range.right - table.range.left).any(|offset| {
                                    source
                                        .cells
                                        .get(&(table.range.top, table.range.left + offset))
                                        .map(|c| c.text.raw())
                                        != sheet
                                            .cells
                                            .get(&(next.range.top, next.range.left + offset))
                                            .map(|c| c.text.raw())
                                })
                            })
                    })
                })
        });
        if changed {
            self.validate_external_rename(state)?;
        }
        Ok(())
    }
    pub fn tables(&self) -> &[SpreadsheetTable] {
        &self.history.current().sheet().tables
    }
    pub fn add_table(&mut self, name: &str, range: Range, total_row: bool) -> Result<u64, Error> {
        self.require_structural_edit()?;
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(Error::Limit)?;
        state.sheet_mut().tables.push(SpreadsheetTable {
            id,
            name: name.into(),
            range,
            total_row,
        });
        validate_tables(&state)?;
        if total_row {
            let s = state.sheet_mut();
            for col in range.left..=range.right {
                if s.cells
                    .get(&(range.bottom, col))
                    .is_some_and(|c| !c.text.raw().is_empty())
                {
                    return Err(Error::Invalid("Total row must be empty"));
                }
                let column = super::office::col_name(col);
                let formula = format!("=SUM({column}{}:{column}{})", range.top + 2, range.bottom);
                s.cells.insert(
                    (range.bottom, col),
                    Cell {
                        text: CellContent::from_raw(formula),
                        style: 0,
                    },
                );
            }
        }
        self.history.apply(state);
        Ok(id)
    }
    pub fn remove_table(&mut self, id: u64) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        let sheet = state.sheet();
        let table = sheet
            .tables
            .iter()
            .find(|table| table.id == id)
            .cloned()
            .ok_or(Error::Bounds)?;
        let sheet_name = sheet.name.replace('\'', "''");
        let range = table.range;
        let reference = |top: usize, left: usize, bottom: usize, right: usize| {
            format!(
                "'{}'!${}${}:${}${}",
                sheet_name,
                super::office::col_name(left),
                top + 1,
                super::office::col_name(right),
                bottom + 1
            )
        };
        let mut replacements = vec![
            (
                format!("{}[#All]", table.name),
                reference(range.top, range.left, range.bottom, range.right),
            ),
            (
                format!("{}[#Headers]", table.name),
                reference(range.top, range.left, range.top, range.right),
            ),
        ];
        if table.total_row {
            replacements.push((
                format!("{}[#Totals]", table.name),
                reference(range.bottom, range.left, range.bottom, range.right),
            ));
        }
        for col in range.left..=range.right {
            let header = sheet
                .cells
                .get(&(range.top, col))
                .ok_or(Error::Bounds)?
                .text
                .raw();
            replacements.push((
                format!("{}[{}]", table.name, header),
                reference(
                    range.top + 1,
                    col,
                    range.bottom - usize::from(table.total_row),
                    col,
                ),
            ));
        }
        for other in &mut state.sheets {
            for cell in other.cells.values_mut() {
                if matches!(cell.text, CellContent::Formula(_)) {
                    let mut formula = cell.text.raw().to_owned();
                    for (from, to) in &replacements {
                        formula = replace_formula_token(&formula, from, to);
                    }
                    if formula
                        .to_ascii_lowercase()
                        .contains(&format!("{}[", table.name.to_ascii_lowercase()))
                    {
                        return Err(Error::Invalid(
                            "Unsupported table reference cannot be converted safely",
                        ));
                    }
                    cell.text = CellContent::from_raw(formula);
                }
            }
        }
        state
            .sheet_mut()
            .tables
            .retain(|candidate| candidate.id != id);
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        Ok(self.history.apply(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn table_history_structured_reference_and_xlsx_roundtrip() {
        let mut book = Workbook::new(8, 4).unwrap();
        for (row, value) in ["Amount", "10", "20", ""].iter().enumerate() {
            book.set_cell(row, 0, (*value).into()).unwrap();
        }
        let table = book
            .add_table(
                "Sales",
                Range {
                    top: 0,
                    left: 0,
                    bottom: 3,
                    right: 0,
                },
                true,
            )
            .unwrap();
        book.set_cell(5, 0, "=SUM(Sales[Amount])".into()).unwrap();
        assert_eq!(book.display_grid()[5][0], "30");
        assert_eq!(book.display_grid()[3][0], "30");
        let project = book.project_bytes().unwrap();
        let project = Workbook::from_project_bytes(&project).unwrap();
        assert_eq!(project.tables()[0].name, "Sales");
        let bytes = book.xlsx_bytes().unwrap();
        let reopened = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert_eq!(reopened.tables()[0].name, "Sales");
        assert_eq!(reopened.display_grid()[5][0], "30");
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        assert!(archive.by_name("xl/tables/table1.xml").is_ok());
        let ods = book.ods_bytes().unwrap();
        let mut ods_archive = zip::ZipArchive::new(std::io::Cursor::new(ods.as_slice())).unwrap();
        let mut content = String::new();
        std::io::Read::read_to_string(
            &mut ods_archive.by_name("content.xml").unwrap(),
            &mut content,
        )
        .unwrap();
        assert!(content.contains("database-range"));
        let independent = spreadsheet_ods::read_ods_buf(&ods).unwrap();
        assert_eq!(independent.num_sheets(), 1);
        let reopened_ods = Workbook::from_office_bytes(&ods, "ods").unwrap();
        assert_eq!(reopened_ods.tables()[0].name, "Sales");
        assert_eq!(reopened_ods.display_grid()[5][0], "30");
        book.remove_table(table).unwrap();
        assert!(book.tables().is_empty());
        assert_eq!(book.display_grid()[5][0], "30");
        assert!(book.undo());
        assert_eq!(book.tables().len(), 1);
        book.set_cell(0, 0, "Revenue".into()).unwrap();
        assert_eq!(book.cell(5, 0), Some("=SUM(Sales[Revenue])"));
        assert_eq!(book.display_grid()[5][0], "30");
    }
}
