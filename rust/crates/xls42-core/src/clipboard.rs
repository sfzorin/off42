//! Portable typed clipboard carrying physical source coordinates and styles.
use super::*;

const MAX_CLIPBOARD_PAYLOAD: usize = 16 * 1024 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    version: u32,
    rows: Vec<usize>,
    columns: Vec<usize>,
    styles: Vec<CellStyle>,
    cells: Vec<Option<ClipboardCell>>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClipboardCell {
    content: CellContent,
    style: usize,
}
fn valid_axis(axis: &[usize], limit: usize) -> bool {
    !axis.is_empty()
        && axis.len() <= limit
        && axis.last().is_some_and(|v| *v < limit)
        && axis.windows(2).all(|v| v[0] < v[1])
}
impl Workbook {
    /// Produces both interoperable TSV and a typed source-coordinate snapshot.
    pub fn copy_clipboard_mapped(
        &self,
        rows: &[usize],
        columns: &[usize],
    ) -> Result<(String, Vec<u8>), Error> {
        if !valid_axis(rows, self.rows()) || !valid_axis(columns, self.cols()) {
            return Err(Error::Bounds);
        }
        let calculation = formula::calculation_at(self, chrono::Local::now().naive_local());
        let cells = self.display_cells_with(&calculation);
        let text = self.tsv_from_cells(rows, columns, &cells)?;
        let state = self.history.current();
        let sheet = state.sheet();
        if sheet.merges.iter().any(|range| {
            rows.iter().any(|r| *r >= range.top && *r <= range.bottom)
                && columns
                    .iter()
                    .any(|c| *c >= range.left && *c <= range.right)
        }) {
            return Err(Error::Invalid("copy involving merged cells is unsupported"));
        }
        let mut styles = Vec::new();
        let mut indices = BTreeMap::new();
        let mut cells = Vec::with_capacity(rows.len() * columns.len());
        for &row in rows {
            for &column in columns {
                let mut copied = sheet.cells.get(&(row, column)).cloned();
                let value_only =
                    calculation
                        .spills
                        .iter()
                        .any(|(&(anchor_row, anchor_col), range)| {
                            row >= range.top
                                && row <= range.bottom
                                && column >= range.left
                                && column <= range.right
                                && (row != anchor_row || column != anchor_col)
                                && !(rows.contains(&anchor_row) && columns.contains(&anchor_col))
                        });
                if value_only {
                    if let Some(value) = calculation.values.get(&(row, column)) {
                        let text = TextValue::from_raw(value.clone().display());
                        let content = match value {
                            formula::Value::Number(_) => CellContent::Number(text),
                            formula::Value::Text(_) => CellContent::Text(text),
                            formula::Value::Boolean(_) => CellContent::Boolean(text),
                            formula::Value::Error(_) => CellContent::Error(text),
                            formula::Value::Array(_) => {
                                return Err(Error::Invalid("Unresolved clipboard array"))
                            }
                        };
                        copied = Some(Cell {
                            text: content,
                            style: copied.as_ref().map_or(0, |c| c.style),
                        });
                    }
                }
                if let Some(cell) = copied.as_ref() {
                    let style = if let Some(index) = indices.get(&cell.style) {
                        *index
                    } else {
                        if styles.len() >= 4096 {
                            return Err(Error::Limit);
                        }
                        let index = styles.len();
                        styles.push(state.styles[cell.style].clone());
                        indices.insert(cell.style, index);
                        index
                    };
                    cells.push(Some(ClipboardCell {
                        content: cell.text.clone(),
                        style,
                    }));
                } else {
                    cells.push(None);
                }
            }
        }
        let payload = Payload {
            version: 1,
            rows: rows.to_vec(),
            columns: columns.to_vec(),
            styles,
            cells,
        };
        let bytes = serde_json::to_vec(&payload)
            .map_err(|_| Error::Invalid("Cannot encode clipboard data"))?;
        if bytes.len() > MAX_CLIPBOARD_PAYLOAD {
            return Err(Error::Limit);
        }
        Ok((text, bytes))
    }
    /// Pastes a typed snapshot using each cell's actual source and destination coordinates.
    pub fn paste_clipboard_mapped(
        &mut self,
        rows: &[usize],
        columns: &[usize],
        bytes: &[u8],
    ) -> Result<bool, Error> {
        self.require_editable()?;
        if bytes.len() > MAX_CLIPBOARD_PAYLOAD {
            return Err(Error::Limit);
        }
        let payload: Payload = serde_json::from_slice(bytes)
            .map_err(|_| Error::Invalid("Invalid spreadsheet clipboard data"))?;
        if payload.version != 1
            || !valid_axis(&payload.rows, MAX_ROWS)
            || !valid_axis(&payload.columns, MAX_COLS)
            || !valid_axis(rows, self.rows())
            || !valid_axis(columns, self.cols())
            || payload.rows.len() > rows.len()
            || payload.columns.len() > columns.len()
            || payload.styles.len() > 4096
            || payload.cells.len()
                != payload
                    .rows
                    .len()
                    .checked_mul(payload.columns.len())
                    .ok_or(Error::Limit)?
            || payload.styles.iter().any(|style| {
                style.borders & !15 != 0
                    || style.font.len() > 4096
                    || style.number_format.len() > 4096
            })
            || payload.cells.iter().flatten().any(|cell| {
                cell.style >= payload.styles.len() || !project::content_valid(&cell.content)
            })
        {
            return Err(Error::Invalid("Invalid spreadsheet clipboard data"));
        }
        let dest_rows = &rows[..payload.rows.len()];
        let dest_cols = &columns[..payload.columns.len()];
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        if state.sheet().merges.iter().any(|range| {
            dest_rows
                .iter()
                .any(|r| *r >= range.top && *r <= range.bottom)
                && dest_cols
                    .iter()
                    .any(|c| *c >= range.left && *c <= range.right)
        }) {
            return Err(Error::Invalid(
                "paste involving merged cells is unsupported",
            ));
        }
        let mut style_map = Vec::with_capacity(payload.styles.len());
        let mut known: std::collections::HashMap<_, _> = state
            .styles
            .iter()
            .cloned()
            .enumerate()
            .map(|(id, style)| (style, id))
            .collect();
        for style in payload.styles {
            let id = if let Some(id) = known.get(&style) {
                *id
            } else {
                if state.styles.len() >= 100_000 {
                    return Err(Error::Limit);
                }
                let id = state.styles.len();
                state.styles.push(style.clone());
                known.insert(style, id);
                id
            };
            style_map.push(id);
        }
        let width = payload.columns.len();
        for (index, cell) in payload.cells.into_iter().enumerate() {
            let r = index / width;
            let c = index % width;
            let key = (dest_rows[r], dest_cols[c]);
            if let Some(cell) = cell {
                let content = if matches!(cell.content, CellContent::Formula(_)) {
                    CellContent::from_raw(office::translate_shared_formula(
                        cell.content.raw(),
                        dest_rows[r] as isize - payload.rows[r] as isize,
                        dest_cols[c] as isize - payload.columns[c] as isize,
                    ))
                } else {
                    cell.content
                };
                state.sheet_mut().cells.insert(
                    key,
                    Cell {
                        text: content,
                        style: style_map[cell.style],
                    },
                );
            } else {
                state.sheet_mut().cells.remove(&key);
            }
        }
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        state.selection = Range {
            top: dest_rows[0],
            left: dest_cols[0],
            bottom: *dest_rows.last().unwrap(),
            right: *dest_cols.last().unwrap(),
        };
        rules::validate_mapped(&state, dest_rows, dest_cols)?;
        tables::reconcile_headers(self.history.current(), &mut state)?;
        self.validate_external_table_edit(&state)?;
        Ok(self.history.apply(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_formulas_use_each_physical_source_and_destination() {
        let mut book = Workbook::new(12, 8).unwrap();
        book.set_cell(0, 1, "=A1".into()).unwrap();
        let (_, bytes) = book.copy_clipboard_mapped(&[0], &[1]).unwrap();
        book.paste_clipboard_mapped(&[1], &[1], &bytes).unwrap();
        assert_eq!(book.cell(1, 1), Some("=A2"));
        book.set_cell(0, 1, "=$A1+A$1+$A$1+'Other Sheet'!A1".into())
            .unwrap();
        book.set_cell(2, 3, "=A3".into()).unwrap();
        let mut style = book.style(0, 1).unwrap().clone();
        style.bold = true;
        book.set_style(
            Range {
                top: 0,
                left: 1,
                bottom: 0,
                right: 1,
            },
            style,
        )
        .unwrap();
        let (_, bytes) = book.copy_clipboard_mapped(&[0, 2], &[1, 3]).unwrap();
        book.paste_clipboard_mapped(&[4, 7], &[2, 4], &bytes)
            .unwrap();
        assert_eq!(book.cell(4, 2), Some("=$A5+B$1+$A$1+'Other Sheet'!B5"));
        assert_eq!(book.cell(7, 4), Some("=B8"));
        assert!(book.style(4, 2).unwrap().bold);
        assert!(book.undo());
        assert_eq!(book.cell(4, 2), Some(""));
        assert!(book.redo());
        assert_eq!(book.cell(7, 4), Some("=B8"));
    }
    #[test]
    fn typed_paste_validates_all_destinations_before_commit() {
        let mut book = Workbook::new(6, 4).unwrap();
        book.set_cell(0, 0, "ok".into()).unwrap();
        book.set_cell(2, 0, "bad".into()).unwrap();
        book.add_validation(DataValidation {
            range: Range {
                top: 4,
                left: 2,
                bottom: 4,
                right: 2,
            },
            allow_blank: false,
            kind: ValidationKind::Number {
                minimum: 1.0,
                maximum: 9.0,
            },
        })
        .unwrap();
        let (_, bytes) = book.copy_clipboard_mapped(&[0, 2], &[0]).unwrap();
        assert!(book.paste_clipboard_mapped(&[3, 4], &[2], &bytes).is_err());
        assert_eq!(book.cell(3, 2), Some(""));
        assert_eq!(book.cell(4, 2), Some(""));
        assert!(book
            .paste_clipboard_mapped(&[3, 4], &[2], b"not valid")
            .is_err());
        assert_eq!(book.cell(3, 2), Some(""));
    }
}

#[cfg(test)]
mod calculated_copy {
    use super::*;
    #[test]
    fn spill_children_copy_as_values_while_whole_spills_recalculate_from_the_anchor() {
        let mut b = Workbook::new(16, 12).unwrap();
        b.set_cell(0, 0, "=SEQUENCE(3,2)".into()).unwrap();
        let (text, partial) = b.copy_clipboard_mapped(&[1, 2], &[0, 1]).unwrap();
        assert_eq!(text, "3\t4\n5\t6");
        b.paste_clipboard_mapped(&[5, 6], &[4, 5], &partial)
            .unwrap();
        assert_eq!(b.cell(5, 4), Some("3"));
        assert!(matches!(b.content(5, 4), Some(CellContent::Number(_))));
        let (text, whole) = b.copy_clipboard_mapped(&[0, 1, 2], &[0, 1]).unwrap();
        assert_eq!(text, "1\t2\n3\t4\n5\t6");
        b.paste_clipboard_mapped(&[9, 10, 11], &[7, 8], &whole)
            .unwrap();
        assert_eq!(b.cell(9, 7), Some("=SEQUENCE(3,2)"));
        assert_eq!(b.cell(10, 7), Some(""));
        assert_eq!(b.display_grid()[11][8], "6");
        b.undo();
        assert_eq!(b.display_grid()[11][8], "");
        b.redo();
        assert_eq!(b.display_grid()[11][8], "6");
    }
    #[test]
    fn plain_and_typed_copies_share_one_volatile_calculation_and_keep_text_types() {
        let mut b = Workbook::new(8, 6).unwrap();
        b.set_cell(0, 0, "=RANDARRAY(3,1,1,100000000,TRUE)".into())
            .unwrap();
        let (text, bytes) = b.copy_clipboard_mapped(&[1, 2], &[0]).unwrap();
        b.paste_clipboard_mapped(&[4, 5], &[3], &bytes).unwrap();
        let expected: Vec<_> = text.lines().collect();
        assert_eq!(b.cell(4, 3).unwrap(), expected[0]);
        assert_eq!(b.cell(5, 3).unwrap(), expected[1]);
        b.set_cell(0, 0, "=VSTACK(\"0007\",\"=1+2\")".into())
            .unwrap();
        let (text, bytes) = b.copy_clipboard_mapped(&[1], &[0]).unwrap();
        assert_eq!(text, "=1+2");
        b.paste_clipboard_mapped(&[6], &[4], &bytes).unwrap();
        assert!(matches!(b.content(6, 4), Some(CellContent::Text(_))));
        assert_eq!(b.display_grid()[6][4], "=1+2");
    }
}
