//! Reference-aware summaries honor filters, manually hidden rows and nested totals.
use super::{finite, statistics, text::Arg, Parser, Value, MAX_STEPS};
impl Parser<'_, '_> {
    pub(super) fn summary(&mut self, name: &str) -> Value {
        let result = (|| -> Result<Value, &'static str> {
            let first = statistics::number(Some(&Arg::Scalar(self.comparison())), None)?.trunc();
            if !self.take(b',') {
                return Err("#VALUE!");
            }
            let (code, skip_hidden, skip_errors, skip_nested) = if name == "SUBTOTAL" {
                if !(1.0..=11.0).contains(&first) && !(101.0..=111.0).contains(&first) {
                    return Err("#VALUE!");
                }
                (first as usize % 100, first > 100.0, false, true)
            } else {
                if !(1.0..=19.0).contains(&first) {
                    return Err("#VALUE!");
                }
                let opt = statistics::number(Some(&Arg::Scalar(self.comparison())), None)?.trunc();
                if !(0.0..=7.0).contains(&opt) || !self.take(b',') {
                    return Err("#VALUE!");
                }
                (
                    first as usize,
                    matches!(opt as usize, 1 | 3 | 5 | 7),
                    matches!(opt as usize, 2 | 3 | 6 | 7),
                    opt <= 3.0,
                )
            };
            let mut values = Vec::new();
            let mut blanks = Vec::new();
            let mut args = 0;
            loop {
                args += 1;
                if args > 1024 {
                    return Err("#LIMIT!");
                }
                let start = self.pos;
                let range = self.read_range();
                self.space();
                if let Some((sheet, top, left, bottom, right)) =
                    range.filter(|_| matches!(self.input.get(self.pos), Some(b',' | b')')))
                {
                    for r in top..=bottom {
                        let source = self
                            .eval
                            .book
                            .history
                            .current()
                            .sheets
                            .iter()
                            .find(|s| s.id == sheet)
                            .ok_or("#REF!")?;
                        if skip_hidden && source.view.hidden_rows.contains(&r) {
                            continue;
                        }
                        if let Some(filter) = &source.filter {
                            if r > filter.range.top && r <= filter.range.bottom {
                                let column = filter.column;
                                let needle = filter.contains.to_lowercase();
                                let value = self
                                    .eval
                                    .cell(sheet, r, column, self.depth + 1)
                                    .display()
                                    .to_lowercase();
                                self.eval.steps =
                                    self.eval.steps.saturating_add(value.len() + needle.len());
                                if self.eval.steps > MAX_STEPS {
                                    return Err("#LIMIT!");
                                }
                                if !value.contains(&needle) {
                                    continue;
                                }
                            }
                        }
                        for c in left..=right {
                            self.eval.steps += 1;
                            if self.eval.steps > MAX_STEPS || values.len() >= 100_000 {
                                return Err("#LIMIT!");
                            }
                            let raw = self.eval.raw(sheet, r, c).ok_or("#REF!")?;
                            if skip_nested && raw.starts_with('=') && nested_summary(raw) {
                                continue;
                            }
                            let blank = raw.is_empty();
                            let value = self.eval.cell(sheet, r, c, self.depth + 1);
                            if skip_errors && matches!(value, Value::Error(_)) {
                                continue;
                            }
                            if !self.charge_argument_text(&value) {
                                return Err("#LIMIT!");
                            }
                            values.push(value);
                            blanks.push(blank);
                        }
                    }
                } else {
                    self.pos = start;
                    let value = self.comparison();
                    if !(skip_errors && matches!(value, Value::Error(_))) {
                        if !self.charge_argument_text(&value) {
                            return Err("#LIMIT!");
                        }
                        values.push(value);
                        blanks.push(false);
                    }
                }
                if code >= 14 {
                    break;
                }
                if self.take(b')') {
                    return Ok(summarize(code, values, blanks, None));
                }
                if !self.take(b',') {
                    return Err("#VALUE!");
                }
            }
            if !self.take(b',') {
                return Err("#VALUE!");
            }
            let k = self.comparison();
            if !self.take(b')') {
                return Err("#VALUE!");
            }
            Ok(summarize(code, values, blanks, Some(k)))
        })();
        result.unwrap_or_else(Value::Error)
    }
}
fn nested_summary(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let mut i = 0;
    let mut quoted = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            if quoted && bytes.get(i + 1) == Some(&b'"') {
                i += 2;
                continue;
            }
            quoted = !quoted;
            i += 1;
            continue;
        }
        if !quoted && (bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            let start = i;
            while bytes
                .get(i)
                .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.'))
            {
                i += 1;
            }
            let name = raw[start..i].to_ascii_uppercase();
            while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
                i += 1;
            }
            if bytes.get(i) == Some(&b'(')
                && matches!(name.trim_start_matches("_XLFN."), "SUBTOTAL" | "AGGREGATE")
            {
                return true;
            }
            continue;
        }
        i += 1;
    }
    false
}
fn summarize(code: usize, values: Vec<Value>, blanks: Vec<bool>, k: Option<Value>) -> Value {
    if code == 3 {
        return finite(values.iter().zip(&blanks).filter(|(_, b)| !**b).count() as f64);
    }
    if code == 2 {
        return finite(
            values
                .iter()
                .zip(&blanks)
                .filter(|(v, b)| !**b && matches!(v, Value::Number(_)))
                .count() as f64,
        );
    }
    let mut nums = Vec::new();
    for (v, blank) in values.iter().zip(&blanks) {
        if *blank {
            continue;
        }
        match v {
            Value::Error(e) => return Value::Error(e),
            Value::Number(n) => nums.push(*n),
            _ => {}
        }
    }
    match code {
        1 if nums.is_empty() => Value::Error("#DIV/0!"),
        1 => finite(nums.iter().sum::<f64>() / nums.len() as f64),
        4 => finite(nums.iter().copied().reduce(f64::max).unwrap_or(0.0)),
        5 => finite(nums.iter().copied().reduce(f64::min).unwrap_or(0.0)),
        6 => finite(if nums.is_empty() {
            0.0
        } else {
            nums.iter().product()
        }),
        9 => finite(nums.iter().sum()),
        _ => {
            let name = match code {
                7 => "STDEV.S",
                8 => "STDEV.P",
                10 => "VAR.S",
                11 => "VAR.P",
                12 => "MEDIAN",
                13 => "MODE.SNGL",
                14 => "LARGE",
                15 => "SMALL",
                16 => "PERCENTILE.INC",
                17 => "QUARTILE.INC",
                18 => "PERCENTILE.EXC",
                19 => "QUARTILE.EXC",
                _ => return Value::Error("#VALUE!"),
            };
            let mut args = vec![Arg::Range(values, blanks)];
            if let Some(k) = k {
                args.push(Arg::Scalar(k));
            }
            statistics::call(name, &args)
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Range, SheetFilter, Workbook};
    #[test]
    fn summaries_respect_hidden_filter_nested_errors_and_full_history() {
        let mut b = Workbook::new(30, 8).unwrap();
        for (r, value) in ["Amount", "1", "2", "3", "4", "=SUBTOTAL(9,A2:A5)", "=1/0"]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*value).into()).unwrap();
            b.set_cell(r, 1, if r == 3 { "skip" } else { "keep" }.into())
                .unwrap();
        }
        let cases = [
            ("=SUBTOTAL(9,A2:A6)", "10"),
            ("=SUBTOTAL(109,A2:A6)", "10"),
            ("=SUBTOTAL(2,A2:A7)", "4"),
            ("=SUBTOTAL(3,A1:A7)", "6"),
            ("=AGGREGATE(9,6,A2:A7)", "20"),
            ("=AGGREGATE(9,2,A2:A7)", "10"),
            ("=AGGREGATE(14,6,A2:A7,2)", "4"),
            ("=AGGREGATE(12,4,A2:A5)", "2.5"),
            ("=SUBTOTAL(9,OFFSET(A2,0,0,4,1))", "10"),
            ("=SUBTOTAL(9,A2:A7)", "#DIV/0!"),
        ];
        for (r, (f, _)) in cases.iter().enumerate() {
            b.set_cell(r, 4, (*f).into()).unwrap();
        }
        let baseline = b.display_grid();
        for (r, (f, e)) in cases.iter().enumerate() {
            assert_eq!(&baseline[r][4], e, "{f}");
        }
        let mut view = b.sheet_view().clone();
        view.hidden_rows.insert(2);
        b.set_sheet_view(view).unwrap();
        let grid = b.display_grid();
        assert_eq!(grid[0][4], "10");
        assert_eq!(grid[1][4], "8");
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 6,
                right: 1,
            },
            column: 1,
            contains: "keep".into(),
        }))
        .unwrap();
        let filtered = b.display_grid();
        assert_eq!(filtered[0][4], "7");
        assert_eq!(filtered[1][4], "5");
        assert_eq!(filtered[8][4], "7");
        b.undo();
        assert_eq!(b.display_grid(), grid);
        b.redo();
        assert_eq!(b.display_grid(), filtered);
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" {
                b.xlsx_bytes()
            } else {
                b.ods_bytes()
            }
            .unwrap();
            let restored = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(restored.display_grid(), filtered, "{kind}");
        }
    }
    #[test]
    fn quoted_function_names_are_not_nested_summaries_and_errors_have_types() {
        let mut b = Workbook::new(12, 5).unwrap();
        for (r, (f, e)) in [
            ("=SUBTOTAL(3,\"SUBTOTAL(\")", "1"),
            ("=SUBTOTAL(0,A1:A2)", "#VALUE!"),
            ("=AGGREGATE(20,0,A1:A2)", "#VALUE!"),
            ("=AGGREGATE(9,8,A1:A2)", "#VALUE!"),
            ("=SUBTOTAL(9,1,2,3)", "6"),
            ("=AGGREGATE(15,6,A1:A2,0)", "#NUM!"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(r, 3, (*f).into()).unwrap();
            assert_eq!(&b.display_grid()[r][3], e, "{f}");
        }
    }
}
