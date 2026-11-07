//! Computed references retain sheet/rectangle identity for aggregates and reference metadata.
use super::{
    finite, information, lookup_equal, lookup_order, statistics, text::Arg, wildcard, Parser,
    Value, MAX_DEPTH, MAX_STEPS,
};
use crate::SheetId;
type Result<T> = std::result::Result<T, &'static str>;
type Rect = (SheetId, usize, usize, usize, usize);
pub(super) fn handles(name: &str) -> bool {
    matches!(name, "OFFSET" | "INDIRECT" | "INDEX")
}
impl Parser<'_, '_> {
    pub(super) fn computed_range(&mut self) -> Option<Result<Rect>> {
        self.space();
        let start = self.pos;
        let mut end = start;
        while self
            .input
            .get(end)
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'))
        {
            end += 1;
        }
        let name = std::str::from_utf8(&self.input[start..end])
            .ok()?
            .to_ascii_uppercase();
        let name = name
            .trim_start_matches("_XLFN.")
            .trim_start_matches("_XLWS.");
        if !handles(name) {
            self.pos = start;
            return None;
        }
        self.pos = end;
        if !self.take(b'(') {
            self.pos = start;
            return None;
        }
        if self.depth >= MAX_DEPTH {
            return Some(Err("#LIMIT!"));
        }
        self.depth += 1;
        self.eval.steps += 1;
        let result = if self.eval.steps > MAX_STEPS {
            Err("#LIMIT!")
        } else {
            self.reference_call(name)
        };
        self.depth -= 1;
        Some(result)
    }
    pub(super) fn reference_value(&mut self, name: &str) -> Value {
        match self.reference_call(name) {
            Ok((sheet, top, left, bottom, right)) => {
                let row = if left == right && (top..=bottom).contains(&self.origin.0) {
                    self.origin.0
                } else {
                    top
                };
                let col = if top == bottom && (left..=right).contains(&self.origin.1) {
                    self.origin.1
                } else {
                    left
                };
                self.eval.cell(sheet, row, col, self.depth + 1)
            }
            Err(e) => Value::Error(e),
        }
    }
    fn ref_number(&mut self) -> Result<i64> {
        let n = statistics::number(Some(&Arg::Scalar(self.comparison())), None)?.trunc();
        if n.abs() >= 9e15 {
            Err("#NUM!")
        } else {
            Ok(n as i64)
        }
    }
    fn optional_ref_number(&mut self, default: i64) -> Result<i64> {
        self.space();
        if matches!(self.input.get(self.pos), Some(b',' | b')')) {
            Ok(default)
        } else {
            self.ref_number()
        }
    }
    fn checked_rect(
        &self,
        sheet: SheetId,
        top: i64,
        left: i64,
        height: i64,
        width: i64,
    ) -> Result<Rect> {
        if top < 0 || left < 0 || height <= 0 || width <= 0 {
            return Err("#REF!");
        }
        let bottom = top.checked_add(height - 1).ok_or("#REF!")?;
        let right = left.checked_add(width - 1).ok_or("#REF!")?;
        let s = self
            .eval
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)
            .ok_or("#REF!")?;
        if bottom >= s.rows as i64 || right >= s.cols as i64 {
            return Err("#REF!");
        }
        Ok((
            sheet,
            top as usize,
            left as usize,
            bottom as usize,
            right as usize,
        ))
    }
    fn required_range(&mut self) -> Result<Rect> {
        let start = self.pos;
        if let Some(result) = self.computed_range() {
            return result;
        }
        self.pos = start;
        self.read_range().ok_or("#REF!")
    }
    fn reference_call(&mut self, name: &str) -> Result<Rect> {
        if name == "INDIRECT" {
            let address = match self.comparison() {
                Value::Text(s) => s,
                Value::Error(e) => return Err(e),
                _ => return Err("#REF!"),
            };
            let a1 = if self.take(b',') {
                information::truth(self.comparison())?
            } else {
                true
            };
            if !self.take(b')') {
                return Err("#VALUE!");
            }
            if address.len() > 4096 || address.contains(['[', ']']) && a1 {
                return Err("#REF!");
            }
            if a1 {
                let mut p = Parser {
                    input: address.as_bytes(),
                    pos: 0,
                    eval: self.eval,
                    depth: self.depth,
                    sheet: self.sheet,
                    origin: self.origin,
                };
                // INDIRECT accepts addresses, never arbitrary formulas or external workbooks.
                let first = p.read_any_ref(p.sheet).ok_or("#REF!")?;
                let last = if p.take(b':') {
                    p.read_any_ref(first.0).ok_or("#REF!")?
                } else {
                    first
                };
                p.space();
                if p.pos != p.input.len()
                    || first.0 != last.0
                    || last.1 < first.1
                    || last.2 < first.2
                {
                    return Err("#REF!");
                }
                return p.checked_rect(
                    first.0,
                    first.1 as i64,
                    first.2 as i64,
                    (last.1 - first.1 + 1) as i64,
                    (last.2 - first.2 + 1) as i64,
                );
            }
            let (sheet, address) = self.r1c1_sheet(&address)?;
            let mut parts = address.split(':');
            let first = r1c1(parts.next().ok_or("#REF!")?, self.origin)?;
            let last = match parts.next() {
                Some(s) => r1c1(s, self.origin)?,
                None => first,
            };
            if parts.next().is_some() {
                return Err("#REF!");
            }
            return self.checked_rect(
                sheet,
                first.0,
                first.1,
                last.0 - first.0 + 1,
                last.1 - first.1 + 1,
            );
        }
        let (sheet, top, left, bottom, right) = self.required_range()?;
        if !self.take(b',') {
            return Err("#VALUE!");
        }
        if name == "OFFSET" {
            let rows = self.ref_number()?;
            if !self.take(b',') {
                return Err("#VALUE!");
            }
            let cols = self.ref_number()?;
            let height = if self.take(b',') {
                self.optional_ref_number((bottom - top + 1) as i64)?
            } else {
                (bottom - top + 1) as i64
            };
            let width = if self.take(b',') {
                self.optional_ref_number((right - left + 1) as i64)?
            } else {
                (right - left + 1) as i64
            };
            if !self.take(b')') {
                return Err("#VALUE!");
            }
            return self.checked_rect(
                sheet,
                (top as i64).checked_add(rows).ok_or("#REF!")?,
                (left as i64).checked_add(cols).ok_or("#REF!")?,
                height,
                width,
            );
        }
        let mut row = self.optional_ref_number(0)?;
        let has_col = self.take(b',');
        let mut col = if has_col {
            self.optional_ref_number(0)?
        } else {
            0
        };
        if !self.take(b')') {
            return Err("#VALUE!");
        }
        let (rows, cols) = ((bottom - top + 1) as i64, (right - left + 1) as i64);
        if !has_col && rows == 1 && cols > 1 {
            col = row;
            row = 1;
        }
        if col == 0 && cols == 1 {
            col = 1;
        }
        if row == 0 && rows == 1 {
            row = 1;
        }
        if row < 0 || col < 0 || row > rows || col > cols {
            return Err("#REF!");
        }
        self.checked_rect(
            sheet,
            top as i64 + if row == 0 { 0 } else { row - 1 },
            left as i64 + if col == 0 { 0 } else { col - 1 },
            if row == 0 { rows } else { 1 },
            if col == 0 { cols } else { 1 },
        )
    }
    fn r1c1_sheet<'a>(&self, address: &'a str) -> Result<(SheetId, &'a str)> {
        let Some((name, address)) = address.rsplit_once('!') else {
            return Ok((self.sheet, address));
        };
        let name = if name.starts_with('\'') && name.ends_with('\'') && name.len() >= 2 {
            name[1..name.len() - 1].replace("''", "'")
        } else {
            name.to_owned()
        };
        let sheet = self
            .eval
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.name.to_lowercase() == name.to_lowercase())
            .ok_or("#REF!")?;
        Ok((sheet.id, address))
    }
    pub(super) fn extended_lookup(&mut self, name: &str) -> Value {
        let result = (|| -> Result<Value> {
            let needle = self.comparison();
            if let Value::Error(e) = needle {
                return Err(e);
            }
            if !self.take(b',') {
                return Err("#VALUE!");
            }
            let range = self.required_range()?;
            let mode = if self.take(b',') {
                self.optional_ref_number(if name == "XMATCH" { 0 } else { 1 })?
            } else if name == "XMATCH" {
                0
            } else {
                return Err("#VALUE!");
            };
            let (search, approximate) = if name == "XMATCH" {
                let search = if self.take(b',') {
                    self.optional_ref_number(1)?
                } else {
                    1
                };
                if ![-1, 0, 1, 2].contains(&mode) || ![-2, -1, 1, 2].contains(&search) {
                    return Err("#VALUE!");
                }
                (search, false)
            } else {
                (
                    1,
                    if self.take(b',') {
                        information::truth(self.comparison())?
                    } else {
                        true
                    },
                )
            };
            if !self.take(b')') {
                return Err("#VALUE!");
            }
            let (rows, cols) = (range.3 - range.1 + 1, range.4 - range.2 + 1);
            if name == "XMATCH" && rows > 1 && cols > 1 {
                return Err("#VALUE!");
            }
            if name == "HLOOKUP" && (mode < 1 || mode > rows as i64) {
                return Err(if mode < 1 { "#VALUE!" } else { "#REF!" });
            }
            let length = if name == "HLOOKUP" || rows == 1 {
                cols
            } else {
                rows
            };
            let mut best: Option<(usize, Value)> = None;
            for offset in 0..length {
                let i = if search < 0 {
                    length - offset - 1
                } else {
                    offset
                };
                self.eval.steps += 1;
                if self.eval.steps > MAX_STEPS {
                    return Err("#LIMIT!");
                }
                let value = self.eval.cell(
                    range.0,
                    range.1 + if name == "XMATCH" && rows > 1 { i } else { 0 },
                    range.2 + if name == "HLOOKUP" || rows == 1 { i } else { 0 },
                    self.depth + 1,
                );
                if (name == "XMATCH" && mode == 2) || (name == "HLOOKUP" && !approximate) {
                    if let (Value::Text(pattern), Value::Text(text)) = (&needle, &value) {
                        self.eval.steps = self
                            .eval
                            .steps
                            .saturating_add(pattern.len().saturating_mul(text.len()));
                        if self.eval.steps > MAX_STEPS {
                            return Err("#LIMIT!");
                        }
                    }
                }
                let equal = lookup_equal(&needle, &value)
                    || ((name == "XMATCH" && mode == 2) || (name == "HLOOKUP" && !approximate))
                        && matches!((&needle,&value),(Value::Text(a),Value::Text(b))if wildcard(a,b));
                if equal {
                    return Ok(if name == "XMATCH" {
                        finite((i + 1) as f64)
                    } else {
                        self.eval.cell(
                            range.0,
                            range.1 + mode as usize - 1,
                            range.2 + i,
                            self.depth + 1,
                        )
                    });
                }
                let direction = if name == "HLOOKUP" && approximate {
                    -1
                } else if name == "XMATCH" && matches!(mode, -1 | 1) {
                    mode
                } else {
                    0
                };
                if direction != 0
                    && lookup_order(&value, &needle).is_some_and(|o| {
                        if direction < 0 {
                            o.is_lt()
                        } else {
                            o.is_gt()
                        }
                    })
                    && best.as_ref().is_none_or(|(_, old)| {
                        lookup_order(&value, old).is_some_and(|o| {
                            if direction < 0 {
                                o.is_gt()
                            } else {
                                o.is_lt()
                            }
                        })
                    })
                {
                    best = Some((i, value));
                }
            }
            let Some((i, _)) = best else {
                return Err("#N/A");
            };
            Ok(if name == "XMATCH" {
                finite((i + 1) as f64)
            } else {
                self.eval.cell(
                    range.0,
                    range.1 + mode as usize - 1,
                    range.2 + i,
                    self.depth + 1,
                )
            })
        })();
        result.unwrap_or_else(Value::Error)
    }
}
fn r1c1(input: &str, origin: (usize, usize)) -> Result<(i64, i64)> {
    fn part(input: &[u8], pos: &mut usize, prefix: u8, origin: usize) -> Result<i64> {
        if input.get(*pos).map(u8::to_ascii_uppercase) != Some(prefix) {
            return Err("#REF!");
        }
        *pos += 1;
        if input.get(*pos) == Some(&b'[') {
            *pos += 1;
            let start = *pos;
            while input.get(*pos).is_some_and(|c| *c != b']') {
                *pos += 1;
            }
            let n = std::str::from_utf8(&input[start..*pos])
                .ok()
                .and_then(|s| s.parse::<i64>().ok())
                .ok_or("#REF!")?;
            if input.get(*pos) != Some(&b']') {
                return Err("#REF!");
            }
            *pos += 1;
            return (origin as i64).checked_add(n).ok_or("#REF!");
        }
        let start = *pos;
        while input.get(*pos).is_some_and(u8::is_ascii_digit) {
            *pos += 1;
        }
        if *pos == start {
            return Ok(origin as i64);
        }
        std::str::from_utf8(&input[start..*pos])
            .ok()
            .and_then(|s| s.parse::<i64>().ok())
            .and_then(|n| n.checked_sub(1))
            .ok_or("#REF!")
    }
    let input = input.trim().as_bytes();
    let mut pos = 0;
    let row = part(input, &mut pos, b'R', origin.0)?;
    let col = part(input, &mut pos, b'C', origin.1)?;
    if pos != input.len() {
        return Err("#REF!");
    }
    Ok((row, col))
}

#[cfg(test)]
mod tests {
    use crate::Workbook;
    #[test]
    fn computed_reference_identity_bounds_relative_origin_and_history_round_trip() {
        let mut b = Workbook::new(48, 16).unwrap();
        for (r, (a, text)) in [
            (3, "apple"),
            (7, "Banana"),
            (2, "apple"),
            (9, "grape"),
            (7, "apple"),
        ]
        .into_iter()
        .enumerate()
        {
            b.set_cell(r, 0, a.to_string()).unwrap();
            b.set_cell(r, 1, text.into()).unwrap();
            b.set_cell(r, 2, (r + 1).to_string()).unwrap();
        }
        for c in 0..3 {
            b.set_cell(0, 4 + c, (c + 1).to_string()).unwrap();
            b.set_cell(1, 4 + c, (c + 4).to_string()).unwrap();
        }
        let first = b.active_sheet();
        let other = b.add_sheet("Data O'Brien!").unwrap();
        b.set_cell(0, 0, "101".into()).unwrap();
        b.select_sheet(first).unwrap();
        let cases = [
            ("=OFFSET(A1,2,1)", "apple"),
            ("=SUM(OFFSET(A1,0,0,3,1))", "12"),
            ("=SUM(OFFSET(OFFSET(A1,1,0),0,0,2,1))", "9"),
            ("=SUM(INDEX(A1:B5,0,1))", "28"),
            ("=SUM(INDEX(A1:C5,2,0))", "9"),
            ("=INDEX(E1:G2,2,2)", "5"),
            ("=INDEX(E1:G1,2)", "2"),
            ("=INDIRECT(\"A\"&4)", "9"),
            ("=SUM(INDIRECT(\"A1:A3\"))", "12"),
            ("=INDIRECT(\"R2C1\",FALSE)", "7"),
            ("=SUM(INDIRECT(\"R1C1:R3C1\",FALSE))", "12"),
            ("=INDIRECT(\"R1C[-9]\",FALSE)", "3"),
            ("=INDIRECT(\"'Data O''Brien!'!A1\")", "101"),
            ("=INDIRECT(\"'Data O''Brien!'!R1C1\",FALSE)", "101"),
            ("=ISREF(OFFSET(A1,1,0))", "TRUE"),
            ("=ROWS(OFFSET(A1,0,0,3,2))", "3"),
            ("=COLUMNS(INDEX(A1:C5,2,0))", "3"),
            ("=CELL(\"address\",OFFSET(A1,1,1))", "$B$2"),
            ("=XMATCH(9,A1:A5)", "4"),
            ("=XMATCH(6.5,A1:A5,1)", "2"),
            ("=XMATCH(6.5,A1:A5,-1)", "1"),
            ("=XMATCH(\"apple\",B1:B5,0,-1)", "5"),
            ("=XMATCH(\"gr?pe\",B1:B5,2)", "4"),
            ("=XMATCH(99,A1:A5)", "#N/A"),
            ("=HLOOKUP(2,E1:G2,2,FALSE)", "5"),
            ("=HLOOKUP(2.5,E1:G2,2)", "5"),
            ("=HLOOKUP(0,E1:G2,2)", "#N/A"),
            ("=HLOOKUP(2,E1:G2,3)", "#REF!"),
            ("=OFFSET(A1,-1,0)", "#REF!"),
            ("=SUM(OFFSET(A1,0,0,0,2))", "#REF!"),
            ("=INDIRECT(\"[other.xlsx]Sheet!A1\")", "#REF!"),
            ("=INDIRECT(\"SUM(A1)\")", "#REF!"),
            ("=INDIRECT(\"R[-99999999999999999999]C1\",FALSE)", "#REF!"),
            ("=INDEX(A1:B5,9,1)", "#REF!"),
        ];
        for (r, (f, _)) in cases.iter().enumerate() {
            b.set_cell(r, 9, (*f).into()).unwrap();
        }
        let grid = b.display_grid();
        for (r, (f, e)) in cases.iter().enumerate() {
            assert_eq!(&grid[r][9], e, "{f}");
        }
        let original = b.project_bytes().unwrap();
        let before = b.display_grid();
        b.set_cell(0, 0, "30".into()).unwrap();
        assert_eq!(b.display_grid()[1][9], "39");
        b.undo();
        assert_eq!(b.display_grid(), before);
        b.redo();
        assert_eq!(b.display_grid()[1][9], "39");
        let restored = Workbook::from_project_bytes(&original).unwrap();
        assert_eq!(restored.display_grid(), before);
        assert!(restored.sheets().iter().any(|s| s.0 == other));
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" {
                restored.xlsx_bytes()
            } else {
                restored.ods_bytes()
            }
            .unwrap();
            let loaded = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(loaded.display_grid(), before, "{kind}");
        }
    }
    #[test]
    fn computed_references_are_bounded_and_cycles_remain_errors() {
        let mut b = Workbook::new(10, 10).unwrap();
        b.set_cell(0, 0, "=INDIRECT(\"A1\")".into()).unwrap();
        assert_eq!(b.display_grid()[0][0], "#CYCLE!");
        b.set_cell(
            0,
            0,
            format!("={}A1{}", "OFFSET(".repeat(160), ",0,0)".repeat(160)),
        )
        .unwrap();
        assert_eq!(b.display_grid()[0][0], "#LIMIT!");
    }
}
