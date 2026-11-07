//! Scalar information and reference metadata without evaluating referenced formulas unnecessarily.
use super::{finite, statistics::number, text::Arg, Parser, Value};
use crate::CellContent;
pub(super) fn scalar_handles(n: &str) -> bool {
    matches!(
        n,
        "NA" | "ISLOGICAL"
            | "ISERR"
            | "ISEVEN"
            | "ISODD"
            | "TYPE"
            | "ERROR.TYPE"
            | "XOR"
            | "AND"
            | "OR"
            | "NOT"
            | "ADDRESS"
    )
}
pub(super) fn reference_handles(n: &str) -> bool {
    matches!(
        n,
        "ISREF"
            | "ISFORMULA"
            | "FORMULATEXT"
            | "CELL"
            | "SHEET"
            | "SHEETS"
            | "ROWS"
            | "COLUMNS"
            | "ROW"
            | "COLUMN"
    )
}
pub(super) const ERRORS: [&str; 12] = [
    "#DIV/0!",
    "#N/A",
    "#NAME?",
    "#NULL!",
    "#NUM!",
    "#REF!",
    "#VALUE!",
    "#GETTING_DATA",
    "#SPILL!",
    "#CALC!",
    "#CYCLE!",
    "#LIMIT!",
];
pub(super) fn truth(value: Value) -> Result<bool, &'static str> {
    match value {
        Value::Array(_) => Err("#VALUE!"),
        Value::Number(n) => Ok(n != 0.0),
        Value::Boolean(b) => Ok(b),
        Value::Error(e) => Err(e),
        Value::Text(s) => match s.to_uppercase().as_str() {
            "TRUE" | "ИСТИНА" | "DOĞRU" => Ok(true),
            "FALSE" | "ЛОЖЬ" | "YANLIŞ" => Ok(false),
            _ => Err("#VALUE!"),
        },
    }
}
fn scalar(arg: &Arg) -> Value {
    match arg {
        Arg::Scalar(v) => v.clone(),
        Arg::Range(v, b) if v.len() == 1 && b[0] => Value::Number(0.0),
        Arg::Range(v, _) if v.len() == 1 => v[0].clone(),
        Arg::Missing => Value::Number(0.0),
        _ => Value::Error("#VALUE!"),
    }
}
pub(super) fn call(n: &str, a: &[Arg]) -> Value {
    if n == "NA" {
        return Value::Error(if a.is_empty() { "#N/A" } else { "#VALUE!" });
    }
    if n == "ADDRESS" {
        return address(a);
    }
    if matches!(n, "XOR" | "AND" | "OR") {
        if a.is_empty() {
            return Value::Error("#VALUE!");
        }
        let mut count = 0;
        let mut result = n == "AND";
        for arg in a {
            if matches!(arg, Arg::Missing) {
                continue;
            }
            let (values, is_range) = match arg {
                Arg::Range(v, _) => (v.clone(), true),
                _ => (vec![scalar(arg)], false),
            };
            for v in values {
                match v {
                    Value::Number(value) => {
                        count += 1;
                        result = match n {
                            "AND" => result && (value != 0.0),
                            "OR" => result || (value != 0.0),
                            _ => result ^ (value != 0.0),
                        };
                    }
                    Value::Boolean(b) => {
                        count += 1;
                        result = match n {
                            "AND" => result && b,
                            "OR" => result || b,
                            _ => result ^ b,
                        };
                    }
                    Value::Error(e) => return Value::Error(e),
                    _ if is_range => {}
                    value => match truth(value) {
                        Ok(b) => {
                            count += 1;
                            result = match n {
                                "AND" => result && b,
                                "OR" => result || b,
                                _ => result ^ b,
                            };
                        }
                        Err(e) => return Value::Error(e),
                    },
                }
            }
        }
        return if count == 0 {
            Value::Error("#VALUE!")
        } else {
            Value::Boolean(result)
        };
    }
    if a.len() != 1 {
        return Value::Error("#VALUE!");
    }
    if n == "TYPE" && matches!(&a[0],Arg::Range(v,_)if v.len()>1) {
        return finite(64.0);
    }
    let v = scalar(&a[0]);
    match n {
        "NOT" => match truth(v) {
            Ok(v) => Value::Boolean(!v),
            Err(e) => Value::Error(e),
        },
        "TYPE" => finite(match v {
            Value::Array(_) => 64.0,
            Value::Number(_) => 1.0,
            Value::Text(_) => 2.0,
            Value::Boolean(_) => 4.0,
            Value::Error(_) => 16.0,
        }),
        "ISLOGICAL" => Value::Boolean(matches!(v, Value::Boolean(_))),
        "ISERR" => Value::Boolean(matches!(v,Value::Error(e)if e!="#N/A")),
        "ERROR.TYPE" => match v {
            Value::Error(e) => match e {
                "#NULL!" => finite(1.0),
                "#DIV/0!" => finite(2.0),
                "#VALUE!" => finite(3.0),
                "#REF!" => finite(4.0),
                "#NAME?" => finite(5.0),
                "#NUM!" => finite(6.0),
                "#N/A" => finite(7.0),
                "#GETTING_DATA" => finite(8.0),
                "#SPILL!" => finite(9.0),
                "#CALC!" => finite(14.0),
                _ => Value::Error("#N/A"),
            },
            _ => Value::Error("#N/A"),
        },
        "ISEVEN" | "ISODD" => match v {
            Value::Error(e) => Value::Error(e),
            Value::Text(_) => Value::Error("#VALUE!"),
            _ => match number(a.first(), None) {
                Ok(v) => Value::Boolean((v.trunc() % 2.0 == 0.0) == (n == "ISEVEN")),
                Err(e) => Value::Error(e),
            },
        },
        _ => Value::Error("#NAME?"),
    }
}
fn address(a: &[Arg]) -> Value {
    let result = (|| -> Result<String, &'static str> {
        if !(2..=5).contains(&a.len()) {
            return Err("#VALUE!");
        }
        let r = number(a.first(), None)?.trunc();
        let c = number(a.get(1), None)?.trunc();
        let mode = number(a.get(2), Some(1.0))?.trunc();
        let a1 = match a.get(3) {
            None | Some(Arg::Missing) => true,
            Some(arg) => truth(arg.scalar()?)?,
        };
        if !(1.0..=1048576.0).contains(&r)
            || !(1.0..=16384.0).contains(&c)
            || !(1.0..=4.0).contains(&mode)
        {
            return Err("#VALUE!");
        }
        let abs_row = mode == 1.0 || mode == 2.0;
        let abs_col = mode == 1.0 || mode == 3.0;
        let mut out = String::new();
        if let Some(arg) = a.get(4) {
            if !matches!(arg, Arg::Missing) {
                let sheet = arg.scalar()?.display();
                if !sheet.is_empty() {
                    let plain = sheet.chars().all(|c| c.is_alphanumeric() || c == '_')
                        && sheet.chars().next().is_some_and(|c| !c.is_numeric());
                    if plain {
                        out.push_str(&sheet);
                        out.push('!');
                    } else {
                        out.push('\'');
                        out.push_str(&sheet.replace('\'', "''"));
                        out.push_str("'!");
                    }
                }
            }
        }
        if a1 {
            if abs_col {
                out.push('$');
            }
            out.push_str(&crate::office::col_name(c as usize - 1));
            if abs_row {
                out.push('$');
            }
            out.push_str(&(r as u32).to_string());
        } else {
            out.push('R');
            out.push_str(&if abs_row {
                (r as i32).to_string()
            } else {
                format!("[{}]", r as i32)
            });
            out.push('C');
            out.push_str(&if abs_col {
                (c as i32).to_string()
            } else {
                format!("[{}]", c as i32)
            });
        }
        Ok(out)
    })();
    match result {
        Ok(s) => Value::Text(s),
        Err(e) => Value::Error(e),
    }
}
impl Parser<'_, '_> {
    pub(super) fn reference_information(&mut self, name: &str) -> Value {
        if self.take(b')') {
            return match name {
                "ROW" => finite(self.origin.0 as f64 + 1.0),
                "COLUMN" => finite(self.origin.1 as f64 + 1.0),
                "SHEET" => finite(
                    self.eval
                        .book
                        .history
                        .current()
                        .sheets
                        .iter()
                        .position(|s| s.id == self.sheet)
                        .unwrap_or(0) as f64
                        + 1.0,
                ),
                "SHEETS" => finite(self.eval.book.history.current().sheets.len() as f64),
                _ => Value::Error("#VALUE!"),
            };
        }
        let info = if name == "CELL" {
            let v = self.comparison();
            if let Value::Error(_) = v {
                return v;
            }
            if self.take(b')') {
                return self.cell_information(
                    &v.display(),
                    self.sheet,
                    self.origin.0,
                    self.origin.1,
                );
            }
            if !self.take(b',') {
                return Value::Error("#VALUE!");
            }
            Some(v.display())
        } else {
            None
        };
        let start = self.pos;
        let range = self.read_range();
        if let Some((sheet, top, left, bottom, right)) = range {
            if self.take(b')') {
                if self.eval.raw(sheet, top, left).is_none()
                    || self.eval.raw(sheet, bottom, right).is_none()
                {
                    return Value::Error("#REF!");
                }
                return match name {
                    "CELL" => self.cell_information(info.as_ref().unwrap(), sheet, top, left),
                    "ISREF" => Value::Boolean(true),
                    "ISFORMULA" => Value::Boolean(matches!(
                        self.eval.content(sheet, top, left),
                        Some(CellContent::Formula(_))
                    )),
                    "FORMULATEXT" => match self.eval.content(sheet, top, left) {
                        Some(CellContent::Formula(t)) => Value::Text(t.raw().into()),
                        _ => Value::Error("#N/A"),
                    },
                    "ROW" => finite(top as f64 + 1.0),
                    "COLUMN" => finite(left as f64 + 1.0),
                    "ROWS" => finite((bottom - top + 1) as f64),
                    "COLUMNS" => finite((right - left + 1) as f64),
                    "SHEET" => finite(
                        self.eval
                            .book
                            .history
                            .current()
                            .sheets
                            .iter()
                            .position(|s| s.id == sheet)
                            .unwrap_or(0) as f64
                            + 1.0,
                    ),
                    "SHEETS" => finite(1.0),
                    _ => Value::Error("#VALUE!"),
                };
            }
        }
        self.pos = start;
        let value = self.comparison();
        if !self.take(b')') {
            return Value::Error("#VALUE!");
        }
        match name {
            "ROWS" | "COLUMNS" => match value {
                Value::Array(m) => finite(if name == "ROWS" { m.rows } else { m.cols } as f64),
                Value::Error(e) => Value::Error(e),
                _ => finite(1.0),
            },
            "ISREF" => Value::Boolean(false),
            "SHEET" => match value {
                Value::Text(s) => self
                    .eval
                    .book
                    .history
                    .current()
                    .sheets
                    .iter()
                    .position(|sh| sh.name.to_lowercase() == s.to_lowercase())
                    .map(|i| finite(i as f64 + 1.0))
                    .unwrap_or(Value::Error("#N/A")),
                Value::Error(e) => Value::Error(e),
                _ => Value::Error("#N/A"),
            },
            "FORMULATEXT" | "SHEETS" => Value::Error("#N/A"),
            _ => Value::Error("#VALUE!"),
        }
    }
    fn cell_information(
        &mut self,
        info: &str,
        sheet: crate::SheetId,
        row: usize,
        col: usize,
    ) -> Value {
        let Some(sh) = self
            .eval
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)
        else {
            return Value::Error("#REF!");
        };
        let content = self.eval.content(sheet, row, col);
        match info.to_ascii_lowercase().as_str() {
            "address" => Value::Text(format!("${}${}", crate::office::col_name(col), row + 1)),
            "row" => finite(row as f64 + 1.0),
            "col" => finite(col as f64 + 1.0),
            "contents" => self.eval.cell(sheet, row, col, self.depth + 1),
            "type" => Value::Text(
                if content.is_none() {
                    "b"
                } else if matches!(content, Some(CellContent::Text(_))) {
                    "l"
                } else {
                    "v"
                }
                .into(),
            ),
            "width" => finite(
                (((sh.col_widths.get(&col).copied().unwrap_or(80) as f64 - 5.0).max(0.0) / 7.0
                    * 100.0)
                    .round()
                    / 100.0)
                    .trunc(),
            ),
            "filename" | "sheet" => Value::Text(sh.name.clone()),
            "prefix" => Value::Text(
                if matches!(content, Some(CellContent::Text(_))) {
                    "'"
                } else {
                    ""
                }
                .into(),
            ),
            _ => Value::Error("#VALUE!"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    #[test]
    fn information_conditions_and_conditional_extrema_match_reference() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/macos-function-cases.json")).unwrap();
        let cases = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| {
                let f = c["formula"].as_str().unwrap();
                let n = f.trim_start_matches('=').split('(').next().unwrap();
                !f.contains('{')
                    && ![
                        "FILTER(",
                        "SORT(",
                        "SORTBY(",
                        "UNIQUE(",
                        "SEQUENCE(",
                        "TRANSPOSE(",
                        "RANDARRAY(",
                        "VSTACK(",
                        "HSTACK(",
                        "TOCOL(",
                        "CHOOSECOLS(",
                        "TAKE(",
                        "DROP(",
                        "EXPAND(",
                        "WRAPROWS(",
                        "TEXTSPLIT(",
                        "MODE.MULT(",
                        "XMATCH(",
                    ]
                    .iter()
                    .any(|name| f.contains(name))
                    && (scalar_handles(n)
                        || reference_handles(n)
                        || matches!(
                            n,
                            "IFNA" | "IFS" | "SWITCH" | "CHOOSE" | "MAXIFS" | "MINIFS"
                        ))
            })
            .collect::<Vec<_>>();
        assert!(cases.len() >= 35);
        let mut b = Workbook::new(cases.len(), 24).unwrap();
        for (col, values) in [
            (10, vec!["3", "7", "2", "9", "4", "7", "1", "8", "5", "6"]),
            (
                11,
                vec![
                    "apple", "Banana", "cherry", "apple", "date", "Elder", "fig", "grape", "apple",
                    "kiwi",
                ],
            ),
            (
                12,
                vec![
                    "2024-01-15",
                    "2024-02-29",
                    "2023-12-31",
                    "2024-07-04",
                    "2025-03-10",
                    "2020-02-29",
                    "2024-11-04",
                    "2019-06-15",
                    "2024-12-25",
                    "2026-01-01",
                ],
            ),
            (17, vec!["TRUE", "text", "", "5", "=NA()", "0"]),
        ] {
            for (i, v) in values.iter().enumerate() {
                b.set_cell(i, col, (*v).into()).unwrap();
            }
        }
        for (i, c) in cases.iter().enumerate() {
            b.set_cell(i, 0, c["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = b.display_grid();
        for (i, c) in cases.iter().enumerate() {
            assert_eq!(
                grid[i][0],
                c["expected"].as_str().unwrap(),
                "{}",
                c["formula"]
            );
        }
    }
    #[test]
    fn metadata_origin_lazy_branches_errors_and_unicode() {
        let mut b = Workbook::new(30, 8).unwrap();
        b.set_cell(0, 0, "1".into()).unwrap();
        b.set_cell(1, 0, "2".into()).unwrap();
        for (i, (f, e)) in [
            ("=IFS(TRUE,7,1/0,8)", "7"),
            ("=SWITCH(1,1,7,1/0,8)", "7"),
            ("=CHOOSE(2,1/0,7)", "7"),
            ("=CHOOSE(\"2\",1/0,7)", "7"),
            ("=IF(FALSE,1/0)", "FALSE"),
            ("=IF(TRUE)", "TRUE"),
            ("=IF(TRUE,,1/0)", "0"),
            ("=IF(\"TRUE\",7,1/0)", "7"),
            ("=NOT(\"FALSE\")", "TRUE"),
            ("=AND(,TRUE)", "TRUE"),
            ("=AND(TRUE,1/0)", "#DIV/0!"),
            ("=OR(FALSE,#N/A)", "#N/A"),
            ("=TYPE(#N/A)", "16"),
            ("=TYPE(H1)", "1"),
            ("=ERROR.TYPE(#NUM!)", "6"),
            ("=IFNA(#N/A,7)", "7"),
            ("=ADDRESS(2,3,1,TRUE,\"Sheet1\")", "Sheet1!$C$2"),
            ("=ADDRESS(2,3,1,TRUE,\"\")", "$C$2"),
            ("=ISFORMULA(A1)", "FALSE"),
            ("=ISREF(1/0)", "FALSE"),
            ("=ROWS(A1:B3)", "3"),
            ("=SWITCH(\"ПЛАТА\",\"плата\",7,0)", "7"),
            ("=FORMULATEXT(A1)", "#N/A"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 2, (*f).into()).unwrap();
            assert_eq!(&b.display_grid()[i][2], e, "{f}");
        }
        for (f, e) in [
            ("=ROW()*100+COLUMN()", "504"),
            ("=IF(TRUE,CELL(\"address\"),0)", "$D$5"),
            ("=XLOOKUP(99,A1:A2,A1:A2,ROW())", "5"),
        ] {
            b.set_cell(4, 3, f.into()).unwrap();
            assert_eq!(b.display_grid()[4][3], e);
        }
        let second = b.add_sheet("Data sheet").unwrap();
        b.set_cell(0, 0, "9".into()).unwrap();
        b.select_sheet(1).unwrap();
        b.set_cell(0, 4, "=SHEET('Data sheet'!A1)".into()).unwrap();
        b.set_cell(1, 4, "=SHEETS()".into()).unwrap();
        b.set_cell(2, 4, "=CELL(\"contents\",'Data sheet'!A1)".into())
            .unwrap();
        assert_eq!(b.display_grid()[0][4], "2");
        assert_eq!(b.display_grid()[1][4], "2");
        assert_eq!(b.display_grid()[2][4], "9");
        assert_ne!(second, 1);
    }
}

#[cfg(test)]
mod external_error_tests {
    use crate::{CellContent, Workbook};
    use std::io::{Cursor, Read, Write};
    #[test]
    fn external_error_cells_remain_distinct_from_error_looking_text() {
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        sheet
            .write_formula(
                0,
                0,
                rust_xlsxwriter::Formula::new("=NA()").set_result("#N/A"),
            )
            .unwrap();
        sheet.write_string(1, 0, "#N/A").unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let mut part = zip.by_index(i).unwrap();
            if part.name() == "xl/worksheets/sheet1.xml" {
                let mut xml = String::new();
                part.read_to_string(&mut xml).unwrap();
                let doc = roxmltree::Document::parse(&xml).unwrap();
                let range = doc
                    .descendants()
                    .find(|n| n.has_tag_name("f"))
                    .unwrap()
                    .range();
                xml.replace_range(range, "");
                out.start_file(
                    "xl/worksheets/sheet1.xml",
                    zip::write::SimpleFileOptions::default(),
                )
                .unwrap();
                out.write_all(xml.as_bytes()).unwrap();
            } else {
                out.raw_copy_file(part).unwrap();
            }
        }
        let bytes = out.finish().unwrap().into_inner();
        let original = bytes.clone();
        let mut b = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert!(matches!(b.content(0, 0), Some(CellContent::Error(_))));
        assert!(matches!(b.content(1, 0), Some(CellContent::Text(_))));
        for (i, (f, e)) in [
            ("=IFNA(A1,7)", "7"),
            ("=ERROR.TYPE(A1)", "7"),
            ("=TYPE(A1)", "16"),
            ("=ISERROR(A1)", "TRUE"),
            ("=ISERROR(A2)", "FALSE"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 1, (*f).into()).unwrap();
            assert_eq!(&b.display_grid()[i][1], e, "{f}");
        }
        assert_eq!(bytes, original);
    }
}
