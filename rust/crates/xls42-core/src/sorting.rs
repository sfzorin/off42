//! Stable typed sorting visits stored keys/cells, with algebraic positions for wholly blank lines.
use super::*;
use std::cmp::Ordering;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortKey {
    pub index: usize,
    pub descending: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SortOptions {
    pub header: bool,
    pub left_to_right: bool,
    pub case_sensitive: bool,
    pub natural: bool,
    pub custom_list: Vec<String>,
}
#[derive(Clone)]
enum SortValue {
    Blank,
    Number(f64),
    Text(String, String),
    Boolean(bool),
    Error,
}
#[derive(Clone)]
struct KeyValue {
    value: SortValue,
    custom: Option<usize>,
}
impl KeyValue {
    fn blank() -> Self {
        Self {
            value: SortValue::Blank,
            custom: None,
        }
    }
    fn from(value: &formula::Value, custom: &BTreeMap<String, usize>) -> Self {
        let rank = custom.get(&value.clone().display().to_lowercase()).copied();
        let value = match value {
            formula::Value::Number(n) => SortValue::Number(*n),
            formula::Value::Text(s) if s.is_empty() => SortValue::Blank,
            formula::Value::Text(s) => SortValue::Text(s.to_lowercase(), s.clone()),
            formula::Value::Boolean(v) => SortValue::Boolean(*v),
            formula::Value::Error(_) => SortValue::Error,
            formula::Value::Array(_) => SortValue::Error,
        };
        Self {
            value,
            custom: rank,
        }
    }
    fn is_blank(&self) -> bool {
        matches!(self.value, SortValue::Blank)
    }
    fn compare(&self, other: &Self, options: &SortOptions, descending: bool) -> Ordering {
        if self.is_blank() != other.is_blank() {
            return if self.is_blank() {
                Ordering::Greater
            } else {
                Ordering::Less
            };
        }
        if self.is_blank() {
            return Ordering::Equal;
        }
        let custom = match (self.custom, other.custom) {
            (Some(a), Some(b)) => Some(a.cmp(&b)),
            (Some(_), None) => Some(Ordering::Less),
            (None, Some(_)) => Some(Ordering::Greater),
            _ => None,
        };
        let rank = |value: &SortValue| match value {
            SortValue::Number(_) => 0,
            SortValue::Text(..) => 1,
            SortValue::Boolean(_) => 2,
            SortValue::Error => 3,
            SortValue::Blank => 4,
        };
        let order = custom.unwrap_or_else(|| match (&self.value, &other.value) {
            (SortValue::Number(a), SortValue::Number(b)) => {
                if a == b {
                    Ordering::Equal
                } else {
                    a.total_cmp(b)
                }
            }
            (SortValue::Text(a, raw_a), SortValue::Text(b, raw_b)) => {
                let order = if options.natural {
                    natural_compare(a, b)
                } else {
                    a.cmp(b)
                };
                if order.is_eq() && options.case_sensitive && a == b {
                    raw_a
                        .chars()
                        .map(|c| (!c.is_lowercase(), c))
                        .cmp(raw_b.chars().map(|c| (!c.is_lowercase(), c)))
                } else {
                    order
                }
            }
            (SortValue::Boolean(a), SortValue::Boolean(b)) => a.cmp(b),
            (SortValue::Error, SortValue::Error) => Ordering::Equal,
            (a, b) => rank(a).cmp(&rank(b)),
        });
        if descending { order.reverse() } else { order }
    }
}
fn natural_compare(a: &str, b: &str) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a.as_bytes()[i].is_ascii_digit() && b.as_bytes()[j].is_ascii_digit() {
            let (mut ai, mut bj) = (i, j);
            while ai < a.len() && a.as_bytes()[ai].is_ascii_digit() {
                ai += 1
            }
            while bj < b.len() && b.as_bytes()[bj].is_ascii_digit() {
                bj += 1
            }
            let x = a[i..ai].trim_start_matches('0');
            let y = b[j..bj].trim_start_matches('0');
            let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
            if !order.is_eq() {
                return order;
            }
            i = ai;
            j = bj;
        } else {
            let x = a[i..].chars().next().unwrap();
            let y = b[j..].chars().next().unwrap();
            let order = x.cmp(&y);
            if !order.is_eq() {
                return order;
            }
            i += x.len_utf8();
            j += y.len_utf8();
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}
impl Workbook {
    /// Sort rows or columns by several typed keys. Blank lines stay last in both directions.
    pub fn sort_with_options(
        &mut self,
        range: Range,
        keys: &[SortKey],
        options: &SortOptions,
    ) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let source = self.history.current();
        let sheet = source.sheet();
        if sheet.protected {
            return Err(Error::Protected);
        }
        if !range.valid(sheet) {
            return Err(Error::Bounds);
        }
        if keys.is_empty()
            || keys.len() > 64
            || options.custom_list.len() > 128
            || options.custom_list.iter().map(String::len).sum::<usize>() > 32768
        {
            return Err(Error::Limit);
        }
        let key_positions = keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.index, i))
            .collect::<BTreeMap<_, _>>();
        if key_positions.len() != keys.len()
            || keys.iter().any(|key| {
                if options.left_to_right {
                    key.index < range.top || key.index > range.bottom
                } else {
                    key.index < range.left || key.index > range.right
                }
            })
        {
            return Err(Error::Invalid("Invalid or duplicate sort key"));
        }
        let mut body = range;
        if options.header {
            if options.left_to_right {
                if body.left == body.right {
                    return Ok(false);
                }
                body.left += 1;
            } else {
                if body.top == body.bottom {
                    return Ok(false);
                }
                body.top += 1;
            }
        }
        if sheet
            .merges
            .iter()
            .chain(&sheet.protected_formula_ranges)
            .any(|r| r.overlaps(body))
        {
            return Err(Error::Invalid("Sort cannot split merged or array cells"));
        }
        let coordinates = sheet.cells.iter().filter_map(|(&(row, col), cell)| {
            if cell.text.raw().is_empty()
                || row < body.top
                || row > body.bottom
                || col < body.left
                || col > body.right
            {
                return None;
            }
            key_positions
                .contains_key(&if options.left_to_right { row } else { col })
                .then_some((row, col))
        });
        let calculation =
            formula::typed_cells_at(self, coordinates, chrono::Local::now().naive_local())
                .map_err(Error::Invalid)?;
        if calculation.spills.values().any(|r| r.overlaps(body)) {
            return Err(Error::Invalid("Sort cannot split a calculated spill"));
        }
        let mut custom = BTreeMap::new();
        for (i, text) in options.custom_list.iter().enumerate() {
            custom.entry(text.to_lowercase()).or_insert(i);
        }
        let mut occupied = BTreeMap::<usize, Vec<KeyValue>>::new();
        for (&(row, col), value) in &calculation.values {
            if row < body.top || row > body.bottom || col < body.left || col > body.right {
                continue;
            }
            let (line, key) = if options.left_to_right {
                (col, row)
            } else {
                (row, col)
            };
            let Some(&key) = key_positions.get(&key) else {
                continue;
            };
            let value = KeyValue::from(value, &custom);
            if value.is_blank() {
                continue;
            }
            if !occupied.contains_key(&line)
                && occupied.len().saturating_add(1).saturating_mul(keys.len()) > 1_000_000
            {
                return Err(Error::Limit);
            }
            occupied
                .entry(line)
                .or_insert_with(|| vec![KeyValue::blank(); keys.len()])[key] = value;
        }
        let source_lines = occupied.keys().copied().collect::<Vec<_>>();
        let count = source_lines.len();
        let first = if options.left_to_right {
            body.left
        } else {
            body.top
        };
        let mut ordered = occupied.into_iter().collect::<Vec<_>>();
        ordered.sort_by(|(a, x), (b, y)| {
            for (i, key) in keys.iter().enumerate() {
                let order = x[i].compare(&y[i], options, key.descending);
                if !order.is_eq() {
                    return order;
                }
            }
            a.cmp(b)
        });
        let destinations = ordered
            .iter()
            .enumerate()
            .map(|(i, (line, _))| (*line, first + i))
            .collect::<BTreeMap<_, _>>();
        let destination = |line: usize| {
            destinations.get(&line).copied().unwrap_or_else(|| {
                first + count + (line - first) - source_lines.partition_point(|v| *v < line)
            })
        };
        let mut state = source.clone();
        let mut cells = BTreeMap::new();
        for (&(row, col), cell) in &sheet.cells {
            let mut cell = cell.clone();
            let (mut r, mut c) = (row, col);
            if row >= body.top && row <= body.bottom && col >= body.left && col <= body.right {
                if options.left_to_right {
                    c = destination(col)
                } else {
                    r = destination(row)
                }
                if (r, c) != (row, col) && matches!(cell.text, CellContent::Formula(_)) {
                    cell.text = CellContent::from_raw(translate_refs(
                        cell.text.raw(),
                        r as isize - row as isize,
                        c as isize - col as isize,
                    ));
                }
            }
            cells.insert((r, c), cell);
        }
        state.sheet_mut().cells = cells;
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        rules::validate_range(&state, body)?;
        tables::reconcile_headers(source, &mut state)?;
        self.validate_external_table_edit(&state)?;
        Ok(self.history.apply(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn full() -> Range {
        Range {
            top: 0,
            left: 0,
            bottom: MAX_ROWS - 1,
            right: MAX_COLS - 1,
        }
    }
    #[test]
    fn full_sheet_sort_moves_only_stored_rows_preserving_far_data_styles_relative_formulas_and_history()
     {
        let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
        for (r, key, text) in [
            (0, "3", "THREE"),
            (9, "1", "=A10*10"),
            (MAX_ROWS - 1, "2", "LAST"),
        ] {
            b.set_cell(r, 0, key.into()).unwrap();
            b.set_cell(r, MAX_COLS - 1, text.into()).unwrap();
        }
        b.set_cell(20, MAX_COLS - 1, "BLANK-KEY".into()).unwrap();
        b.set_style(
            Range {
                top: 9,
                left: MAX_COLS - 1,
                bottom: 9,
                right: MAX_COLS - 1,
            },
            CellStyle {
                bold: true,
                number_format: "0.00".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let original = b.project_bytes().unwrap();
        assert!(b.sort_range(full(), 0, false).unwrap());
        assert_eq!(
            [b.cell(0, 0), b.cell(1, 0), b.cell(2, 0)],
            [Some("1"), Some("2"), Some("3")]
        );
        assert_eq!(b.cell(0, MAX_COLS - 1), Some("=A1*10"));
        assert_eq!(b.calculated_cells()[&(0, MAX_COLS - 1)], "10");
        assert_eq!(b.cell(1, MAX_COLS - 1), Some("LAST"));
        assert_eq!(b.cell(2, MAX_COLS - 1), Some("THREE"));
        assert_eq!(b.cell(21, MAX_COLS - 1), Some("BLANK-KEY"));
        assert!(b.style(0, MAX_COLS - 1).unwrap().bold);
        assert_eq!(b.style(0, MAX_COLS - 1).unwrap().number_format, "0.00");
        let changed = b.project_bytes().unwrap();
        assert!(b.undo());
        assert_eq!(b.project_bytes().unwrap(), original);
        assert!(b.redo());
        assert_eq!(b.project_bytes().unwrap(), changed);
        let opened = Workbook::from_office_bytes(&b.xlsx_bytes().unwrap(), "xlsx").unwrap();
        assert_eq!(opened.calculated_cells(), b.calculated_cells());
        assert!(opened.style(0, MAX_COLS - 1).unwrap().bold);
        assert_eq!(opened.style(0, MAX_COLS - 1).unwrap().number_format, "0.00");
        // The pre-existing ODS format guard must still refuse this numeric format rather than lose it.
        assert!(b.ods_bytes().is_err());
        assert_eq!(b.project_bytes().unwrap(), changed);
    }
    #[test]
    fn mixed_types_are_transitive_blanks_stay_last_in_both_directions_and_sort_is_stable() {
        let mut b = Workbook::new(12, 3).unwrap();
        for (r, text) in [
            (0, "10"),
            (1, "2"),
            (2, "1a"),
            (4, "TRUE"),
            (5, "FALSE"),
            (7, "=\"\""),
        ] {
            b.set_cell(r, 0, text.into()).unwrap();
        }
        b.set_content(3, 0, CellContent::Text(TextValue::from_raw("10".into())))
            .unwrap();
        b.set_content(6, 0, CellContent::Error(TextValue::from_raw("#N/A".into())))
            .unwrap();
        for r in 0..9 {
            b.set_cell(r, 1, format!("row{r}")).unwrap();
        }
        let original = b.project_bytes().unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 11,
            right: 1,
        };
        for (desc, expected) in [
            (false, [1, 0, 3, 2, 5, 4, 6, 7, 8]),
            (true, [6, 4, 5, 2, 3, 0, 1, 7, 8]),
        ] {
            b.sort_range(range, 0, desc).unwrap();
            for (r, source) in expected.iter().enumerate() {
                assert_eq!(b.cell(r, 1), Some(format!("row{source}").as_str()));
            }
            assert!(b.undo());
            assert_eq!(b.project_bytes().unwrap(), original);
        }
    }
    #[test]
    fn multiple_keys_custom_priority_natural_text_header_and_horizontal_sort_keep_complete_rows_or_columns()
     {
        let mut b = Workbook::new(10, 5).unwrap();
        for (r, group, part) in [
            (0, "Priority", "Part"),
            (1, "low", "R10"),
            (2, "HIGH", "R2"),
            (3, "Medium", "R1"),
            (4, "low", "R02"),
        ] {
            b.set_cell(r, 0, group.into()).unwrap();
            b.set_cell(r, 1, part.into()).unwrap();
            b.set_cell(r, 2, format!("id{r}")).unwrap();
        }
        let range = Range {
            top: 0,
            left: 0,
            bottom: 9,
            right: 2,
        };
        let options = SortOptions {
            header: true,
            natural: true,
            custom_list: vec!["high".into(), "medium".into(), "low".into()],
            ..Default::default()
        };
        let original = b.project_bytes().unwrap();
        b.sort_with_options(
            range,
            &[
                SortKey {
                    index: 0,
                    descending: false,
                },
                SortKey {
                    index: 1,
                    descending: false,
                },
            ],
            &options,
        )
        .unwrap();
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let opened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(opened.calculated_cells(), b.calculated_cells());
        }
        assert_eq!(b.cell(0, 0), Some("Priority"));
        assert_eq!(
            [b.cell(1, 2), b.cell(2, 2), b.cell(3, 2), b.cell(4, 2)],
            [Some("id2"), Some("id3"), Some("id4"), Some("id1")]
        );
        assert!(b.undo());
        assert_eq!(b.project_bytes().unwrap(), original);
        let mut b = Workbook::new(3, 6).unwrap();
        for (c, value) in [(0, "Header"), (1, "R10"), (2, "R2"), (3, "R1"), (4, "R02")] {
            b.set_cell(0, c, value.into()).unwrap();
            b.set_cell(1, c, format!("=A2+{c}")).unwrap();
        }
        let original = b.project_bytes().unwrap();
        b.sort_with_options(
            Range {
                top: 0,
                left: 0,
                bottom: 1,
                right: 5,
            },
            &[SortKey {
                index: 0,
                descending: false,
            }],
            &SortOptions {
                header: true,
                left_to_right: true,
                natural: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            [
                b.cell(0, 0),
                b.cell(0, 1),
                b.cell(0, 2),
                b.cell(0, 3),
                b.cell(0, 4)
            ],
            [
                Some("Header"),
                Some("R1"),
                Some("R2"),
                Some("R02"),
                Some("R10")
            ]
        );
        assert_eq!(b.cell(1, 1), Some("=#REF!+3"));
        assert_eq!(b.cell(1, 4), Some("=D2+1"));
        assert!(b.undo());
        assert_eq!(b.project_bytes().unwrap(), original);
    }
    #[test]
    fn invalid_keys_spills_merges_protection_and_failed_destination_validation_leave_source_and_redo_intact()
     {
        let mut b = Workbook::new(10, 4).unwrap();
        b.set_cell(0, 0, "2".into()).unwrap();
        b.set_cell(1, 0, "1".into()).unwrap();
        b.add_validation(DataValidation {
            range: Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            allow_blank: true,
            kind: ValidationKind::Number {
                minimum: 2.0,
                maximum: 3.0,
            },
        })
        .unwrap();
        let before = b.project_bytes().unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 9,
            right: 3,
        };
        assert!(b.sort_range(range, 0, false).is_err());
        assert_eq!(b.project_bytes().unwrap(), before);
        assert!(
            b.sort_with_options(
                range,
                &[
                    SortKey {
                        index: 0,
                        descending: false
                    },
                    SortKey {
                        index: 0,
                        descending: true
                    }
                ],
                &SortOptions::default()
            )
            .is_err()
        );
        assert_eq!(b.project_bytes().unwrap(), before);
        b.set_cell(0, 2, "=SEQUENCE(2)".into()).unwrap();
        let spilled = b.project_bytes().unwrap();
        assert!(b.sort_range(range, 0, true).is_err());
        assert_eq!(b.project_bytes().unwrap(), spilled);
        b.undo();
        assert_eq!(b.project_bytes().unwrap(), before);
        assert!(b.sort_range(range, 0, false).is_err());
        assert!(b.redo());
        assert_eq!(b.project_bytes().unwrap(), spilled);
        b.set_protected(true);
        let protected = b.project_bytes().unwrap();
        assert!(b.sort_range(range, 0, false).is_err());
        assert_eq!(b.project_bytes().unwrap(), protected);
        let mut b = Workbook::new(4, 4).unwrap();
        b.merge(Range {
            top: 1,
            left: 1,
            bottom: 2,
            right: 1,
        })
        .unwrap();
        let merged = b.project_bytes().unwrap();
        assert!(
            b.sort_range(
                Range {
                    top: 0,
                    left: 0,
                    bottom: 3,
                    right: 3
                },
                0,
                false
            )
            .is_err()
        );
        assert_eq!(b.project_bytes().unwrap(), merged);
    }
}
