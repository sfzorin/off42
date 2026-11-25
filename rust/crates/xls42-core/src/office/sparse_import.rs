//! Streaming XLSX cells; distant coordinates never create a dense bounding rectangle.
use super::*;
use calamine::{DataRef, Reader, Xlsx};
type Cache = BTreeMap<(SheetId, usize, usize), String>;
pub(super) fn xlsx(
    bytes: &[u8],
    modern_errors: &BTreeMap<(String, usize, usize), String>,
) -> Result<(Vec<Sheet>, Cache), Error> {
    let mut book =
        Xlsx::new(Cursor::new(bytes)).map_err(|_| Error::Invalid("invalid XLSX workbook"))?;
    let names = book.sheet_names().to_vec();
    if names.is_empty() || names.len() > MAX_SHEETS {
        return Err(Error::Limit);
    }
    let mut sheets = Vec::new();
    let mut cache = Cache::new();
    let mut total = 0usize;
    let mut records = 0usize;
    for (index, name) in names.into_iter().enumerate() {
        let mut sheet = Sheet::new(index as u64 + 1, name, 20, 8);
        let mut reader = book
            .worksheet_cells_reader(&sheet.name)
            .map_err(|_| Error::Invalid("cannot stream XLSX sheet"))?;
        while let Some(cell) = reader
            .next_cell_with_formula()
            .map_err(|_| Error::Invalid("cannot stream XLSX cell"))?
        {
            records += 1;
            if records > 1_000_000 {
                return Err(Error::Limit);
            }
            let (r, c) = (cell.pos.0 as usize, cell.pos.1 as usize);
            if r >= MAX_ROWS || c >= MAX_COLS {
                return Err(Error::Limit);
            }
            sheet.rows = sheet.rows.max(r + 1);
            sheet.cols = sheet.cols.max(c + 1);
            let value = match cell.value {
                DataRef::Empty => None,
                DataRef::Int(v) => Some(CellContent::Number(TextValue::from_raw(v.to_string()))),
                DataRef::Float(v) if v.is_finite() => {
                    Some(CellContent::Number(TextValue::from_raw(v.to_string())))
                }
                DataRef::Float(_) => return Err(Error::Invalid("invalid numeric value")),
                DataRef::String(v) => Some(CellContent::Text(TextValue::from_raw(v))),
                DataRef::SharedString(v) => Some(CellContent::Text(TextValue::from_raw(v.into()))),
                DataRef::Bool(v) => Some(CellContent::Boolean(TextValue::from_raw(v.to_string()))),
                DataRef::DateTime(v) => Some(CellContent::Date(TextValue::from_raw(
                    v.as_datetime()
                        .map(|d| d.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
                        .unwrap_or_else(|| v.as_f64().to_string()),
                ))),
                DataRef::DateTimeIso(v) | DataRef::DurationIso(v) => {
                    Some(CellContent::Date(TextValue::from_raw(v)))
                }
                DataRef::Error(v) => Some(CellContent::Error(TextValue::from_raw(
                    modern_errors
                        .get(&(sheet.name.clone(), r, c))
                        .cloned()
                        .unwrap_or_else(|| v.to_string()),
                ))),
            };
            if let Some(v) = &value {
                total = total.checked_add(v.raw().len()).ok_or(Error::Limit)?;
            }
            let content = if let Some(formula) = cell.formula {
                if let Some(value) = value {
                    cache.insert((sheet.id, r, c), value.raw().into());
                }
                let text = xlsx_formula_to_a1(&formula);
                total = total.checked_add(text.len()).ok_or(Error::Limit)?;
                Some(CellContent::Formula(TextValue::from_raw(text)))
            } else {
                value
            };
            if total > MAX_TEXT {
                return Err(Error::Limit);
            }
            if let Some(text) = content {
                sheet.cells.insert((r, c), Cell { text, style: 0 });
            }
        }
        sheets.push(sheet);
    }
    Ok((sheets, cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn independent_sparse_xlsx_edges_keep_types_caches_history_and_unknown_parts() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let s = source.add_worksheet();
        s.write_number(0, 0, 2).unwrap();
        s.write_string(0, 1, "=literal").unwrap();
        s.write_boolean(1, 1, true).unwrap();
        s.write_formula(
            (MAX_ROWS - 1) as u32,
            (MAX_COLS - 1) as u16,
            rust_xlsxwriter::Formula::new("=A1*3").set_result("6"),
        )
        .unwrap();
        let mut zip =
            zip::ZipWriter::new_append(Cursor::new(source.save_to_buffer().unwrap())).unwrap();
        zip.start_file(
            "customXml/opaque.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"preserve exact unknown bytes").unwrap();
        let original = zip.finish().unwrap().into_inner();
        let mut b = Workbook::from_office_bytes(&original, "xlsx").unwrap();
        assert_eq!((b.rows(), b.cols()), (MAX_ROWS, MAX_COLS));
        assert_eq!(b.display_cells().len(), 4);
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "6");
        assert!(matches!(b.content(0,1),Some(CellContent::Text(v))if v.raw()=="=literal"));
        assert!(matches!(b.content(1, 1), Some(CellContent::Boolean(_))));
        b.set_cell(0, 0, "4".into()).unwrap();
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "12");
        let changed = b.xlsx_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(changed.as_slice())).unwrap();
        let mut opaque = String::new();
        zip.by_name("customXml/opaque.txt")
            .unwrap()
            .read_to_string(&mut opaque)
            .unwrap();
        assert_eq!(opaque, "preserve exact unknown bytes");
        let reopened = Workbook::from_office_bytes(&changed, "xlsx").unwrap();
        assert_eq!(reopened.display_cells(), b.display_cells());
        assert!(b.undo());
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "6");
        assert!(b.redo());
        assert_eq!(b.display_cells()[&(MAX_ROWS - 1, MAX_COLS - 1)], "12");
        assert_eq!(b.external_source.as_deref(), Some(original.as_slice()));
    }
}
