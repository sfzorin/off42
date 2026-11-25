//! Aggregate reference traversal proportional to stored data and spill cells, not reference area.
use super::*;
impl Eval<'_> {
    pub(super) fn occupied_range(
        &mut self,
        sheet: SheetId,
        top: usize,
        left: usize,
        bottom: usize,
        right: usize,
        depth: usize,
    ) -> Result<Vec<(usize, usize)>, &'static str> {
        self.ensure_spills(sheet, depth);
        let source = self
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)
            .ok_or("#REF!")?;
        let mut coordinates = std::collections::BTreeSet::new();
        for (&(r, c), cell) in source.cells.range((top, left)..=(bottom, right)) {
            self.steps = self.steps.saturating_add(1);
            if self.steps > MAX_STEPS {
                return Err("#LIMIT!");
            }
            if c >= left && c <= right && !cell.text.raw().is_empty() {
                coordinates.insert((r, c));
            }
        }
        for (&(s, r, c),_) in self.spills.range((sheet,top,left)..=(sheet,bottom,right)) {
            self.steps = self.steps.saturating_add(1);
            if self.steps > MAX_STEPS {
                return Err("#LIMIT!");
            }
            if s == sheet && r >= top && r <= bottom && c >= left && c <= right {
                coordinates.insert((r, c));
            }
        }
        Ok(coordinates.into_iter().collect())
    }
}
impl Parser<'_, '_> {
    pub(super) fn count_blank_reference(&mut self) -> Option<Value> {
        let start = self.pos;
        let Some((sheet, top, left, bottom, right)) = self.read_range() else {
            self.pos = start;
            return None;
        };
        if !self.take(b')') {
            self.pos = start;
            return None;
        }
        let coordinates =
            match self
                .eval
                .occupied_range(sheet, top, left, bottom, right, self.depth + 1)
            {
                Ok(v) => v,
                Err(e) => return Some(Value::Error(e)),
            };
        let mut present = 0usize;
        for (r, c) in coordinates {
            let (value, blank) = self.eval.range_value(sheet, r, c, self.depth + 1);
            if !blank && !matches!(value,Value::Text(ref s)if s.is_empty()) {
                present += 1;
            }
        }
        let area = (bottom - top + 1).checked_mul(right - left + 1);
        Some(match area {
            Some(area) => finite(area.saturating_sub(present) as f64),
            None => Value::Error("#LIMIT!"),
        })
    }
}
#[cfg(test)]
mod tests {
    use crate::{MAX_COLS, MAX_ROWS, Workbook};
    #[test]
    fn full_sheet_aggregates_count_implicit_blanks_typed_values_and_spills_without_expansion() {
        let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
        b.rename_sheet(1, "Data").unwrap();
        for (r, c, text) in [
            (0, 0, "10"),
            (MAX_ROWS - 1, MAX_COLS - 1, "20"),
            (0, 1, "text"),
            (1, 1, "=\"\""),
            (0, 2, "TRUE"),
            (0, 3, "=SEQUENCE(2,2)"),
        ] {
            b.set_cell(r, c, text.into()).unwrap();
        }
        b.set_content(
            1,
            2,
            crate::CellContent::Error(suite42_core::TextValue::from_raw("#DIV/0!".into())),
        )
        .unwrap();
        b.add_sheet("Results").unwrap();
        let cases = [
            ("=SUM(Data!A1:XFD1048576)", "#DIV/0!"),
            ("=COUNT(Data!A1:XFD1048576)", "6"),
            ("=COUNTA(Data!A1:XFD1048576)", "10"),
            ("=COUNTBLANK(Data!A1:XFD1048576)", "17179869175"),
            ("=SUM(Data!A1:B1048576,Data!XFD1:XFD1048576)", "30"),
            ("=AVERAGE(Data!A1:B1048576,Data!XFD1:XFD1048576)", "15"),
            ("=MIN(Data!A1:B1048576,Data!XFD1:XFD1048576)", "10"),
            ("=MAX(Data!A1:B1048576,Data!XFD1:XFD1048576)", "20"),
            ("=PRODUCT(Data!A1:B1048576,Data!XFD1:XFD1048576)", "200"),
            ("=SUMSQ(Data!A1:B1048576,Data!XFD1:XFD1048576)", "500"),
            ("=COUNTBLANK(Data!D1:E2)", "0"),
        ];
        for (r, (formula, _)) in cases.iter().enumerate() {
            b.set_cell(r, 0, (*formula).into()).unwrap();
        }
        let values = b.calculated_cells();
        for (r, (formula, want)) in cases.iter().enumerate() {
            assert_eq!(&values[&(r, 0)], want, "{formula}");
        }
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let opened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(opened.calculated_cells(), values);
        }
    }
}
