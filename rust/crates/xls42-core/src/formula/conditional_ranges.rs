//! Conditional reference aggregates visit occupied offsets; all-blank offsets share one result.
use super::*;
type Rect = (SheetId, usize, usize, usize, usize);

// SUMIF/AVERAGEIF use the target's top-left corner with the criteria range's dimensions.
// https://support.microsoft.com/en-au/excel/functions/sumif-function
// https://support.microsoft.com/en-us/excel/functions/averageif-function
pub(super) fn aligned_target(target: Rect, base: Rect) -> Result<Rect, &'static str> {
    let bottom = target.1.checked_add(base.3 - base.1).ok_or("#REF!")?;
    let right = target.2.checked_add(base.4 - base.2).ok_or("#REF!")?;
    if bottom >= crate::MAX_ROWS || right >= crate::MAX_COLS {
        return Err("#REF!");
    }
    Ok((target.0, target.1, target.2, bottom, right))
}

impl Parser<'_, '_> {
    fn conditional_match(
        &mut self,
        value: &Value,
        blank: bool,
        criterion: &Value,
    ) -> Result<bool, &'static str> {
        self.eval.steps = self.eval.steps.saturating_add(1);
        if let Value::Text(spec) = criterion {
            if spec.contains(['*', '?', '~']) {
                let len = match value {
                    Value::Text(text) => text.len(),
                    _ => value.clone().display().len(),
                };
                self.eval.steps = self
                    .eval
                    .steps
                    .saturating_add(spec.len().saturating_mul(len));
            }
        }
        if self.eval.steps > MAX_STEPS {
            return Err("#LIMIT!");
        }
        Ok(criterion_matches(value, blank, criterion))
    }
    pub(super) fn sparse_conditional(
        &mut self,
        name: &str,
        criteria: &[(Rect, Value)],
        target: Option<Rect>,
    ) -> Value {
        match self.conditional_result(name, criteria, target) {
            Ok(value) => value,
            Err(e) => Value::Error(e),
        }
    }
    fn conditional_result(
        &mut self,
        name: &str,
        criteria: &[(Rect, Value)],
        target: Option<Rect>,
    ) -> Result<Value, &'static str> {
        let base = criteria[0].0;
        let area = (base.3 - base.1 + 1)
            .checked_mul(base.4 - base.2 + 1)
            .ok_or("#LIMIT!")?;
        let mut offsets = std::collections::BTreeSet::new();
        for range in criteria.iter().map(|(range, _)| *range).chain(target) {
            for (r, c) in self.eval.occupied_range(
                range.0,
                range.1,
                range.2,
                range.3,
                range.4,
                self.depth + 1,
            )? {
                offsets.insert((r - range.1, c - range.2));
            }
        }
        let mut count = 0usize;
        if target.is_none() {
            let mut blank_matches = true;
            for (_, criterion) in criteria {
                if !self.conditional_match(&Value::Number(0.0), true, criterion)? {
                    blank_matches = false;
                    break;
                }
            }
            if blank_matches {
                count = area - offsets.len();
            }
        }
        let mut sum = 0.0;
        let mut extremum: Option<f64> = None;
        for (row, col) in offsets {
            let mut matched = true;
            for (range, criterion) in criteria {
                let (r, c) = (range.1 + row, range.2 + col);
                let (value, blank) = self.eval.range_value(range.0, r, c, self.depth + 1);
                let value = if blank { Value::Number(0.0) } else { value };
                if !self.conditional_match(&value, blank, criterion)? {
                    matched = false;
                    break;
                }
            }
            if !matched {
                continue;
            }
            if let Some(range) = target {
                // range_cell distinguishes physical blank/text/logical cells from numeric formula results.
                match self
                    .eval
                    .range_cell(range.0, range.1 + row, range.2 + col, self.depth + 1)
                {
                    Some(Value::Number(n)) => {
                        sum += n;
                        count += 1;
                        extremum = Some(extremum.map_or(n, |old| {
                            if name == "MINIFS" {
                                old.min(n)
                            } else {
                                old.max(n)
                            }
                        }));
                    }
                    Some(Value::Error(e)) => return Err(e),
                    _ => {}
                }
            } else {
                count += 1;
            }
        }
        Ok(match name {
            "COUNTIF" | "COUNTIFS" => finite(count as f64),
            "SUMIF" | "SUMIFS" => finite(sum),
            "MAXIFS" | "MINIFS" => finite(extremum.unwrap_or(0.0)),
            _ if count == 0 => Value::Error("#DIV/0!"),
            _ => finite(sum / count as f64),
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{CellContent, MAX_COLS, MAX_ROWS, Workbook};
    #[test]
    fn full_column_conditions_include_distant_values_blank_offsets_empty_formulas_and_spills() {
        let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
        b.rename_sheet(1, "Data").unwrap();
        for (r, a, number, region) in [
            (0, "apple", "10", "east"),
            (1, "apple", "-2", "west"),
            (2, "banana", "30", "east"),
            (3, "=\"\"", "40", "east"),
            (4, "", "50", "east"),
            (5, "=SEQUENCE(2,1)", "5", ""),
            (6, "", "7", ""),
            (7, "TRUE", "0", ""),
            (8, "", "3", ""),
            (9, "", "90", ""),
            (MAX_ROWS - 1, "apple", "20", "east"),
        ] {
            for (c, text) in [a, number, region].into_iter().enumerate() {
                if !text.is_empty() {
                    b.set_cell(r, c, text.into()).unwrap();
                }
            }
        }
        b.set_content(
            8,
            0,
            CellContent::Error(suite42_core::TextValue::from_raw("#N/A".into())),
        )
        .unwrap();
        b.set_content(
            9,
            0,
            CellContent::Text(suite42_core::TextValue::from_raw("0".into())),
        )
        .unwrap();
        b.add_sheet("Results").unwrap();
        let cases = [
            ("=COUNTIF(Data!A:A,\"apple\")", "3"),
            ("=COUNTIFS(Data!A:A,\"apple\",Data!C:C,\"east\")", "2"),
            ("=SUMIF(Data!A:A,\"apple\",Data!B:B)", "28"),
            (
                "=SUMIFS(Data!B:B,Data!A:A,\"apple\",Data!C:C,\"east\")",
                "30",
            ),
            (
                "=AVERAGEIFS(Data!B:B,Data!A:A,\"apple\",Data!C:C,\"east\")",
                "15",
            ),
            ("=MAXIFS(Data!B:B,Data!A:A,\"apple\")", "20"),
            ("=MINIFS(Data!B:B,Data!A:A,\"apple\")", "-2"),
            ("=COUNTIF(Data!A:A,\"\")", "1048567"),
            ("=COUNTIFS(Data!A:A,\"\",Data!C:C,\"east\")", "2"),
            ("=SUMIF(Data!A:A,\"\",Data!B:B)", "90"),
            ("=AVERAGEIF(Data!A:A,\"\",Data!B:B)", "45"),
            ("=COUNTIF(Data!A:A,\"a*\")", "3"),
            ("=SUMIF(Data!A:A,\"~*\",Data!B:B)", "0"),
            ("=SUMIF(Data!A:A,1,Data!B:B)", "5"),
            ("=COUNTIFS(Data!A:A,\"\",Data!C:C,\"\")", "1048565"),
            ("=COUNTIF(Data!A:XFD,\"\")", "17179869158"),
            ("=SUMIFS(Data!B:B,Data!A1:A3,\"apple\")", "#VALUE!"),
            ("=AVERAGEIFS(Data!B:B,Data!A:A,\"absent\")", "#DIV/0!"),
        ];
        for (r, (formula, _)) in cases.iter().enumerate() {
            b.set_cell(r, 0, (*formula).into()).unwrap();
        }
        let before = b.project_bytes().unwrap();
        for (r, (formula, expected)) in cases.iter().enumerate() {
            assert_eq!(&b.calculated_cells()[&(r, 0)], expected, "{formula}");
        }
        assert_eq!(
            b.project_bytes().unwrap(),
            before,
            "calculation changed the document"
        );
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let opened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(opened.calculated_cells(), b.calculated_cells());
        }
        b.select_sheet(1).unwrap();
        b.set_cell(MAX_ROWS - 1, 1, "22".into()).unwrap();
        b.select_sheet(2).unwrap();
        assert_eq!(b.calculated_cells()[&(3, 0)], "32");
        assert!(b.undo()); // tab selection
        assert!(b.undo()); // far value edit
        assert_eq!(b.cell(MAX_ROWS - 1, 1), Some("20"));
        assert!(b.redo());
        assert_eq!(b.cell(MAX_ROWS - 1, 1), Some("22"));
    }
    #[test]
    fn offset_targets_use_the_criteria_shape_and_propagate_only_selected_errors() {
        let mut b = Workbook::new(20, 8).unwrap();
        for (r, c, text) in [
            (0, 0, "pick"),
            (1, 0, "skip"),
            (2, 0, "pick"),
            (0, 1, "10"),
            (1, 1, "20"),
            (2, 1, "30"),
        ] {
            b.set_cell(r, c, text.into()).unwrap();
        }
        b.set_content(
            1,
            1,
            CellContent::Error(suite42_core::TextValue::from_raw("#N/A".into())),
        )
        .unwrap();
        for (r, formula, expected) in [
            (0, "=SUMIF(A1:A3,\"pick\",B1)", "40"),
            (1, "=AVERAGEIF(A1:A3,\"pick\",B1)", "20"),
            (2, "=SUMIF(A1:A3,\"skip\",B1)", "#N/A"),
            (3, "=SUMIFS(B1:B3,A1:A3,\"skip\")", "#N/A"),
            (4, "=COUNTIF(A1:A1048576,\"\")", "1048573"),
            (5, "=SUMIF(A1:A3,\"pick\",B1048575)", "#REF!"),
            (6, "=AVERAGEIF(A1:A3,\"pick\",XFD1048575)", "#REF!"),
        ] {
            b.set_cell(r, 4, formula.into()).unwrap();
            assert_eq!(b.calculated_cells()[&(r, 4)], expected, "{formula}");
        }
    }
}
