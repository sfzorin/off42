//! Calculated/formatted text only for occupied cells and spills; empty worksheet area allocates no strings.
use super::*;
impl Workbook {
    fn can_use_external_values(&self) -> bool {
        self.external_baseline.as_ref().is_some_and(|base| {
            base.sheets.len() == self.history.current().sheets.len()
                && base
                    .sheets
                    .iter()
                    .zip(&self.history.current().sheets)
                    .all(|(a, b)| {
                        if a == b {
                            return true;
                        }
                        let mut a = a.clone();
                        a.solver = b.solver.clone();
                        a == *b
                    })
        })
    }
    /// Stored editor inputs in all sheets; implicit blank coordinates require no validation or allocation.
    pub fn cell_inputs(&self)->impl Iterator<Item=(SheetId,usize,usize,&str)>+'_ {
        self.history.current().sheets.iter().flat_map(|sheet|sheet.cells.iter().map(move|(&(r,c),cell)|(sheet.id,r,c,cell.text.raw())))
    }
    pub fn display_cells(&self) -> BTreeMap<(usize, usize), String> {
        let calculation = formula::calculation_at(self, chrono::Local::now().naive_local());
        self.display_cells_with(&calculation)
    }
    pub fn calculated_cells(&self) -> BTreeMap<(usize, usize), String> {
        let calculation = formula::calculation_at(self, chrono::Local::now().naive_local());
        self.calculated_cells_with(&calculation)
    }
    pub(super) fn calculated_cells_with(
        &self,
        calculation: &formula::Calculation,
    ) -> BTreeMap<(usize, usize), String> {
        let mut cells: BTreeMap<_, _> = self
            .history
            .current()
            .sheet()
            .cells
            .iter()
            .filter_map(|(&cell, value)| {
                let text = match &value.text {
                    CellContent::Boolean(v) => {
                        if v.raw().eq_ignore_ascii_case("true") {
                            "TRUE".into()
                        } else {
                            "FALSE".into()
                        }
                    }
                    _ => value.text.raw().to_owned(),
                };
                (!text.is_empty()).then_some((cell, text))
            })
            .collect();
        for (&cell, value) in &calculation.values {
            let text = value.clone().display();
            if text.is_empty() {
                cells.remove(&cell);
            } else {
                cells.insert(cell, text);
            }
        }
        if self.can_use_external_values() {
            for (&(sheet, row, col), value) in &self.external_cache {
                if sheet == self.active_sheet()
                    && row < self.rows()
                    && col < self.cols()
                    && cells.get(&(row, col)).is_some_and(|s| s == "#ERROR!")
                {
                    cells.insert((row, col), value.clone());
                }
            }
        }
        cells
    }
    pub(super) fn display_cells_with(
        &self,
        calculation: &formula::Calculation,
    ) -> BTreeMap<(usize, usize), String> {
        let mut cells = self.calculated_cells_with(calculation);
        for (&(r, c), text) in &mut cells {
            display::format_cell(
                self,
                self.active_sheet(),
                r,
                c,
                calculation.values.get(&(r, c)),
                text,
            );
        }
        cells.retain(|_, text| !text.is_empty());
        cells
    }
    pub fn visible_rows_from_cells(&self, cells: &BTreeMap<(usize, usize), String>) -> Vec<usize> {
        let sheet = self.history.current().sheet();
        let needle = sheet.filter.as_ref().map(|f| f.contains.to_lowercase());
        (0..sheet.rows)
            .filter(|r| {
                !sheet.view.hidden_rows.contains(r)
                    && sheet.filter.as_ref().is_none_or(|f| {
                        *r <= f.range.top
                            || *r > f.range.bottom
                            || cells
                                .get(&(*r, f.column))
                                .map_or("", String::as_str)
                                .to_lowercase()
                                .contains(needle.as_ref().unwrap())
                    })
            })
            .collect()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_projection_has_precise_formats_spills_booleans_errors_and_empty_area() {
        let mut b = Workbook::new(1000, 100).unwrap();
        for (r, c, v) in [
            (0, 0, "1.2345678901234567"),
            (1, 0, "TRUE"),
            (2, 0, "=SEQUENCE(2,2,7,1)"),
            (4, 0, "=1/0"),
            (5, 0, "=\"\""),
            (999, 99, "LAST"),
        ] {
            b.set_cell(r, c, v.into()).unwrap();
        }
        let cells = b.display_cells();
        assert_eq!(cells.len(), 8);
        assert_eq!(cells[&(0, 0)], "1.2345678901");
        assert_eq!(cells[&(1, 0)], "TRUE");
        assert_eq!(cells[&(2, 0)], "7");
        assert_eq!(cells[&(2, 1)], "8");
        assert_eq!(cells[&(3, 0)], "9");
        assert_eq!(cells[&(3, 1)], "10");
        assert_eq!(cells[&(4, 0)], "#DIV/0!");
        assert_eq!(cells[&(999, 99)], "LAST");
        assert!(!cells.contains_key(&(5, 0)));
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 999,
                right: 99,
            },
            column: 0,
            contains: "9".into(),
        }))
        .unwrap();
        let rows = b.visible_rows_from_cells(&cells);
        assert!(rows.contains(&0) && rows.contains(&3) && !rows.contains(&4));
    }
}
