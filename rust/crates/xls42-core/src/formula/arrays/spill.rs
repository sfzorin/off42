use super::super::Eval;
use super::*;
use crate::{CellContent, Range, SheetId};
impl Eval<'_> {
    pub(in crate::formula) fn ensure_spills(&mut self, sheet: SheetId, depth: usize) {
        if self.spills_done.contains(&sheet)
            || self.spills_scanning.contains(&sheet)
            || self.steps >= MAX_STEPS
        {
            return;
        }
        self.spills_scanning.insert(sheet);
        let anchors: Vec<_> = self
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)
            .into_iter()
            .flat_map(|s| s.cells.iter())
            .filter(|(_, c)| {
                matches!(c.text, CellContent::Formula(_)) && dynamic_formula(c.text.raw())
            })
            .map(|(&(r, c), _)| (r, c))
            .collect();
        for &(r, c) in &anchors {
            if !self.cache.contains_key(&(sheet, r, c)) && !self.visiting.contains(&(sheet, r, c)) {
                self.cell(sheet, r, c, depth + 1);
            }
        }
        if anchors
            .iter()
            .all(|&(r, c)| self.cache.contains_key(&(sheet, r, c)))
        {
            self.spills_done.insert(sheet);
        }
        self.spills_scanning.remove(&sheet);
    }
    pub(in crate::formula) fn finish_array(
        &mut self,
        sheet: SheetId,
        row: usize,
        col: usize,
        raw: &str,
        m: Arc<Grid>,
    ) -> Value {
        if m.values.len() == 1 || !dynamic_formula(raw) {
            return m.implicit((row, col));
        }
        let Some(bottom) = row.checked_add(m.rows - 1) else {
            return Value::Error("#SPILL!");
        };
        let Some(right) = col.checked_add(m.cols - 1) else {
            return Value::Error("#SPILL!");
        };
        let Some(source) = self
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)
        else {
            return Value::Error("#REF!");
        };
        if bottom >= source.rows || right >= source.cols {
            return Value::Error("#SPILL!");
        }
        let range = Range {
            top: row,
            left: col,
            bottom,
            right,
        };
        if source.merges.iter().any(|r| r.overlaps(range))
            || source
                .protected_formula_ranges
                .iter()
                .any(|r| r.overlaps(range))
        {
            return Value::Error("#SPILL!");
        }
        if charge(&mut self.steps, m.values.len()).is_err() {
            return Value::Error("#LIMIT!");
        }
        for r in row..=bottom {
            for c in col..=right {
                if r == row && c == col {
                    continue;
                }
                if source
                    .cells
                    .get(&(r, c))
                    .is_some_and(|cell| !cell.text.raw().is_empty())
                    || self.spills.contains_key(&(sheet, r, c))
                {
                    return Value::Error("#SPILL!");
                }
            }
        }
        for r in 0..m.rows {
            for c in 0..m.cols {
                if r == 0 && c == 0 {
                    continue;
                }
                let key = (sheet, row + r, col + c);
                self.cache.remove(&key);
                self.spills.insert(key, m.at(r, c));
            }
        }
        self.spill_ranges.insert((sheet, row, col), range);
        m.at(0, 0)
    }
    pub(in crate::formula) fn range_value(
        &mut self,
        sheet: SheetId,
        row: usize,
        col: usize,
        depth: usize,
    ) -> (Value, bool) {
        let value = self.cell(sheet, row, col, depth);
        let blank =
            self.raw(sheet, row, col) == Some("") && !self.spills.contains_key(&(sheet, row, col));
        (
            if blank {
                Value::Text(String::new())
            } else {
                value
            },
            blank,
        )
    }
}
pub(in crate::formula) fn dynamic_formula(raw: &str) -> bool {
    let b = raw.as_bytes();
    let (mut i, mut quoted, mut sheet_quoted) = (0, false, false);
    while i < b.len() {
        if b[i] == b'"' && !sheet_quoted {
            if quoted && b.get(i + 1) == Some(&b'"') {
                i += 2;
                continue;
            }
            quoted = !quoted;
            i += 1;
            continue;
        }
        if b[i] == b'\'' && !quoted {
            if sheet_quoted && b.get(i + 1) == Some(&b'\'') {
                i += 2;
                continue;
            }
            sheet_quoted = !sheet_quoted;
            i += 1;
            continue;
        }
        if !quoted && !sheet_quoted && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
            let start = i;
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'))
            {
                i += 1;
            }
            let mut next = i;
            while b.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            let name = raw[start..i].to_ascii_uppercase();
            let name = name
                .trim_start_matches("_XLFN.")
                .trim_start_matches("_XLWS.");
            if b.get(next) == Some(&b'(') && dynamic(name) {
                return true;
            }
            continue;
        }
        i += 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    #[test]
    fn macos_spill_scenario_matches_values_conflicts_and_implicit_intersection() {
        let mut b = Workbook::new(20, 16).unwrap();
        for (r, n) in ["3", "1", "2"].iter().enumerate() {
            b.set_cell(r, 0, (*n).into()).unwrap();
        }
        for (r, c, f) in [
            (0, 2, "=SORT(A1:A3)"),
            (0, 3, "=SEQUENCE(2,3)"),
            (4, 3, "=C2*10"),
            (0, 7, "=TRANSPOSE(A1:A3)"),
            (2, 7, "x"),
            (1, 7, "=SEQUENCE(2)"),
            (1, 9, "=IF(A1:A3>1,\"y\",\"n\")"),
            (1, 10, "=IFERROR(1/(A1:A3-1),\"e\")"),
            (1, 11, "=LEN(A1:A3)&\"!\""),
        ] {
            b.set_cell(r, c, f.into()).unwrap();
        }
        let grid = b.display_grid();
        assert_eq!([&grid[0][2], &grid[1][2], &grid[2][2]], ["1", "2", "3"]);
        assert_eq!(grid[1][5], "6");
        assert_eq!(grid[4][3], "20");
        assert_eq!(&grid[0][7..10], ["3", "1", "2"]);
        assert_eq!(grid[1][7], "#SPILL!");
        assert_eq!(grid[1][9], "n");
        assert_eq!(grid[1][10], "e");
        assert_eq!(grid[1][11], "1!");
        assert_eq!(b.cell(1, 2), Some(""));
        b.set_cell(1, 2, "blocked".into()).unwrap();
        assert_eq!(b.display_grid()[0][2], "#SPILL!");
        assert_eq!(b.display_grid()[2][2], "");
        b.undo();
        assert_eq!(b.display_grid(), grid);
        b.redo();
        assert_eq!(b.display_grid()[0][2], "#SPILL!");
        b.undo();
        let now = chrono::NaiveDate::from_ymd_opt(2026, 11, 8)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        let calculation = super::super::super::calculation_at(&b, now);
        assert_eq!(
            calculation.spills[&(0, 2)],
            Range {
                top: 0,
                left: 2,
                bottom: 2,
                right: 2
            }
        );
        assert_eq!(calculation.values[&(1, 5)], Value::Number(6.0));
        let project = Workbook::from_project_bytes(&b.project_bytes().unwrap()).unwrap();
        assert_eq!(project.display_grid(), grid);
        let xlsx = b.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(xlsx.as_slice())).unwrap();
        let mut source = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("xl/worksheets/sheet1.xml").unwrap(),
            &mut source,
        )
        .unwrap();
        let document = roxmltree::Document::parse(&source).unwrap();
        let c1 = document
            .descendants()
            .find(|n| n.has_tag_name("c") && n.attribute("r") == Some("C1"))
            .unwrap();
        assert_eq!(c1.attribute("cm"), Some("1"));
        let f = c1.children().find(|n| n.has_tag_name("f")).unwrap();
        assert_eq!(f.attribute("t"), Some("array"));
        assert_eq!(f.attribute("ref"), Some("C1:C3"));
        let c2 = document
            .descendants()
            .find(|n| n.has_tag_name("c") && n.attribute("r") == Some("C2"))
            .unwrap();
        assert!(!c2.children().any(|n| n.has_tag_name("f")));
        assert_eq!(
            c2.children().find(|n| n.has_tag_name("v")).unwrap().text(),
            Some("2")
        );
        let h2 = document
            .descendants()
            .find(|n| n.has_tag_name("c") && n.attribute("r") == Some("H2"))
            .unwrap();
        assert_eq!(h2.attribute("t"), Some("e"));
        assert_eq!(
            h2.children().find(|n| n.has_tag_name("v")).unwrap().text(),
            Some("#SPILL!")
        );
        assert!(archive.by_name("xl/metadata.xml").is_ok());
        let reopened = Workbook::from_office_bytes(&xlsx, "xlsx").unwrap();
        assert_eq!(reopened.display_grid(), grid);
        let ods = b.ods_bytes().unwrap();
        assert_eq!(
            Workbook::from_office_bytes(&ods, "ods")
                .unwrap()
                .display_grid(),
            grid
        );
        let external = spreadsheet_ods::read_ods_buf(&ods).unwrap();
        assert_eq!(external.sheet(0).matrix_row_span(0, 2), 3);
        assert_eq!(external.sheet(0).matrix_col_span(0, 3), 3);
        assert_eq!(
            external.sheet(0).value(1, 2),
            &spreadsheet_ods::Value::Number(2.0)
        );
    }
    #[test]
    fn dependencies_starting_at_a_spill_child_and_shrinking_arrays_recompute() {
        let mut b = Workbook::new(16, 12).unwrap();
        b.set_cell(0, 0, "=SEQUENCE(3,2)".into()).unwrap();
        for (r, f, e) in [
            (0, "=COUNTA(A2:B3)", "4"),
            (1, "=SUM(A2:B3)", "18"),
            (2, "=COUNTBLANK(A2:B3)", "0"),
            (3, "=TEXTJOIN(\",\",TRUE,A2)", "3"),
            (4, "=ISBLANK(A2)", "FALSE"),
            (5, "=SUBTOTAL(9,A2:B3)", "18"),
        ] {
            b.set_cell(r, 5, f.into()).unwrap();
            assert_eq!(super::super::super::display_cell(&b, r, 5), e, "{f}");
        }
        b.set_cell(0, 0, "=SEQUENCE(2,1)".into()).unwrap();
        let grid = b.display_grid();
        assert_eq!(grid[1][0], "2");
        assert_eq!(grid[2][0], "");
        assert_eq!(grid[0][1], "");
        let source = b.project_bytes().unwrap();
        assert_eq!(b.cell(1, 0), Some(""));
        assert!(!String::from_utf8(source).unwrap().contains("spills"));
    }
}

#[cfg(test)]
mod bounds {
    use crate::{formula, Workbook};
    #[test]
    fn replicated_text_and_huge_arrays_are_refused_before_unbounded_allocation() {
        let mut book = Workbook::new(1000, 100).unwrap();
        book.set_cell(0, 0, "x".repeat(128 * 1024)).unwrap();
        let repeated = format!("=VSTACK({})", vec!["A1"; 200].join(","));
        book.set_cell(0, 1, repeated).unwrap();
        assert_eq!(formula::display_cell(&book, 0, 1), "#LIMIT!");
        for expression in [
            "=EXPAND(A1,1000,100,A1)",
            "=WRAPROWS({1},100000,A1)",
            "=CHOOSECOLS(A1,SEQUENCE(1,100000,1,0))",
            "=SEQUENCE(100000)&A1",
            "=SEQUENCE(100001)",
        ] {
            book.set_cell(0, 1, expression.into()).unwrap();
            assert_eq!(
                formula::display_cell(&book, 0, 1),
                "#LIMIT!",
                "{expression}"
            );
        }
    }
}

#[cfg(test)]mod metadata_tests{
use crate::{formula,Workbook};
#[test]fn cell_metadata_observes_typed_spill_values_and_empty_inputs(){
let mut b=Workbook::new(8,8).unwrap();
b.set_cell(0,0,"=SEQUENCE(2)".into()).unwrap();
b.set_cell(0,2,"=TEXTSPLIT(\"one,two\",\",\")".into()).unwrap();
for(r,f,e)in [(4,"=CELL(\"type\",A2)","v"),(5,"=CELL(\"type\",D1)","l"),(6,"=CELL(\"prefix\",D1)","'"),(7,"=CELL(\"type\",H1)","b")]{b.set_cell(r,0,f.into()).unwrap();assert_eq!(formula::display_cell(&b,r,0),e,"{f}");}
}}
