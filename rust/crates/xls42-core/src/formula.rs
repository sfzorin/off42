//! Small, bounded spreadsheet expression evaluator. Raw cells remain authoritative.
mod arrays;
mod calendar;
mod references;
mod summaries;
mod distributions;
mod engineering;
mod finance;
mod math;
mod information;
mod statistics;
mod text;
mod units;
use crate::{CellContent, SheetId, Workbook};
use std::collections::{HashMap, HashSet};

const MAX_DEPTH: usize = 128;
const MAX_STEPS: usize = 1_000_000;
const MAX_FORMULA: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Value {
    Array(std::sync::Arc<arrays::Grid>),
    Number(f64),
    Text(String),
    Boolean(bool),
    Error(&'static str),
}
impl Value {
    pub(super) fn display(self) -> String {
        match self {
            Self::Array(m) => m.at(0, 0).display(),
            Self::Number(0.0) => "0".into(),
            Self::Number(n) => n.to_string(),
            Self::Text(s) => s,
            Self::Boolean(v) => {
                if v {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                }
            }
            Self::Error(e) => e.into(),
        }
    }
}
struct Eval<'a> {
    book: &'a Workbook,
    cache: HashMap<(SheetId, usize, usize), Value>,
    visiting: HashSet<(SheetId, usize, usize)>,
    steps: usize,
    text_bytes: usize,
    argument_text_bytes: usize,
    now: chrono::NaiveDateTime,
    spills: HashMap<(SheetId, usize, usize), Value>,
    spill_ranges: std::collections::BTreeMap<(SheetId, usize, usize), crate::Range>,
    spills_done: HashSet<SheetId>,
    spills_scanning: HashSet<SheetId>,
}
impl<'a> Eval<'a> {
    fn new(book: &'a Workbook, now: chrono::NaiveDateTime) -> Self {
        Self {
            book,
            cache: HashMap::new(),
            visiting: HashSet::new(),
            steps: 0,
            text_bytes: 0,
            argument_text_bytes: 0,
            now,
            spills: HashMap::new(),
            spill_ranges: std::collections::BTreeMap::new(),
            spills_done: HashSet::new(),
            spills_scanning: HashSet::new(),
        }
    }
    fn raw(&self, sheet: SheetId, row: usize, col: usize) -> Option<&str> {
        let s = self
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)?;
        if row >= s.rows || col >= s.cols {
            return None;
        }
        Some(s.cells.get(&(row, col)).map_or("", |c| c.text.raw()))
    }
    fn content(&self, sheet: SheetId, row: usize, col: usize) -> Option<&CellContent> {
        let s = self
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == sheet)?;
        s.cells.get(&(row, col)).map(|c| &c.text)
    }
    fn display_for_filter(
        &mut self,
        sheet: SheetId,
        row: usize,
        col: usize,
        depth: usize,
    ) -> String {
        let (value, blank) = self.range_value(sheet, row, col, depth);
        if blank {
            return String::new();
        }
        let mut text = match self.content(sheet, row, col) {
            Some(CellContent::Text(v) | CellContent::Number(v) | CellContent::Date(v)) => {
                v.raw().to_owned()
            }
            _ => value.clone().display(),
        };
        crate::display::format_cell(self.book, sheet, row, col, Some(&value), &mut text);
        text
    }
    fn range_cell(
        &mut self,
        sheet: SheetId,
        row: usize,
        col: usize,
        depth: usize,
    ) -> Option<Value> {
        self.raw(sheet, row, col)?;
        self.ensure_spills(sheet, depth);
        if let Some(value) = self.spills.get(&(sheet, row, col)) {
            return match value {
                Value::Number(_) | Value::Error(_) => Some(value.clone()),
                _ => None,
            };
        }
        if !matches!(
            self.content(sheet, row, col),
            Some(
                CellContent::Number(_)
                    | CellContent::Date(_)
                    | CellContent::Formula(_)
                    | CellContent::Error(_)
            )
        ) {
            return None;
        }
        Some(self.cell(sheet, row, col, depth))
    }
    fn cell(&mut self, sheet: SheetId, row: usize, col: usize, depth: usize) -> Value {
        if self.raw(sheet, row, col).is_none() {
            return Value::Error("#REF!");
        }
        let key = (sheet, row, col);
        if self.raw(sheet, row, col) == Some("") {
            self.ensure_spills(sheet, depth);
            return self.spills.get(&key).cloned().unwrap_or(Value::Number(0.0));
        }
        if let Some(v) = self.cache.get(&key) {
            return v.clone();
        }
        if self.visiting.contains(&key) {
            return Value::Error("#CYCLE!");
        }
        if depth > MAX_DEPTH || self.steps >= MAX_STEPS {
            return Value::Error("#LIMIT!");
        }
        self.steps += 1;
        self.visiting.insert(key);
        let raw = self.raw(sheet, row, col).unwrap().to_owned();
        let v = if matches!(self.content(sheet, row, col), Some(CellContent::Formula(_))) {
            let expr = raw.strip_prefix('=').unwrap();
            if expr.len() > MAX_FORMULA {
                Value::Error("#LIMIT!")
            } else {
                let mut p = Parser {
                    input: expr.as_bytes(),
                    pos: 0,
                    eval: self,
                    depth,
                    sheet,
                    origin: (row, col),
                    locals: HashMap::new(),
                };
                let result = p.comparison();
                p.space();
                if p.pos != p.input.len() && !matches!(result, Value::Error(_)) {
                    Value::Error("#ERROR!")
                } else {
                    result
                }
            }
        } else if raw.trim().is_empty() {
            Value::Number(0.0)
        } else if matches!(self.content(sheet, row, col), Some(CellContent::Number(_))) {
            match raw.trim().parse::<f64>() {
                Ok(n) if n.is_finite() => Value::Number(n),
                _ => Value::Error("#VALUE!"),
            }
        } else if matches!(self.content(sheet, row, col), Some(CellContent::Date(_))) {
            match crate::display::workbook_parse_serial(self.book, &raw)
                .or_else(|| raw.parse::<f64>().ok())
            {
                Some(v) => finite(v),
                None => Value::Error("#VALUE!"),
            }
        } else if matches!(self.content(sheet, row, col), Some(CellContent::Error(_))) {
            Value::Error(
                information::ERRORS
                    .iter()
                    .copied()
                    .find(|e| *e == raw)
                    .unwrap_or("#VALUE!"),
            )
        } else if matches!(self.content(sheet, row, col), Some(CellContent::Boolean(_))) {
            Value::Boolean(raw.eq_ignore_ascii_case("true"))
        } else {
            Value::Text(raw.clone())
        };
        let v = match v {
            Value::Array(m) => self.finish_array(sheet, row, col, &raw, m),
            v => v,
        };
        self.visiting.remove(&key);
        let v = match v {
            Value::Text(t)
                if t.len() > crate::MAX_TEXT
                    || self.text_bytes.saturating_add(t.len()) > crate::MAX_TEXT * 2 =>
            {
                Value::Error("#LIMIT!")
            }
            Value::Text(t) => {
                self.text_bytes += t.len();
                Value::Text(t)
            }
            v => v,
        };
        self.cache.insert(key, v.clone());
        v
    }
}
struct Parser<'a, 'b> {
    input: &'a [u8],
    pos: usize,
    eval: &'a mut Eval<'b>,
    depth: usize,
    sheet: SheetId,
    origin: (usize, usize),
    locals: HashMap<String, Value>,
}
impl Parser<'_, '_> {
    fn space(&mut self) {
        while self
            .input
            .get(self.pos)
            .is_some_and(u8::is_ascii_whitespace)
        {
            self.pos += 1;
        }
    }
    fn take(&mut self, c: u8) -> bool {
        self.space();
        if self.input.get(self.pos) == Some(&c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn comparison(&mut self) -> Value {
        let left = self.expr();
        self.space();
        let remaining = &self.input[self.pos..];
        let (op, len) = if remaining.starts_with(b"<=") {
            (0, 2)
        } else if remaining.starts_with(b">=") {
            (1, 2)
        } else if remaining.starts_with(b"<>") {
            (2, 2)
        } else if remaining.starts_with(b"=") {
            (3, 1)
        } else if remaining.starts_with(b"<") {
            (4, 1)
        } else if remaining.starts_with(b">") {
            (5, 1)
        } else {
            return left;
        };
        self.pos += len;
        let right = self.expr();
        if matches!(left, Value::Array(_)) || matches!(right, Value::Array(_)) {
            arrays::broadcast(&[left, right], &mut self.eval.steps, |v| {
                compare_scalar(v[0].clone(), v[1].clone(), op)
            })
        } else {
            compare_scalar(left, right, op)
        }
    }

    fn expr(&mut self) -> Value {
        let mut v = self.additive();
        while self.take(b'&') {
            let right = self.additive();
            v = if matches!(v, Value::Array(_)) || matches!(right, Value::Array(_)) {
                arrays::broadcast(&[v, right], &mut self.eval.steps, |v| {
                    concat_scalar(v[0].clone(), v[1].clone())
                })
            } else {
                concat_scalar(v, right)
            };
        }
        v
    }
    fn additive(&mut self) -> Value {
        let mut v = self.term();
        loop {
            if self.take(b'+') {
                let r = self.term();
                v = self.binary(v, r, b'+');
            } else if self.take(b'-') {
                let r = self.term();
                v = self.binary(v, r, b'-');
            } else {
                return v;
            }
        }
    }
    fn term(&mut self) -> Value {
        let mut v = self.unary();
        loop {
            if self.take(b'*') {
                let r = self.unary();
                v = self.binary(v, r, b'*');
            } else if self.take(b'/') {
                let r = self.unary();
                v = self.binary(v, r, b'/');
            } else {
                return v;
            }
        }
    }
    fn unary(&mut self) -> Value {
        self.space();
        if self.depth >= MAX_DEPTH {
            return Value::Error("#LIMIT!");
        }
        if self.take(b'+') {
            self.depth += 1;
            let v = self.unary();
            self.depth -= 1;
            v
        } else if self.take(b'-') {
            self.depth += 1;
            let v = self.unary();
            self.depth -= 1;
            match v {
                Value::Array(m) => {
                    arrays::broadcast(&[Value::Array(m)], &mut self.eval.steps, |values| {
                        calc(Value::Number(0.0), values[0].clone(), b'-')
                    })
                }
                Value::Number(n) => Value::Number(-n),
                Value::Boolean(v) => Value::Number(if v { -1.0 } else { 0.0 }),
                Value::Error(e) => Value::Error(e),
                Value::Text(_) => Value::Error("#VALUE!"),
            }
        } else {
            self.power()
        }
    }
    fn power(&mut self) -> Value {
        let mut left = self.primary();
        while self.take(b'%') {
            left = self.binary(left, Value::Number(100.0), b'/');
        }
        if self.take(b'^') {
            self.depth += 1;
            let right = self.unary();
            self.depth -= 1;
            self.binary(left, right, b'^')
        } else {
            left
        }
    }
    fn primary(&mut self) -> Value {
        self.space();
        if self.take(b'{') {
            return self.array_literal();
        }
        if self.take(b'(') {
            self.depth += 1;
            let v = self.comparison();
            self.depth -= 1;
            return if self.take(b')') {
                v
            } else {
                Value::Error("#ERROR!")
            };
        }
        if self.input.get(self.pos) == Some(&b'"') {
            self.pos += 1;
            let mut result = String::new();
            loop {
                let Some(&ch) = self.input.get(self.pos) else {
                    return Value::Error("#ERROR!");
                };
                if ch == b'"' {
                    self.pos += 1;
                    if self.input.get(self.pos) == Some(&b'"') {
                        result.push('"');
                        self.pos += 1
                    } else {
                        break;
                    }
                } else {
                    let Ok(t) = std::str::from_utf8(&self.input[self.pos..]) else {
                        return Value::Error("#ERROR!");
                    };
                    let ch = t.chars().next().unwrap();
                    result.push(ch);
                    self.pos += ch.len_utf8();
                }
            }
            return Value::Text(result);
        }
        for error in information::ERRORS {
            if self.input[self.pos..].starts_with(error.as_bytes()) {
                self.pos += error.len();
                return Value::Error(error);
            }
        }
        let start = self.pos;
        let tail = std::str::from_utf8(&self.input[start..]).unwrap_or("");
        let end = tail
            .char_indices()
            .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '.'))
            .last()
            .map(|(i, c)| start + i + c.len_utf8())
            .unwrap_or(start);
        if end > start {
            if let Some(value) = self.locals.get(
                &std::str::from_utf8(&self.input[start..end])
                    .unwrap()
                    .to_uppercase(),
            ) {
                self.pos = end;
                return value.clone();
            }
        }
        // Function names can contain digits and Excel compatibility prefixes.
        // Recognize the complete token followed by '(' before trying A1 references.
        if self
            .input
            .get(start)
            .is_some_and(|b| b.is_ascii_alphabetic() || *b == b'_')
        {
            let mut end = start;
            while self
                .input
                .get(end)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
            {
                end += 1;
            }
            let mut next = end;
            while self.input.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if self.input.get(next) == Some(&b'(') {
                let mut name = std::str::from_utf8(&self.input[start..end])
                    .unwrap_or("")
                    .to_ascii_uppercase();
                loop {
                    if let Some(tail) = name
                        .strip_prefix("_XLFN.")
                        .or_else(|| name.strip_prefix("_XLWS."))
                    {
                        name = tail.to_owned();
                    } else {
                        break;
                    }
                }
                self.pos = next + 1;
                return self.named_call(&name);
            }
        }
        let reference_start = self.pos;
        if let Some(range) = self.read_range() {
            if range.1 != range.3 || range.2 != range.4 {
                return self.array_range(range);
            }
        }
        self.pos = reference_start;
        if let Some((sheet, row, col)) = self.read_qualified_ref() {
            return self.eval.cell(sheet, row, col, self.depth + 1);
        }
        self.pos = start;
        if self.input.get(self.pos) == Some(&b'$')
            || self
                .input
                .get(self.pos)
                .is_some_and(u8::is_ascii_alphabetic)
        {
            if self.input.get(self.pos) == Some(&b'$') {
                self.pos += 1;
            }
            while self
                .input
                .get(self.pos)
                .is_some_and(u8::is_ascii_alphabetic)
            {
                self.pos += 1;
            }
            let letters_end = self.pos;
            if self.input.get(self.pos) == Some(&b'$') {
                self.pos += 1;
            }
            let digits_start = self.pos;
            while self.input.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
            }
            if self.pos > digits_start {
                return match reference(&self.input[start..self.pos]) {
                    Some((r, c)) => self.eval.cell(self.sheet, r, c, self.depth + 1),
                    None => Value::Error("#REF!"),
                };
            }
            let name = std::str::from_utf8(&self.input[start..letters_end])
                .unwrap_or("")
                .to_ascii_uppercase();
            if !self.take(b'(') {
                return match name.as_str() {
                    "TRUE" => Value::Boolean(true),
                    "FALSE" => Value::Boolean(false),
                    _ => Value::Error("#ERROR!"),
                };
            }
            return self.named_call(&name);
        }
        while self
            .input
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_digit() || *c == b'.')
        {
            self.pos += 1;
        }
        if start == self.pos {
            return Value::Error("#ERROR!");
        }
        if matches!(self.input.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.input.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            let exponent_start = self.pos;
            while self.input.get(self.pos).is_some_and(u8::is_ascii_digit) {
                self.pos += 1;
            }
            if self.pos == exponent_start {
                return Value::Error("#ERROR!");
            }
        }
        match std::str::from_utf8(&self.input[start..self.pos])
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
        {
            Some(n) => finite(n),
            None => Value::Error("#ERROR!"),
        }
    }
    fn named_call(&mut self, name: &str) -> Value {
        self.depth += 1;
        let value = if self.depth > MAX_DEPTH {
            Value::Error("#LIMIT!")
        } else if matches!(
            name,
            "IF" | "IFERROR" | "IFNA" | "IFS" | "SWITCH" | "CHOOSE" | "LET"
        ) {
            self.lazy_conditional(name)
        } else {
            self.function(name)
        };
        self.depth -= 1;
        value
    }
    fn function(&mut self, name: &str) -> Value {
        if arrays::handles(name) {
            return self.array_function(name);
        }
        if matches!(name, "SUBTOTAL" | "AGGREGATE") {
            return self.summary(name);
        }
        if references::handles(name) {
            return self.reference_value(name);
        }
        if matches!(name, "XMATCH" | "HLOOKUP") {
            return self.extended_lookup(name);
        }
        if self.depth > MAX_DEPTH {
            return Value::Error("#LIMIT!");
        }
        if text::handles(name)
            || statistics::handles(name)
            || distributions::handles(name)
            || finance::handles(name)
            || calendar::handles(name)
            || information::scalar_handles(name)
        {
            return self.extended_text_function(name);
        }
        if matches!(name, "TRUE" | "FALSE") {
            return if self.take(b')') {
                Value::Boolean(name == "TRUE")
            } else {
                Value::Error("#ERROR!")
            };
        }
        if information::reference_handles(name) {
            return self.reference_information(name);
        }
        if name == "LOOKUP" {
            return self.lookup();
        }
        if name == "XLOOKUP" {
            return self.xlookup();
        }
        if matches!(
            name,
            "COUNTIFS" | "SUMIFS" | "AVERAGEIF" | "AVERAGEIFS" | "MAXIFS" | "MINIFS"
        ) {
            return self.conditional_aggregate(name);
        }
        if engineering::handles(name) {
            return self.text_function(name);
        }
        if matches!(name, "ISBLANK" | "ISNUMBER" | "ISTEXT" | "ISERROR" | "ISNA") {
            return self.type_predicate(name);
        }
        if matches!(name, "COUNTIF" | "SUMIF" | "MATCH" | "VLOOKUP") {
            return self.range_function(name);
        }
        if matches!(
            name,
            "LEN"
                | "LOWER"
                | "UPPER"
                | "TRIM"
                | "LEFT"
                | "RIGHT"
                | "MID"
                | "EXACT"
                | "VALUE"
                | "DATEVALUE"
                | "TEXT"
        ) {
            return self.text_function(name);
        }
        let aggregate = matches!(
            name,
            "SUM" | "AVERAGE" | "MIN" | "MAX" | "COUNT" | "COUNTA" | "PRODUCT"
        );
        let mut array_arguments = Vec::new();
        let mut first_error = None;
        let mut argument_count = 0usize;
        let mut values = Vec::new();
        let mut nonempty = 0usize;
        if !self.take(b')') {
            loop {
                argument_count += 1;
                let start = self.pos;
                let range = if aggregate || math::aggregate(name) {
                    self.read_range()
                } else {
                    None
                };
                self.space();
                if let Some((sheet, top, left, bottom, right)) =
                    range.filter(|_| matches!(self.input.get(self.pos), Some(b',' | b')')))
                {
                    self.eval.ensure_spills(sheet, self.depth + 1);
                    for r in top..=bottom {
                        for c in left..=right {
                            self.eval.steps += 1;
                            if self.eval.steps > MAX_STEPS {
                                return Value::Error("#LIMIT!");
                            }
                            if !self.eval.raw(sheet, r, c).unwrap_or("").is_empty()
                                || self.eval.spills.contains_key(&(sheet, r, c))
                            {
                                nonempty += 1;
                            }
                            match self.eval.range_cell(sheet, r, c, self.depth + 1) {
                                Some(Value::Number(n)) => values.push(n),
                                Some(Value::Error(e)) if name != "COUNT" && name != "COUNTA" => {
                                    return Value::Error(e)
                                }
                                _ => {}
                            }
                        }
                    }
                } else {
                    self.pos = start;
                    let operand = self.comparison();
                    if !self.charge_argument_text(&operand) {
                        return Value::Error("#LIMIT!");
                    }
                    array_arguments.push(operand.clone());
                    match operand {
                        Value::Array(m) => {
                            if arrays::charge(&mut self.eval.steps, m.values.len()).is_err() {
                                return Value::Error("#LIMIT!");
                            }
                            for (i, value) in m.values.iter().enumerate() {
                                if m.blanks[i] {
                                    continue;
                                }
                                nonempty += 1;
                                match value {
                                    Value::Number(n) => values.push(*n),
                                    Value::Error(e) if !matches!(name, "COUNT" | "COUNTA") => {
                                        return Value::Error(e)
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Value::Number(n) => {
                            values.push(n);
                            nonempty += 1
                        }
                        Value::Error(e) if aggregate || math::aggregate(name) => {
                            return Value::Error(e)
                        }
                        Value::Error(e) => {
                            first_error.get_or_insert(e);
                        }
                        Value::Boolean(value) => {
                            values.push(f64::from(value));
                            nonempty += 1;
                        }
                        Value::Text(_) if name == "COUNTA" => nonempty += 1,
                        Value::Text(value) => match value.trim().parse::<f64>() {
                            Ok(number) if number.is_finite() => {
                                values.push(number);
                                nonempty += 1;
                            }
                            _ if name == "COUNT" => {}
                            _ => {
                                first_error.get_or_insert("#VALUE!");
                            }
                        },
                    }
                }
                if self.take(b')') {
                    break;
                }
                if !self.take(b',') {
                    return Value::Error("#ERROR!");
                }
                if values.len() > MAX_STEPS {
                    return Value::Error("#LIMIT!");
                }
            }
        }
        if !aggregate
            && !math::aggregate(name)
            && array_arguments.iter().any(|v| matches!(v, Value::Array(_)))
        {
            if math::handles(name)
                || matches!(
                    name,
                    "DATE"
                        | "YEAR"
                        | "MONTH"
                        | "DAY"
                        | "TIME"
                        | "CEILING"
                        | "FLOOR"
                        | "ABS"
                        | "SQRT"
                        | "SIN"
                        | "COS"
                        | "TAN"
                        | "EXP"
                        | "LOG10"
                        | "RADIANS"
                        | "DEGREES"
                        | "POWER"
                        | "MOD"
                        | "LOG"
                        | "ROUND"
                        | "ROUNDUP"
                        | "ROUNDDOWN"
                        | "FACT"
                        | "LN"
                        | "INT"
                        | "SIGN"
                )
            {
                return self.lift_scalar(name, &array_arguments);
            }
            return Value::Error("#ERROR!");
        }
        if let Some(e) = first_error {
            return Value::Error(e);
        }
        if math::handles(name) {
            return math::call(name, &values, argument_count);
        }
        if aggregate {
            return match name {
                "COUNT" => finite(values.len() as f64),
                "COUNTA" => finite(nonempty as f64),
                "SUM" => finite(values.iter().sum()),
                "PRODUCT" => finite(if values.is_empty() {
                    0.0
                } else {
                    values.iter().product()
                }),
                "AVERAGE" if values.is_empty() => Value::Error("#DIV/0!"),
                "AVERAGE" => finite(values.iter().sum::<f64>() / values.len() as f64),
                "MIN" => finite(values.iter().copied().reduce(f64::min).unwrap_or(0.0)),
                "MAX" => finite(values.iter().copied().reduce(f64::max).unwrap_or(0.0)),
                _ => Value::Error("#ERROR!"),
            };
        }
        let arg = |n: usize| values.get(n).copied();
        let one = values.len() == 1;
        match name {
            "DATE" if values.len() == 3 => {
                if values
                    .iter()
                    .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
                {
                    return Value::Error("#NUM!");
                }
                let y = values[0].trunc() as i32;
                let m = values[1].trunc() as i64;
                let d = values[2].trunc() as i64;
                let year = if (0..1900).contains(&y) { y + 1900 } else { y };
                let normalized = (year as i64) * 12 + m - 1;
                let y = normalized.div_euclid(12);
                let month = normalized.rem_euclid(12) + 1;
                let Some(date) = i32::try_from(y)
                    .ok()
                    .and_then(|y| chrono::NaiveDate::from_ymd_opt(y, month as u32, 1))
                else {
                    return Value::Error("#NUM!");
                };
                match crate::display::workbook_date_to_serial(self.eval.book, date) {
                    Some(n) if n + (d - 1) as f64 >= 0.0 => finite(n + (d - 1) as f64),
                    _ => Value::Error("#NUM!"),
                }
            }
            "YEAR" | "MONTH" | "DAY" if one => {
                if !self.eval.book.history.current().date_1904 && values[0].floor() == 60.0 {
                    return finite(match name {
                        "YEAR" => 1900.0,
                        "MONTH" => 2.0,
                        _ => 29.0,
                    });
                }
                use chrono::Datelike;
                match crate::display::workbook_serial_to_date(self.eval.book, values[0]) {
                    Some(d) => finite(match name {
                        "YEAR" => d.year() as f64,
                        "MONTH" => d.month() as f64,
                        _ => d.day() as f64,
                    }),
                    None => Value::Error("#NUM!"),
                }
            }
            "TODAY" if values.is_empty() => {
                match crate::display::workbook_date_to_serial(self.eval.book, self.eval.now.date())
                {
                    Some(value) => finite(value),
                    None => Value::Error("#NUM!"),
                }
            }
            "NOW" if values.is_empty() => {
                use chrono::Timelike;
                match crate::display::workbook_date_to_serial(self.eval.book, self.eval.now.date())
                {
                    Some(day) => finite(
                        day + (self.eval.now.time().num_seconds_from_midnight() as f64
                            + self.eval.now.time().nanosecond() as f64 / 1_000_000_000.0)
                            / 86400.0,
                    ),
                    None => Value::Error("#NUM!"),
                }
            }
            "TIME" if values.len() == 3 => {
                if values
                    .iter()
                    .any(|n| !n.is_finite() || n.abs() > 1_000_000.0)
                {
                    return Value::Error("#NUM!");
                }
                let seconds =
                    values[0].trunc() * 3600.0 + values[1].trunc() * 60.0 + values[2].trunc();
                if seconds < 0.0 {
                    Value::Error("#NUM!")
                } else {
                    finite(seconds.rem_euclid(86400.0) / 86400.0)
                }
            }
            "CEILING" | "FLOOR" if values.len() == 2 => {
                let (number, significance) = (values[0], values[1]);
                if significance == 0.0 {
                    return finite(0.0);
                }
                if number > 0.0 && significance < 0.0 {
                    return Value::Error("#NUM!");
                }
                let magnitude = significance.abs();
                let round_up = (name == "CEILING") != (number < 0.0 && significance < 0.0);
                finite(if round_up {
                    (number / magnitude).ceil() * magnitude
                } else {
                    (number / magnitude).floor() * magnitude
                })
            }
            "PI" if values.is_empty() => finite(std::f64::consts::PI),
            "ABS" if one => finite(values[0].abs()),
            "SQRT" if one && values[0] >= 0.0 => finite(values[0].sqrt()),
            "SQRT" if one => Value::Error("#NUM!"),
            "SIN" if one => finite(values[0].sin()),
            "COS" if one => finite(values[0].cos()),
            "TAN" if one => finite(values[0].tan()),
            "EXP" if one => finite(values[0].exp()),
            "LOG10" if one && values[0] > 0.0 => finite(values[0].log10()),
            "LOG10" if one => Value::Error("#NUM!"),
            "RADIANS" if one => finite(values[0].to_radians()),
            "DEGREES" if one => finite(values[0].to_degrees()),
            "POWER" if values.len() == 2 => finite(values[0].powf(values[1])),
            "MOD" if values.len() == 2 && values[1] == 0.0 => Value::Error("#DIV/0!"),
            "MOD" if values.len() == 2 => {
                finite(values[0] - values[1] * (values[0] / values[1]).floor())
            }
            "LOG" if values.len() == 1 && values[0] > 0.0 => finite(values[0].log10()),
            "LOG"
                if values.len() == 2 && values[0] > 0.0 && values[1] > 0.0 && values[1] != 1.0 =>
            {
                finite(values[0].log(values[1]))
            }
            "LOG" => Value::Error("#NUM!"),
            "ROUNDUP" | "ROUNDDOWN" if values.len() == 2 => {
                let p = values[1].trunc().clamp(-308.0, 308.0) as i32;
                let factor = 10f64.powi(p);
                let n = values[0] * factor;
                finite(
                    (if name == "ROUNDUP" {
                        n.abs().ceil() * n.signum()
                    } else {
                        n.trunc()
                    }) / factor,
                )
            }
            "FACT" if one && values[0] >= 0.0 && values[0].trunc() <= 170.0 => {
                finite((1..=values[0].trunc() as u32).fold(1.0, |a, b| a * b as f64))
            }
            "FACT" if one => Value::Error("#NUM!"),
            "LN" if one && values[0] > 0.0 => finite(values[0].ln()),
            "LN" if one => Value::Error("#NUM!"),
            "INT" if one => finite(values[0].floor()),
            "SIGN" if one => finite(if values[0] == 0.0 {
                0.0
            } else {
                values[0].signum()
            }),
            "ROUND" if values.len() == 2 => {
                let p = arg(1).unwrap().trunc().clamp(-308.0, 308.0) as i32;
                let factor = 10f64.powi(p);
                finite((values[0] * factor).round() / factor)
            }
            _ => Value::Error("#ERROR!"),
        }
    }
    fn read_table_range(&mut self) -> Option<(SheetId, usize, usize, usize, usize)> {
        self.space();
        let start = self.pos;
        while self
            .input
            .get(self.pos)
            .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
        {
            self.pos += 1;
        }
        if self.pos == start || self.input.get(self.pos) != Some(&b'[') {
            self.pos = start;
            return None;
        }
        let name = std::str::from_utf8(&self.input[start..self.pos]).ok()?;
        self.pos += 1;
        let column_start = self.pos;
        while self.input.get(self.pos).is_some_and(|c| *c != b']') {
            self.pos += 1;
        }
        if self.input.get(self.pos) != Some(&b']') {
            self.pos = start;
            return None;
        }
        let column = std::str::from_utf8(&self.input[column_start..self.pos]).ok()?;
        self.pos += 1;
        for sheet in &self.eval.book.history.current().sheets {
            if let Some(table) = sheet
                .tables
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name))
            {
                let r = table.range;
                if column.eq_ignore_ascii_case("#all") {
                    return Some((sheet.id, r.top, r.left, r.bottom, r.right));
                }
                if column.eq_ignore_ascii_case("#headers") {
                    return Some((sheet.id, r.top, r.left, r.top, r.right));
                }
                if column.eq_ignore_ascii_case("#totals") && table.total_row {
                    return Some((sheet.id, r.bottom, r.left, r.bottom, r.right));
                }
                for col in r.left..=r.right {
                    if sheet
                        .cells
                        .get(&(r.top, col))
                        .is_some_and(|c| c.text.raw().eq_ignore_ascii_case(column))
                    {
                        return Some((
                            sheet.id,
                            r.top + 1,
                            col,
                            r.bottom - usize::from(table.total_row),
                            col,
                        ));
                    }
                }
            }
        }
        self.pos = start;
        None
    }
    fn read_range(&mut self) -> Option<(SheetId, usize, usize, usize, usize)> {
        let start = self.pos;
        if let Some(result) = self.computed_range() {
            if let Ok(range) = result {
                return Some(range);
            }
            self.pos = start;
            return None;
        }
        self.pos = start;
        if let Some(range) = self.read_table_range() {
            return Some(range);
        }
        self.pos = start;
        let first = self.read_any_ref(self.sheet)?;
        let last = if self.take(b':') {
            self.read_any_ref(first.0)?
        } else {
            first
        };
        if first.0 != last.0 || first.1 > last.1 || first.2 > last.2 {
            return None;
        }
        let sheet = self
            .eval
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.id == first.0)?;
        if last.1 >= sheet.rows || last.2 >= sheet.cols {
            return None;
        }
        Some((first.0, first.1, first.2, last.1, last.2))
    }
    fn lookup(&mut self) -> Value {
        let needle = self.comparison();
        if matches!(needle, Value::Error(_)) {
            return needle;
        }
        if !self.take(b',') {
            return Value::Error("#ERROR!");
        }
        let Some(lookup) = self.read_range() else {
            return Value::Error("#REF!");
        };
        let result = if self.take(b',') {
            let Some(range) = self.read_range() else {
                return Value::Error("#REF!");
            };
            range
        } else {
            lookup
        };
        if !self.take(b')') {
            return Value::Error("#ERROR!");
        }
        let vertical = lookup.2 == lookup.4;
        if !vertical && lookup.1 != lookup.3 {
            return Value::Error("#VALUE!");
        }
        let length = if vertical {
            lookup.3 - lookup.1 + 1
        } else {
            lookup.4 - lookup.2 + 1
        };
        if vertical && (result.2 != result.4 || result.3 - result.1 + 1 != length)
            || !vertical && (result.1 != result.3 || result.4 - result.2 + 1 != length)
        {
            return Value::Error("#VALUE!");
        }
        let mut best: Option<(usize, Value)> = None;
        for index in 0..length {
            self.eval.steps += 1;
            if self.eval.steps > MAX_STEPS {
                return Value::Error("#LIMIT!");
            }
            let value = self.eval.cell(
                lookup.0,
                lookup.1 + if vertical { index } else { 0 },
                lookup.2 + if vertical { 0 } else { index },
                self.depth + 1,
            );
            let eligible = lookup_order(&value, &needle).is_some_and(|o| !o.is_gt());
            let better = best.as_ref().is_none_or(|(_, previous)| {
                lookup_order(&value, previous).is_some_and(|o| !o.is_lt())
            });
            if eligible && better {
                best = Some((index, value));
            }
        }
        if let Some((index, _)) = best {
            self.eval.cell(
                result.0,
                result.1 + if vertical { index } else { 0 },
                result.2 + if vertical { 0 } else { index },
                self.depth + 1,
            )
        } else {
            Value::Error("#N/A")
        }
    }

    fn xlookup(&mut self) -> Value {
        let needle = self.comparison();
        if matches!(needle, Value::Error(_)) {
            return needle;
        }
        if !self.take(b',') {
            return Value::Error("#ERROR!");
        }
        let Some(lookup) = self.read_range() else {
            return Value::Error("#REF!");
        };
        if !self.take(b',') {
            return Value::Error("#ERROR!");
        }
        let Some(result) = self.read_range() else {
            return Value::Error("#REF!");
        };
        let vertical = lookup.2 == lookup.4;
        if !vertical && lookup.1 != lookup.3 {
            return Value::Error("#VALUE!");
        }
        let length = if vertical {
            lookup.3 - lookup.1 + 1
        } else {
            lookup.4 - lookup.2 + 1
        };
        if vertical && result.3 - result.1 + 1 != length
            || !vertical && result.4 - result.2 + 1 != length
        {
            return Value::Error("#VALUE!");
        }
        let fallback = if self.take(b',') {
            let start = self.pos;
            let mut i = start;
            let mut nesting = 0usize;
            let mut quoted = false;
            let mut sheet_quoted = false;
            loop {
                let Some(&ch) = self.input.get(i) else {
                    return Value::Error("#ERROR!");
                };
                if ch == b'"' && !sheet_quoted {
                    if quoted && self.input.get(i + 1) == Some(&b'"') {
                        i += 2;
                        continue;
                    }
                    quoted = !quoted;
                } else if ch == b'\'' && !quoted {
                    if sheet_quoted && self.input.get(i + 1) == Some(&b'\'') {
                        i += 2;
                        continue;
                    }
                    sheet_quoted = !sheet_quoted;
                }
                if !quoted && !sheet_quoted {
                    match ch {
                        b'(' | b'{' => nesting += 1,
                        b')' if nesting == 0 => break,
                        b')' | b'}' if nesting > 0 => nesting -= 1,
                        b',' if nesting == 0 => break,
                        _ => {}
                    }
                }
                i += 1;
            }
            self.pos = i;
            Some(&self.input[start..i])
        } else {
            None
        };
        let mode = if fallback.is_some() && self.take(b',') {
            if self.input.get(self.pos) == Some(&b',') || self.input.get(self.pos) == Some(&b')') {
                0
            } else {
                match self.comparison() {
                    Value::Number(n) if [-1.0, 0.0, 1.0, 2.0].contains(&n) => n as i32,
                    Value::Error(e) => return Value::Error(e),
                    _ => return Value::Error("#VALUE!"),
                }
            }
        } else {
            0
        };
        let reverse = if self.take(b',') {
            match self.comparison() {
                Value::Number(-1.0) => true,
                Value::Number(1.0) => false,
                Value::Error(e) => return Value::Error(e),
                _ => return Value::Error("#VALUE!"),
            }
        } else {
            false
        };
        if !self.take(b')') {
            return Value::Error("#ERROR!");
        }
        let mut best: Option<(usize, Value)> = None;
        for offset in 0..length {
            let index = if reverse { length - offset - 1 } else { offset };
            self.eval.steps += 1;
            if self.eval.steps > MAX_STEPS {
                return Value::Error("#LIMIT!");
            }
            let value = self.eval.cell(
                lookup.0,
                lookup.1 + if vertical { index } else { 0 },
                lookup.2 + if vertical { 0 } else { index },
                self.depth + 1,
            );
            if mode == 2 {
                if let (Value::Text(pattern), Value::Text(text)) = (&needle, &value) {
                    self.eval.steps = self
                        .eval
                        .steps
                        .saturating_add(pattern.len().saturating_mul(text.len()));
                    if self.eval.steps > MAX_STEPS {
                        return Value::Error("#LIMIT!");
                    }
                }
            }
            let equal = lookup_equal(&needle, &value)
                || mode == 2
                    && matches!((&needle, &value), (Value::Text(pattern), Value::Text(text)) if wildcard(pattern, text));
            if equal {
                return self.lookup_result(result, index, vertical);
            }
            if mode == -1 || mode == 1 {
                let valid = lookup_order(&value, &needle).is_some_and(|o| {
                    if mode == -1 {
                        o.is_lt()
                    } else {
                        o.is_gt()
                    }
                });
                let better = best.as_ref().is_none_or(|(_, old)| {
                    lookup_order(&value, old).is_some_and(|o| {
                        if mode == -1 {
                            o.is_gt()
                        } else {
                            o.is_lt()
                        }
                    })
                });
                if valid && better {
                    best = Some((index, value));
                }
            }
        }
        if let Some((index, _)) = best {
            return self.lookup_result(result, index, vertical);
        }
        if let Some(input) = fallback.filter(|input| !input.iter().all(u8::is_ascii_whitespace)) {
            let mut p = Parser {
                input,
                pos: 0,
                eval: self.eval,
                depth: self.depth,
                sheet: self.sheet,
                origin: self.origin,
                locals: self.locals.clone(),
            };
            let value = p.comparison();
            p.space();
            if p.pos == input.len() {
                value
            } else {
                Value::Error("#ERROR!")
            }
        } else {
            Value::Error("#N/A")
        }
    }
    fn type_predicate(&mut self, name: &str) -> Value {
        let start = self.pos;
        if name == "ISBLANK" {
            if let Some((sheet, row, col)) = self.read_any_ref(self.sheet) {
                if self.take(b')') {
                    if self.eval.raw(sheet, row, col).is_none() {
                        return Value::Error("#REF!");
                    }
                    self.eval.ensure_spills(sheet, self.depth + 1);
                    return Value::Boolean(
                        self.eval.raw(sheet, row, col) == Some("")
                            && !self.eval.spills.contains_key(&(sheet, row, col)),
                    );
                }
            }
            self.pos = start;
        }
        let value = self.comparison();
        if !self.take(b')') {
            return Value::Error("#ERROR!");
        }
        Value::Boolean(match name {
            "ISBLANK" => false,
            "ISNUMBER" => matches!(value, Value::Number(_)),
            "ISTEXT" => matches!(value, Value::Text(_)),
            "ISERROR" => matches!(value, Value::Error(_)),
            "ISNA" => value == Value::Error("#N/A"),
            _ => false,
        })
    }
    fn conditional_aggregate(&mut self, name: &str) -> Value {
        let aggregate = if matches!(name, "SUMIFS" | "AVERAGEIFS" | "MAXIFS" | "MINIFS") {
            let Some(range) = self.read_range() else {
                return Value::Error("#REF!");
            };
            if !self.take(b',') {
                return Value::Error("#ERROR!");
            }
            Some(range)
        } else {
            None
        };
        let mut criteria = Vec::new();
        loop {
            let Some(range) = self.read_range() else {
                return Value::Error("#REF!");
            };
            if !self.take(b',') {
                return Value::Error("#ERROR!");
            }
            let criterion = self.comparison();
            if let Value::Error(_) = criterion {
                return criterion;
            }
            criteria.push((range, criterion));
            if criteria.len() > 127 {
                return Value::Error("#LIMIT!");
            }
            if name == "AVERAGEIF" {
                break;
            }
            if self.take(b')') {
                break;
            }
            if !self.take(b',') {
                return Value::Error("#ERROR!");
            }
        }
        let aggregate = if name == "AVERAGEIF" {
            let selected = if self.take(b',') {
                let Some(r) = self.read_range() else {
                    return Value::Error("#REF!");
                };
                r
            } else {
                criteria[0].0
            };
            if !self.take(b')') {
                return Value::Error("#ERROR!");
            }
            Some(selected)
        } else {
            aggregate
        };
        let base = criteria[0].0;
        let height = base.3 - base.1 + 1;
        let width = base.4 - base.2 + 1;
        if criteria
            .iter()
            .any(|(r, _)| r.3 - r.1 + 1 != height || r.4 - r.2 + 1 != width)
            || aggregate.is_some_and(|r| r.3 - r.1 + 1 != height || r.4 - r.2 + 1 != width)
        {
            return Value::Error("#VALUE!");
        }
        let mut sum = 0.0;
        let mut count = 0usize;
        let mut extremum: Option<f64> = None;
        for row in 0..height {
            for col in 0..width {
                let mut matched = true;
                for (range, criterion) in &criteria {
                    self.eval.steps = self.eval.steps.saturating_add(1);
                    if self.eval.steps > MAX_STEPS {
                        return Value::Error("#LIMIT!");
                    }
                    let r = range.1 + row;
                    let c = range.2 + col;
                    let raw = self.eval.raw(range.0, r, c).unwrap_or("").to_owned();
                    let value = self.eval.cell(range.0, r, c, self.depth + 1);
                    if let Value::Text(spec) = criterion {
                        if spec.contains(['*', '?', '~']) {
                            self.eval.steps = self
                                .eval
                                .steps
                                .saturating_add(spec.len().saturating_mul(raw.len()));
                            if self.eval.steps > MAX_STEPS {
                                return Value::Error("#LIMIT!");
                            }
                        }
                    }
                    if !criterion_matches(&value, &raw, criterion) {
                        matched = false;
                        break;
                    }
                }
                if !matched {
                    continue;
                }
                if let Some(target) = aggregate {
                    match self.eval.range_cell(
                        target.0,
                        target.1 + row,
                        target.2 + col,
                        self.depth + 1,
                    ) {
                        Some(Value::Number(n)) => {
                            sum += n;
                            extremum = Some(extremum.map_or(n, |old| {
                                if name == "MINIFS" {
                                    old.min(n)
                                } else {
                                    old.max(n)
                                }
                            }));
                            count += 1;
                        }
                        Some(Value::Error(e)) => return Value::Error(e),
                        _ => {}
                    }
                } else {
                    count += 1;
                }
            }
        }
        match name {
            "COUNTIFS" => finite(count as f64),
            "SUMIFS" => finite(sum),
            "MAXIFS" | "MINIFS" => finite(extremum.unwrap_or(0.0)),
            _ if count == 0 => Value::Error("#DIV/0!"),
            _ => finite(sum / count as f64),
        }
    }
    fn range_function(&mut self, name: &str) -> Value {
        if name == "COUNTIF" || name == "SUMIF" {
            let Some(range) = self.read_range() else {
                return Value::Error("#REF!");
            };
            if !self.take(b',') {
                return Value::Error("#ERROR!");
            }
            let criterion = self.comparison();
            if let Value::Error(_) = criterion {
                return criterion;
            }
            let sum_range = if name == "SUMIF" && self.take(b',') {
                let Some(r) = self.read_range() else {
                    return Value::Error("#REF!");
                };
                Some(r)
            } else {
                None
            };
            if !self.take(b')') {
                return Value::Error("#ERROR!");
            }
            let mut total = 0.0;
            for r in range.1..=range.3 {
                for c in range.2..=range.4 {
                    self.eval.steps += 1;
                    if self.eval.steps > MAX_STEPS {
                        return Value::Error("#LIMIT!");
                    }
                    let raw = self.eval.raw(range.0, r, c).unwrap_or("").to_owned();
                    let value = self.eval.cell(range.0, r, c, self.depth + 1);
                    if let Value::Text(spec) = &criterion {
                        if spec.contains(['*', '?', '~']) {
                            let len = match &value {
                                Value::Text(t) => t.len(),
                                _ => raw.len(),
                            };
                            self.eval.steps = self
                                .eval
                                .steps
                                .saturating_add(spec.len().saturating_mul(len));
                            if self.eval.steps > MAX_STEPS {
                                return Value::Error("#LIMIT!");
                            }
                        }
                    }
                    if criterion_matches(&value, &raw, &criterion) {
                        if name == "COUNTIF" {
                            total += 1.0
                        } else {
                            let target = sum_range.unwrap_or(range);
                            let rr = target.1 + r - range.1;
                            let cc = target.2 + c - range.2;
                            if self.eval.raw(target.0, rr, cc).is_none() {
                                return Value::Error("#REF!");
                            }
                            match self.eval.cell(target.0, rr, cc, self.depth + 1) {
                                Value::Number(v) => total += v,
                                Value::Error(e) => return Value::Error(e),
                                _ => {}
                            }
                        }
                    }
                }
            }
            return finite(total);
        }
        let needle = self.comparison();
        if let Value::Error(_) = needle {
            return needle;
        }
        if !self.take(b',') {
            return Value::Error("#ERROR!");
        }
        let Some(range) = self.read_range() else {
            return Value::Error("#REF!");
        };
        let arg = if self.take(b',') {
            self.comparison()
        } else if name == "MATCH" {
            Value::Number(1.0)
        } else {
            return Value::Error("#ERROR!");
        };
        let exact = if name == "VLOOKUP" && self.take(b',') {
            self.comparison()
        } else {
            Value::Boolean(true)
        };
        if !self.take(b')') {
            return Value::Error("#ERROR!");
        }
        if name == "MATCH" {
            let Value::Number(mode) = arg else {
                return Value::Error("#VALUE!");
            };
            let mode = if mode > 0.0 {
                1
            } else if mode < 0.0 {
                -1
            } else {
                0
            };
            if range.1 != range.3 && range.2 != range.4 {
                return Value::Error("#N/A");
            }
            let length = (range.3 - range.1).max(range.4 - range.2) + 1;
            let mut best: Option<(usize, Value)> = None;
            for i in 0..length {
                self.eval.steps += 1;
                if self.eval.steps > MAX_STEPS {
                    return Value::Error("#LIMIT!");
                }
                let r = range.1 + if range.1 == range.3 { 0 } else { i };
                let c = range.2 + if range.2 == range.4 { 0 } else { i };
                let value = self.eval.cell(range.0, r, c, self.depth + 1);
                let equal = lookup_equal(&needle, &value)
                    || mode == 0
                        && matches!((&needle,&value),(Value::Text(a),Value::Text(b)) if wildcard(a,b));
                if equal {
                    return Value::Number((i + 1) as f64);
                }
                if mode != 0 {
                    let candidate = lookup_order(&value, &needle).is_some_and(|o| {
                        if mode == 1 {
                            !o.is_gt()
                        } else {
                            !o.is_lt()
                        }
                    });
                    let improvement = best.as_ref().is_none_or(|(_, old)| {
                        lookup_order(&value, old).is_some_and(|o| {
                            if mode == 1 {
                                o.is_gt()
                            } else {
                                o.is_lt()
                            }
                        })
                    });
                    if candidate && improvement {
                        best = Some((i, value))
                    }
                }
            }
            if let Some((index, _)) = best {
                return Value::Number((index + 1) as f64);
            }
            Value::Error("#N/A")
        } else {
            let Value::Number(col) = arg else {
                return Value::Error("#VALUE!");
            };
            if col < 1.0 {
                return Value::Error("#VALUE!");
            }
            if col.trunc() > (range.4 - range.2 + 1) as f64 {
                return Value::Error("#REF!");
            }
            let approximate = match exact {
                Value::Number(n) => n != 0.0,
                Value::Boolean(v) => v,
                _ => return Value::Error("#VALUE!"),
            };
            let col = range.2 + col.trunc() as usize - 1;
            if col > range.4 {
                return Value::Error("#REF!");
            }
            let mut best: Option<(usize, Value)> = None;
            for r in range.1..=range.3 {
                self.eval.steps += 1;
                if self.eval.steps > MAX_STEPS {
                    return Value::Error("#LIMIT!");
                }
                let value = self.eval.cell(range.0, r, range.2, self.depth + 1);
                if lookup_equal(&needle, &value) {
                    return self.eval.cell(range.0, r, col, self.depth + 1);
                }
                if approximate
                    && lookup_order(&value, &needle).is_some_and(|o| !o.is_gt())
                    && best
                        .as_ref()
                        .is_none_or(|(_, b)| lookup_order(&value, b).is_some_and(|o| o.is_gt()))
                {
                    best = Some((r, value))
                }
            }
            if let Some((row, _)) = best {
                return self.eval.cell(range.0, row, col, self.depth + 1);
            }
            Value::Error("#N/A")
        }
    }
    fn extended_text_function(&mut self, name: &str) -> Value {
        let mut args = Vec::new();
        if self.take(b')') {
            return self.call_extended(name, &args);
        }
        loop {
            self.space();
            if matches!(self.input.get(self.pos), Some(b',' | b')')) {
                args.push(text::Arg::Missing);
            } else {
                let start = self.pos;
                let range = if statistics::handles(name)
                    || distributions::handles(name)
                    || finance::handles(name)
                    || calendar::handles(name)
                    || information::scalar_handles(name)
                    || matches!(name, "TEXTJOIN" | "CONCAT" | "CONCATENATE")
                {
                    self.read_range()
                } else {
                    None
                };
                self.space();
                if let Some((sheet, top, left, bottom, right)) =
                    range.filter(|_| matches!(self.input.get(self.pos), Some(b',' | b')')))
                {
                    let mut values = Vec::new();
                    let mut blanks = Vec::new();
                    for row in top..=bottom {
                        for col in left..=right {
                            self.eval.steps += 1;
                            if self.eval.steps > MAX_STEPS || values.len() >= 100_000 {
                                return Value::Error("#LIMIT!");
                            }
                            let Some(_) = self.eval.raw(sheet, row, col) else {
                                return Value::Error("#REF!");
                            };
                            let (value, blank) =
                                self.eval.range_value(sheet, row, col, self.depth + 1);
                            blanks.push(blank);
                            if !self.charge_argument_text(&value) {
                                return Value::Error("#LIMIT!");
                            }
                            values.push(value);
                        }
                    }
                    args.push(text::Arg::Range(values, blanks));
                } else {
                    self.pos = start;
                    // A directly referenced blank cell remains blank for text aggregation.
                    let reference_start = self.pos;
                    let reference = self.read_any_ref(self.sheet);
                    self.space();
                    let blank = reference
                        .filter(|_| matches!(self.input.get(self.pos), Some(b',' | b')')))
                        .is_some_and(|(sheet, row, col)| {
                            self.eval.ensure_spills(sheet, self.depth + 1);
                            self.eval.raw(sheet, row, col) == Some("")
                                && !self.eval.spills.contains_key(&(sheet, row, col))
                        });
                    if blank {
                        args.push(text::Arg::Scalar(Value::Text(String::new())));
                    } else {
                        self.pos = reference_start;
                        let value = self.comparison();
                        if !self.charge_argument_text(&value) {
                            return Value::Error("#LIMIT!");
                        }
                        if let Value::Array(m) = value {
                            if name == "TYPE" {
                                args.push(text::Arg::Scalar(Value::Array(m)));
                            } else {
                                args.push(text::Arg::Range(m.values.clone(), m.blanks.clone()));
                            }
                        } else {
                            args.push(text::Arg::Scalar(value));
                        }
                    }
                }
            }
            if self.take(b')') {
                break;
            }
            if !self.take(b',') || args.len() >= 1024 {
                return Value::Error("#ERROR!");
            }
        }
        self.call_extended(name, &args)
    }
    fn charge_argument_text(&mut self, value: &Value) -> bool {
        if let Value::Array(m) = value {
            return m.values.iter().all(|v| self.charge_argument_text(v));
        }
        if let Value::Text(text) = value {
            self.eval.argument_text_bytes =
                self.eval.argument_text_bytes.saturating_add(text.len());
        }
        self.eval.argument_text_bytes <= crate::MAX_TEXT * 2
    }
    fn call_extended(&mut self, name: &str, args: &[text::Arg]) -> Value {
        if information::scalar_handles(name) {
            information::call(name, args)
        } else if statistics::handles(name) {
            statistics::call(name, args)
        } else if calendar::handles(name) {
            calendar::call(name, args, self.eval.book, &mut self.eval.steps)
        } else if finance::handles(name) {
            finance::call(name, args, &mut self.eval.steps)
        } else if distributions::handles(name) {
            distributions::call(name, args, &mut self.eval.steps)
        } else {
            text::call(name, args)
        }
    }
    fn text_function(&mut self, name: &str) -> Value {
        let mut args = Vec::new();
        if !self.take(b')') {
            loop {
                let v = self.comparison();
                if !self.charge_argument_text(&v) {
                    return Value::Error("#LIMIT!");
                }
                args.push(v);
                if self.take(b')') {
                    break;
                }
                if !self.take(b',') || args.len() > 1024 {
                    return Value::Error("#ERROR!");
                }
            }
        }
        if args.iter().any(|v| matches!(v, Value::Array(_))) {
            return self.lift_scalar(name, &args);
        }
        if let Some(error) = args.iter().find(|v| matches!(v, Value::Error(_))) {
            return error.clone();
        }
        if engineering::handles(name) {
            return engineering::call(name, &args);
        }
        let string = |n: usize| args.get(n).cloned().map(Value::display);
        let number = |n: usize, default: usize| -> Option<usize> {
            match args.get(n) {
                None => Some(default),
                Some(Value::Number(v)) if *v >= 0.0 && v.is_finite() => {
                    Some(v.trunc().min(1_000_000.0) as usize)
                }
                _ => None,
            }
        };
        let result = match name {
            "VALUE" if args.len() == 1 => {
                let text = string(0).unwrap();
                let trimmed = text.trim();
                if let Some(value) = trimmed
                    .strip_suffix('%')
                    .and_then(|s| s.trim().parse::<f64>().ok())
                {
                    finite(value / 100.0)
                } else if let Ok(value) = trimmed.parse::<f64>() {
                    finite(value)
                } else if let Some(value) =
                    crate::display::workbook_parse_serial(self.eval.book, trimmed)
                {
                    finite(value)
                } else {
                    Value::Error("#VALUE!")
                }
            }
            "DATEVALUE" if args.len() == 1 => {
                let text = string(0).unwrap();
                if !self.eval.book.history.current().date_1904 && text.trim() == "1900-02-29" {
                    return Value::Number(60.0);
                }
                match chrono::NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d")
                    .ok()
                    .and_then(|date| crate::display::workbook_date_to_serial(self.eval.book, date))
                {
                    Some(n) => finite(n),
                    None => Value::Error("#VALUE!"),
                }
            }
            "TEXT" if args.len() == 2 => {
                let value = match args[0] {
                    Value::Number(n) => n,
                    _ => return Value::Error("#VALUE!"),
                };
                let code = string(1).unwrap();
                match format_formula_number(self.eval.book, value, &code) {
                    Some(s) => Value::Text(s),
                    None => Value::Error("#VALUE!"),
                }
            }
            "LEN" if args.len() == 1 => Value::Number(string(0).unwrap().chars().count() as f64),
            "LOWER" if args.len() == 1 => Value::Text(string(0).unwrap().to_lowercase()),
            "UPPER" if args.len() == 1 => Value::Text(string(0).unwrap().to_uppercase()),
            "TRIM" if args.len() == 1 => Value::Text(
                string(0)
                    .unwrap()
                    .split(' ')
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            ),
            "EXACT" if args.len() == 2 => Value::Boolean(string(0) == string(1)),
            "LEFT" | "RIGHT" if (1..=2).contains(&args.len()) => {
                let Some(count) = number(1, 1) else {
                    return Value::Error("#VALUE!");
                };
                let chars: Vec<_> = string(0).unwrap().chars().collect();
                let start = if name == "RIGHT" {
                    chars.len().saturating_sub(count)
                } else {
                    0
                };
                Value::Text(chars.into_iter().skip(start).take(count).collect())
            }
            "MID" if args.len() == 3 => {
                let Some(start) = number(1, 0).and_then(|v| v.checked_sub(1)) else {
                    return Value::Error("#VALUE!");
                };
                let Some(count) = number(2, 0) else {
                    return Value::Error("#VALUE!");
                };
                Value::Text(string(0).unwrap().chars().skip(start).take(count).collect())
            }
            _ => Value::Error("#ERROR!"),
        };
        match result {
            Value::Text(s) if s.len() > crate::MAX_TEXT => Value::Error("#LIMIT!"),
            other => other,
        }
    }
    fn read_qualified_ref(&mut self) -> Option<(SheetId, usize, usize)> {
        self.space();
        let start = self.pos;
        let name = if self.input.get(self.pos) == Some(&b'\'') {
            self.pos += 1;
            let mut name = Vec::new();
            loop {
                let ch = *self.input.get(self.pos)?;
                self.pos += 1;
                if ch == b'\'' {
                    if self.input.get(self.pos) == Some(&b'\'') {
                        name.push(ch);
                        self.pos += 1
                    } else {
                        break;
                    }
                } else {
                    name.push(ch)
                }
            }
            String::from_utf8(name).ok()?
        } else {
            let begin = self.pos;
            while self
                .input
                .get(self.pos)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'.')
            {
                self.pos += 1
            }
            if begin == self.pos {
                self.pos = start;
                return None;
            }
            std::str::from_utf8(&self.input[begin..self.pos])
                .ok()?
                .to_string()
        };
        if self.input.get(self.pos) != Some(&b'!') {
            self.pos = start;
            return None;
        }
        self.pos += 1;
        let id = self
            .eval
            .book
            .history
            .current()
            .sheets
            .iter()
            .find(|s| s.name.eq_ignore_ascii_case(&name))
            .map(|s| s.id);
        let Some(id) = id else {
            self.pos = start;
            return None;
        };
        let Some((r, c)) = self.read_ref() else {
            self.pos = start;
            return None;
        };
        Some((id, r, c))
    }
    fn read_any_ref(&mut self, default: SheetId) -> Option<(SheetId, usize, usize)> {
        let start = self.pos;
        if let Some(v) = self.read_qualified_ref() {
            return Some(v);
        }
        self.pos = start;
        self.read_ref().map(|(r, c)| (default, r, c))
    }
    fn lazy_conditional(&mut self, name: &str) -> Value {
        let start = self.pos;
        let mut i = self.pos;
        let mut level = 0usize;
        let mut quoted = false;
        let mut sheet_quoted = false;
        let mut segments = Vec::new();
        let mut part = start;
        while i < self.input.len() {
            let ch = self.input[i];
            if ch == b'"' && !sheet_quoted {
                if quoted && self.input.get(i + 1) == Some(&b'"') {
                    i += 2;
                    continue;
                }
                quoted = !quoted
            } else if ch == b'\'' && !quoted {
                if sheet_quoted && self.input.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                sheet_quoted = !sheet_quoted;
            } else if !quoted && !sheet_quoted {
                match ch {
                    b'(' | b'{' => level += 1,
                    b')' if level == 0 => {
                        segments.push(&self.input[part..i]);
                        self.pos = i + 1;
                        break;
                    }
                    b')' | b'}' if level > 0 => level -= 1,
                    b',' if level == 0 => {
                        segments.push(&self.input[part..i]);
                        part = i + 1
                    }
                    _ => {}
                }
            }
            i += 1
        }
        if self.pos == start {
            return Value::Error("#ERROR!");
        }
        if (name == "IF" && !(1..=3).contains(&segments.len()))
            || (matches!(name, "IFERROR" | "IFNA") && segments.len() != 2)
            || (name == "IFS" && (segments.len() < 2 || segments.len() % 2 != 0))
            || (name == "SWITCH" && segments.len() < 3)
            || (name == "CHOOSE" && segments.len() < 2)
        {
            return Value::Error("#ERROR!");
        }
        let eval_arg = |this: &mut Self, idx: usize| {
            if segments[idx].iter().all(u8::is_ascii_whitespace) {
                return Value::Number(0.0);
            }
            let mut p = Parser {
                input: segments[idx],
                pos: 0,
                eval: this.eval,
                depth: this.depth,
                sheet: this.sheet,
                origin: this.origin,
                locals: this.locals.clone(),
            };
            let v = p.comparison();
            p.space();
            if p.pos != p.input.len() {
                Value::Error("#ERROR!")
            } else {
                v
            }
        };
        if name == "LET" {
            if segments.len() < 3 || segments.len() % 2 == 0 {
                return Value::Error("#VALUE!");
            }
            let previous = self.locals.clone();
            let result = (|| {
                for i in (0..segments.len() - 1).step_by(2) {
                    let name = std::str::from_utf8(segments[i])
                        .unwrap_or("")
                        .trim()
                        .to_uppercase();
                    let mut chars = name.chars();
                    if !chars.next().is_some_and(|c| c.is_alphabetic() || c == '_')
                        || !chars.all(|c| c.is_alphanumeric() || matches!(c, '_' | '.'))
                        || matches!(name.as_str(), "R" | "C")
                        || reference(name.as_bytes())
                            .is_some_and(|(r, c)| r < 1_048_576 && c < 16384)
                    {
                        return Value::Error("#VALUE!");
                    }
                    let value = eval_arg(self, i + 1);
                    if !self.charge_argument_text(&value) {
                        return Value::Error("#LIMIT!");
                    }
                    self.locals.insert(name, value);
                }
                eval_arg(self, segments.len() - 1)
            })();
            self.locals = previous;
            return result;
        }
        if name == "IFS" {
            for i in (0..segments.len()).step_by(2) {
                match information::truth(eval_arg(self, i)) {
                    Ok(true) => return eval_arg(self, i + 1),
                    Ok(false) => {}
                    Err(e) => return Value::Error(e),
                }
            }
            return Value::Error("#N/A");
        }
        if name == "SWITCH" {
            let value = eval_arg(self, 0);
            if let Value::Error(_) = value {
                return value;
            }
            let mut i = 1;
            while i + 1 < segments.len() {
                let candidate = eval_arg(self, i);
                if let Value::Error(_) = candidate {
                    return candidate;
                }
                if lookup_equal(&value, &candidate) {
                    return eval_arg(self, i + 1);
                }
                i += 2;
            }
            return if i < segments.len() {
                eval_arg(self, i)
            } else {
                Value::Error("#N/A")
            };
        }
        if name == "CHOOSE" {
            let value = eval_arg(self, 0);
            let idx = match statistics::number(Some(&text::Arg::Scalar(value)), None) {
                Ok(v) => v.trunc(),
                Err(e) => return Value::Error(e),
            };
            return if idx >= 1.0 && idx < (segments.len() as f64) {
                eval_arg(self, idx as usize)
            } else {
                Value::Error("#VALUE!")
            };
        }
        let first = eval_arg(self, 0);
        if matches!(name, "IFERROR" | "IFNA") {
            if let Value::Array(m) = &first {
                let needs_fallback = m
                    .values
                    .iter()
                    .any(|v| matches!(v,Value::Error(e)if name=="IFERROR"||*e=="#N/A"));
                if !needs_fallback {
                    return first;
                }
                let fallback = eval_arg(self, 1);
                return arrays::broadcast(&[first, fallback], &mut self.eval.steps, |v| {
                    if matches!(&v[0],Value::Error(e)if name=="IFERROR"||*e=="#N/A") {
                        v[1].clone()
                    } else {
                        v[0].clone()
                    }
                });
            }
            if matches!(first, Value::Error(_))
                && (name == "IFERROR" || first == Value::Error("#N/A"))
            {
                eval_arg(self, 1)
            } else {
                first
            }
        } else {
            if let Value::Array(m) = &first {
                let yes = m
                    .values
                    .iter()
                    .any(|v| information::truth(v.clone()) == Ok(true));
                let no = m
                    .values
                    .iter()
                    .any(|v| information::truth(v.clone()) == Ok(false));
                let then = if yes && segments.len() > 1 {
                    eval_arg(self, 1)
                } else {
                    Value::Boolean(true)
                };
                let other = if no && segments.len() > 2 {
                    eval_arg(self, 2)
                } else {
                    Value::Boolean(false)
                };
                return arrays::broadcast(&[first, then, other], &mut self.eval.steps, |v| {
                    match information::truth(v[0].clone()) {
                        Ok(true) => v[1].clone(),
                        Ok(false) => v[2].clone(),
                        Err(e) => Value::Error(e),
                    }
                });
            }
            match information::truth(first) {
                Ok(truth) => {
                    let index = if truth { 1 } else { 2 };
                    if index < segments.len() {
                        eval_arg(self, index)
                    } else {
                        Value::Boolean(truth)
                    }
                }
                Err(e) => Value::Error(e),
            }
        }
    }

    fn read_ref(&mut self) -> Option<(usize, usize)> {
        self.space();
        let start = self.pos;
        if self.input.get(self.pos) == Some(&b'$') {
            self.pos += 1;
        }
        while self
            .input
            .get(self.pos)
            .is_some_and(u8::is_ascii_alphabetic)
        {
            self.pos += 1;
        }
        if self.input.get(self.pos) == Some(&b'$') {
            self.pos += 1;
        }
        while self.input.get(self.pos).is_some_and(u8::is_ascii_digit) {
            self.pos += 1;
        }
        reference(&self.input[start..self.pos])
    }
}
fn lookup_order(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(b),
        (Value::Text(a), Value::Text(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(b)),
        _ => None,
    }
}
fn lookup_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => a == b,
        (Value::Text(a), Value::Text(b)) => a.to_lowercase()==b.to_lowercase(),
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        _ => false,
    }
}
fn wildcard(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let v: Vec<char> = value.to_lowercase().chars().collect();
    if p.len().saturating_mul(v.len()) > 100_000 {
        return false;
    }
    let mut previous = vec![false; v.len() + 1];
    previous[0] = true;
    let mut i = 0;
    while i < p.len() {
        let mut next = vec![false; v.len() + 1];
        let mut ch = p[i];
        let escaped = ch == '~' && i + 1 < p.len();
        if escaped {
            i += 1;
            ch = p[i]
        }
        if ch == '*' && !escaped {
            next[0] = previous[0];
            for j in 1..=v.len() {
                next[j] = previous[j] || next[j - 1]
            }
        } else {
            for j in 1..=v.len() {
                next[j] = previous[j - 1] && (ch == v[j - 1] || (ch == '?' && !escaped))
            }
        }
        previous = next;
        i += 1
    }
    previous[v.len()]
}
fn format_formula_number(book: &Workbook, value: f64, format: &str) -> Option<String> {
    let format = format.trim().to_ascii_lowercase();
    if format == "general" {
        return Some(crate::display::general(value));
    }
    if matches!(
        format.as_str(),
        "yyyy-mm-dd" | "m/d/yy" | "mm-dd-yy" | "d-mmm-yy"
    ) {
        if !book.history.current().date_1904 && value.floor() == 60.0 {
            return Some(
                match format.as_str() {
                    "yyyy-mm-dd" => "1900-02-29",
                    "m/d/yy" => "2/29/00",
                    "mm-dd-yy" => "02-29-00",
                    _ => "29-Feb-00",
                }
                .into(),
            );
        }
        use chrono::Datelike;
        let date = crate::display::workbook_serial_to_date(book, value)?;
        return Some(match format.as_str() {
            "yyyy-mm-dd" => date.format("%Y-%m-%d").to_string(),
            "m/d/yy" => format!("{}/{}/{:02}", date.month(), date.day(), date.year() % 100),
            "mm-dd-yy" => date.format("%m-%d-%y").to_string(),
            _ => date.format("%-d-%b-%y").to_string(),
        });
    }
    crate::display::number_pattern(value, format.as_str())
}
fn criterion_matches(value: &Value, raw: &str, criterion: &Value) -> bool {
    let Value::Text(spec) = criterion else {
        return lookup_equal(value, criterion);
    };
    let (op, body) = if let Some(v) = spec.strip_prefix(">=") {
        (0, v)
    } else if let Some(v) = spec.strip_prefix("<=") {
        (1, v)
    } else if let Some(v) = spec.strip_prefix("<>") {
        (2, v)
    } else if let Some(v) = spec.strip_prefix('>') {
        (3, v)
    } else if let Some(v) = spec.strip_prefix('<') {
        (4, v)
    } else if let Some(v) = spec.strip_prefix('=') {
        (5, v)
    } else {
        (5, spec.as_str())
    };
    if body.is_empty() && (op == 5 || op == 2) {
        return if op == 5 {
            raw.is_empty()
        } else {
            !raw.is_empty()
        };
    }
    let ordering = if let Ok(n) = body.parse::<f64>() {
        if let Value::Number(v) = value {
            v.partial_cmp(&n)
        } else {
            None
        }
    } else {
        Some(
            value
                .clone()
                .display()
                .to_lowercase()
                .cmp(&body.to_lowercase()),
        )
    };
    if (op == 5 || op == 2) && body.contains(['*', '?', '~']) {
        let matched = wildcard(body, &value.clone().display());
        return if op == 5 { matched } else { !matched };
    }
    let Some(ord) = ordering else { return false };
    match op {
        0 => !ord.is_lt(),
        1 => !ord.is_gt(),
        2 => !ord.is_eq(),
        3 => ord.is_gt(),
        4 => ord.is_lt(),
        _ => ord.is_eq(),
    }
}
fn reference(s: &[u8]) -> Option<(usize, usize)> {
    let clean: Vec<u8> = s.iter().copied().filter(|c| *c != b'$').collect();
    let s = clean.as_slice();
    let split = s.iter().position(u8::is_ascii_digit)?;
    if split == 0 {
        return None;
    }
    let mut col = 0usize;
    for c in &s[..split] {
        if !c.is_ascii_alphabetic() {
            return None;
        }
        col = col
            .checked_mul(26)?
            .checked_add((c.to_ascii_uppercase() - b'A' + 1) as usize)?;
    }
    let row = std::str::from_utf8(&s[split..])
        .ok()?
        .parse::<usize>()
        .ok()?;
    Some((row.checked_sub(1)?, col.checked_sub(1)?))
}
fn finite(n: f64) -> Value {
    if n.is_finite() {
        Value::Number(n)
    } else {
        Value::Error("#NUM!")
    }
}
fn calc(a: Value, b: Value, op: u8) -> Value {
    match (a, b) {
        (Value::Error(e), _) | (_, Value::Error(e)) => Value::Error(e),
        (Value::Number(a), Value::Number(b)) => match op {
            b'+' => finite(a + b),
            b'-' => finite(a - b),
            b'*' => finite(a * b),
            b'^' => finite(a.powf(b)),
            b'/' if b == 0.0 => Value::Error("#DIV/0!"),
            b'/' => finite(a / b),
            _ => Value::Error("#ERROR!"),
        },
        (Value::Boolean(a), Value::Number(b)) => calc(
            Value::Number(if a { 1.0 } else { 0.0 }),
            Value::Number(b),
            op,
        ),
        (Value::Number(a), Value::Boolean(b)) => calc(
            Value::Number(a),
            Value::Number(if b { 1.0 } else { 0.0 }),
            op,
        ),
        (Value::Boolean(a), Value::Boolean(b)) => calc(
            Value::Number(if a { 1.0 } else { 0.0 }),
            Value::Number(if b { 1.0 } else { 0.0 }),
            op,
        ),
        _ => Value::Error("#VALUE!"),
    }
}

pub(super) struct Calculation {
    pub values: std::collections::BTreeMap<(usize, usize), Value>,
    pub spills: std::collections::BTreeMap<(usize, usize), crate::Range>,
}
pub(super) fn numeric_cell_at(book:&Workbook,cell:(usize,usize),now:chrono::NaiveDateTime)->Result<f64,&'static str>{
    let mut eval=Eval::new(book,now);
    match eval.cell(book.active_sheet(),cell.0,cell.1,0){
        Value::Number(value)if value.is_finite()=>Ok(value),
        Value::Error(error)=>Err(error),_=>Err("The formula must return a finite number"),
    }
}
pub(super) fn optimization_values_at(book:&Workbook,cells:&[(usize,usize)],now:chrono::NaiveDateTime)->Option<Vec<f64>>{
    let mut eval=Eval::new(book,now);
    cells.iter().map(|&(row,col)|match eval.cell(book.active_sheet(),row,col,0){
        Value::Number(value)if value.is_finite()=>Some(value),Value::Boolean(value)=>Some(if value{1.}else{0.}),_=>None,
    }).collect()
}
pub(super) fn calculation_at(book: &Workbook, now: chrono::NaiveDateTime) -> Calculation {
    let sheet = book.active_sheet();
    let mut eval = Eval::new(book, now);
    eval.ensure_spills(sheet, 0);
    let mut values: std::collections::BTreeMap<_, _> = book
        .history
        .current()
        .sheet()
        .cells
        .iter()
        .filter(|(_, c)| matches!(c.text, CellContent::Formula(_)))
        .map(|(&(r, c), _)| ((r, c), eval.cell(sheet, r, c, 0)))
        .collect();
    values.extend(
        eval.spills
            .into_iter()
            .filter(|((s, _, _), _)| *s == sheet)
            .map(|((_, r, c), v)| ((r, c), v)),
    );
    let spills = eval
        .spill_ranges
        .into_iter()
        .filter(|((s, _, _), _)| *s == sheet)
        .map(|((_, r, c), range)| ((r, c), range))
        .collect();
    Calculation { values, spills }
}

#[cfg(test)]
pub(crate) fn display_grid_at(book: &Workbook, now: chrono::NaiveDateTime) -> Vec<Vec<String>> {
    let calculation = calculation_at(book, now);
    display_grid_with_calculation(book, &calculation)
}
#[cfg(test)]
fn display_grid_with_calculation(
    book: &Workbook,
    calculation: &Calculation,
) -> Vec<Vec<String>> {
    (0..book.rows())
        .map(|r| {
            (0..book.cols())
                .map(|c| {
                    if let Some(value) = calculation.values.get(&(r, c)) {
                        return value.clone().display();
                    }
                    match book.content(r, c) {
                        Some(CellContent::Boolean(v)) => {
                            if v.raw().eq_ignore_ascii_case("true") {
                                "TRUE".into()
                            } else {
                                "FALSE".into()
                            }
                        }
                        _ => book.cell(r, c).unwrap().to_owned(),
                    }
                })
                .collect()
        })
        .collect()
}

pub(crate) fn display_cell(book: &Workbook, row: usize, col: usize) -> String {
    let mut eval = Eval::new(book, chrono::Local::now().naive_local());
    eval.cell(book.active_sheet(), row, col, 0).display()
}

#[cfg(test)]
mod common_formula_regression {
    use super::*;
    #[test]
    fn lookup_modes_finance_and_excel_leap_day() {
        let mut book = Workbook::new(12, 4).unwrap();
        for (row, value) in [1, 3, 5].into_iter().enumerate() {
            book.set_cell(row, 0, value.to_string()).unwrap();
            book.set_cell(row, 1, (value * 10).to_string()).unwrap();
        }
        for (row, formula, expected) in [
            (3, "=XLOOKUP(4,A1:A3,B1:B3,, -1)", "30"),
            (4, "=XLOOKUP(4,A1:A3,B1:B3,, 1)", "50"),
            (5, "=LOOKUP(4,A1:A3,B1:B3)", "30"),
            (6, "=NPV(0,10,20,30)", "60"),
            (7, "=DATE(1900,2,29)", "60"),
            (8, "=DAY(60)", "29"),
            (9, "=CEILING(-2.3,-1)", "-3"),
        ] {
            book.set_cell(row, 0, formula.into()).unwrap();
            assert_eq!(book.display_grid()[row][0], expected, "{formula}");
        }
    }
    #[test]
    fn formulas_respect_the_1904_date_system() {
        let mut book = Workbook::new(3, 3).unwrap();
        let mut state = book.history.current().clone();
        state.date_1904 = true;
        book.history.apply(state);
        book.set_cell(0, 0, "=DATE(1904,1,1)".into()).unwrap();
        book.set_cell(1, 0, "=YEAR(0)".into()).unwrap();
        book.set_cell(2, 0, "=TEXT(0,\"yyyy-mm-dd\")".into())
            .unwrap();
        let grid = display_grid_at(
            &book,
            chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
        );
        assert_eq!(grid[0][0], "0");
        assert_eq!(grid[1][0], "1904");
        assert_eq!(grid[2][0], "1904-01-01");
    }
    #[test]
    fn recalc_uses_one_injected_local_time() {
        let mut book = Workbook::new(2, 3).unwrap();
        book.set_cell(0, 0, "=TODAY()".into()).unwrap();
        book.set_cell(0, 1, "=NOW()-TODAY()".into()).unwrap();
        book.set_cell(0, 2, "=NOW()-NOW()".into()).unwrap();
        let time = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap();
        let grid = display_grid_at(&book, time);
        assert_eq!(grid[0][0], "45292");
        assert_eq!(grid[0][1], "0.5");
        assert_eq!(grid[0][2], "0");
    }
    #[test]
    fn conditional_aggregation_predicates_and_financial_functions() {
        let mut book = Workbook::new(20, 8).unwrap();
        for (row, (kind, region, amount)) in [
            ("apple", "east", "10"),
            ("apple", "west", "20"),
            ("pear", "east", "30"),
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(row, 0, kind.into()).unwrap();
            book.set_cell(row, 1, region.into()).unwrap();
            book.set_cell(row, 2, amount.into()).unwrap();
        }
        for (row, formula, expected) in [
            (3, "=SUMIFS(C1:C3,A1:A3,\"apple\",B1:B3,\"east\")", "10"),
            (4, "=COUNTIFS(A1:A3,\"a*\",C1:C3,\">10\")", "1"),
            (5, "=AVERAGEIF(A1:A3,\"apple\",C1:C3)", "15"),
            (6, "=AVERAGEIFS(C1:C3,B1:B3,\"east\")", "20"),
            (7, "=ISBLANK(H1)", "TRUE"),
            (8, "=ISNUMBER(C1)", "TRUE"),
            (9, "=ISTEXT(A1)", "TRUE"),
            (10, "=ISERROR(1/0)", "TRUE"),
            (11, "=IFNA(1/0,5)", "#DIV/0!"),
            (12, "=PMT(0,10,100)", "-10"),
            (13, "=CEILING(2.3,1)", "3"),
            (14, "=XLOOKUP(\"pear\",A1:A3,C1:C3)", "30"),
            (15, "=XLOOKUP(\"missing\",A1:A3,C1:C3,77)", "77"),
            (16, "=TEXT(1.234,\"0.00\")", "1.23"),
            (17, "=VALUE(\"25%\")", "0.25"),
            (18, "=DATEVALUE(\"2024-01-01\")", "45292"),
        ] {
            book.set_cell(row, 0, formula.into()).unwrap();
            assert_eq!(book.display_grid()[row][0], expected, "{formula}");
        }
    }
}

/// Filter exclusions use one bounded evaluator and clock, rather than materializing a full display grid.
pub(super) fn filter_hidden_rows_at(
    book: &Workbook,
    now: chrono::NaiveDateTime,
    filter: &crate::SheetFilter,
) -> std::collections::BTreeSet<usize> {
    let mut eval = Eval::new(book, now);
    let needle = filter.contains.to_lowercase();
    (filter.range.top + 1..=filter.range.bottom)
        .filter(|r| {
            !eval
                .display_for_filter(book.active_sheet(), *r, filter.column, 0)
                .to_lowercase()
                .contains(&needle)
        })
        .collect()
}

fn compare_scalar(left: Value, right: Value, op: u8) -> Value {
    let ordering = match (left, right) {
        (Value::Error(e), _) | (_, Value::Error(e)) => return Value::Error(e),
        (Value::Number(a), Value::Number(b)) => a.partial_cmp(&b),
        (Value::Text(a), Value::Text(b)) => Some(a.to_lowercase().cmp(&b.to_lowercase())),
        (Value::Boolean(a), Value::Boolean(b)) => Some(a.cmp(&b)),
        _ => return Value::Error("#VALUE!"),
    };
    let Some(order) = ordering else {
        return Value::Error("#NUM!");
    };
    let yes = match op {
        0 => order != std::cmp::Ordering::Greater,
        1 => order != std::cmp::Ordering::Less,
        2 => order != std::cmp::Ordering::Equal,
        3 => order == std::cmp::Ordering::Equal,
        4 => order == std::cmp::Ordering::Less,
        _ => order == std::cmp::Ordering::Greater,
    };
    Value::Boolean(yes)
}

fn concat_scalar(a: Value, b: Value) -> Value {
    match (a, b) {
        (Value::Error(e), _) | (_, Value::Error(e)) => Value::Error(e),
        (a, b) => {
            let a = a.display();
            let b = b.display();
            if a.len().saturating_add(b.len()) > crate::MAX_TEXT {
                Value::Error("#LIMIT!")
            } else {
                Value::Text(a + &b)
            }
        }
    }
}

pub(super) fn is_dynamic_formula(raw: &str) -> bool {
    arrays::dynamic_formula(raw)
}
