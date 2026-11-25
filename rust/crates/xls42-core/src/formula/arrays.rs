//! Bounded rectangular operands; source cells remain authoritative and calculated arrays are disposable.
mod lookup;
mod matrix;
mod parse;
mod regression;
mod spill;
mod text_split;
mod text_regex;
mod transforms;
use super::{finite, information, statistics, text::Arg, Value, MAX_STEPS};
use std::sync::Arc;
type Result<T> = std::result::Result<T, &'static str>;
pub(super) const MAX_CELLS: usize = 100_000;
fn text_length(value: &Value) -> usize {
    if let Value::Text(s) = value {
        s.len()
    } else {
        0
    }
}
fn text_budget(total: &mut usize, bytes: usize) -> Result<()> {
    *total = total.checked_add(bytes).ok_or("#LIMIT!")?;
    if *total > crate::MAX_TEXT * 2 {
        Err("#LIMIT!")
    } else {
        Ok(())
    }
}
fn replication_text_budget(m: &Grid, pad: &Value, count: usize) -> Result<()> {
    let mut total = 0;
    for value in &m.values {
        text_budget(&mut total, text_length(value))?;
    }
    text_budget(
        &mut total,
        text_length(pad).checked_mul(count).ok_or("#LIMIT!")?,
    )
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Grid {
    pub rows: usize,
    pub cols: usize,
    pub values: Vec<Value>,
    pub blanks: Vec<bool>,
    pub origin: Option<(usize, usize)>,
}
impl Grid {
    pub fn new(rows: usize, cols: usize, values: Vec<Value>) -> Result<Self> {
        if rows == 0 || cols == 0 {
            return Err("#CALC!");
        }
        if rows.checked_mul(cols) != Some(values.len()) || values.len() > MAX_CELLS {
            return Err("#LIMIT!");
        }
        if values.iter().any(|v| matches!(v, Value::Array(_))) {
            return Err("#VALUE!");
        }
        let mut total = 0;
        for value in &values {
            text_budget(&mut total, text_length(value))?;
        }
        Ok(Self {
            rows,
            cols,
            blanks: vec![false; values.len()],
            values,
            origin: None,
        })
    }
    pub fn scalar(v: Value) -> Self {
        match v {
            Value::Array(m) => (*m).clone(),
            v => Self {
                rows: 1,
                cols: 1,
                values: vec![v],
                blanks: vec![false],
                origin: None,
            },
        }
    }
    pub fn at(&self, row: usize, col: usize) -> Value {
        let row = if self.rows == 1 { 0 } else { row };
        let col = if self.cols == 1 { 0 } else { col };
        if row >= self.rows || col >= self.cols {
            return Value::Error("#N/A");
        }
        let i = row * self.cols + col;
        if self.blanks[i] {
            Value::Number(0.0)
        } else {
            self.values[i].clone()
        }
    }
    pub fn implicit(&self, origin: (usize, usize)) -> Value {
        let mut row = 0;
        let mut col = 0;
        if let Some((top, left)) = self.origin {
            if self.cols == 1 && (top..top + self.rows).contains(&origin.0) {
                row = origin.0 - top;
            }
            if self.rows == 1 && (left..left + self.cols).contains(&origin.1) {
                col = origin.1 - left;
            }
        }
        self.at(row, col)
    }
    pub fn value(self) -> Value {
        Value::Array(Arc::new(self))
    }
    fn select(&self, rows: &[usize], cols: &[usize]) -> Result<Self> {
        size(rows.len(), cols.len())?;
        let mut total = 0;
        for r in rows {
            for c in cols {
                let i = r
                    .checked_mul(self.cols)
                    .and_then(|i| i.checked_add(*c))
                    .ok_or("#REF!")?;
                let value = self.values.get(i).ok_or("#REF!")?;
                text_budget(&mut total, text_length(value))?;
            }
        }

        let mut out = Self::new(
            rows.len(),
            cols.len(),
            rows.iter()
                .flat_map(|r| {
                    cols.iter()
                        .map(move |c| self.values[r * self.cols + c].clone())
                })
                .collect(),
        )?;
        out.blanks = rows
            .iter()
            .flat_map(|r| cols.iter().map(move |c| self.blanks[r * self.cols + c]))
            .collect();
        Ok(out)
    }
    fn transpose(&self) -> Result<Self> {
        let mut out = Self::new(
            self.cols,
            self.rows,
            (0..self.cols)
                .flat_map(|c| (0..self.rows).map(move |r| self.values[r * self.cols + c].clone()))
                .collect(),
        )?;
        out.blanks = (0..self.cols)
            .flat_map(|c| (0..self.rows).map(move |r| self.blanks[r * self.cols + c]))
            .collect();
        Ok(out)
    }
}
pub(super) fn charge(steps: &mut usize, n: usize) -> Result<()> {
    *steps = steps.saturating_add(n);
    if *steps > MAX_STEPS {
        Err("#LIMIT!")
    } else {
        Ok(())
    }
}
fn size(rows: usize, cols: usize) -> Result<usize> {
    if rows == 0 || cols == 0 {
        return Err("#CALC!");
    }
    rows.checked_mul(cols)
        .filter(|n| *n <= MAX_CELLS)
        .ok_or("#LIMIT!")
}
pub(super) fn broadcast(
    values: &[Value],
    steps: &mut usize,
    mut f: impl FnMut(Vec<Value>) -> Value,
) -> Value {
    let grids: Vec<_> = values.iter().cloned().map(Grid::scalar).collect();
    let rows = grids.iter().map(|m| m.rows).max().unwrap_or(1);
    let cols = grids.iter().map(|m| m.cols).max().unwrap_or(1);
    let count = match size(rows, cols)
        .and_then(|n| charge(steps, n.saturating_mul(values.len().max(1))).map(|_| n))
    {
        Ok(n) => n,
        Err(e) => return Value::Error(e),
    };
    let mut result = Vec::with_capacity(count);
    let mut text_bytes = 0;
    for r in 0..rows {
        for c in 0..cols {
            let value = f(grids.iter().map(|m| m.at(r, c)).collect());
            if text_budget(&mut text_bytes, text_length(&value)).is_err() {
                return Value::Error("#LIMIT!");
            }
            result.push(value);
        }
    }
    let mut result = Grid::new(rows, cols, result).unwrap();
    result.origin = grids
        .iter()
        .find(|m| m.rows == rows && m.cols == cols && m.origin.is_some())
        .and_then(|m| m.origin);
    result.value()
}
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "FILTER"
            | "SORT"
            | "SORTBY"
            | "UNIQUE"
            | "SEQUENCE"
            | "RANDARRAY"
            | "TRANSPOSE"
            | "VSTACK"
            | "HSTACK"
            | "TOCOL"
            | "TOROW"
            | "CHOOSECOLS"
            | "CHOOSEROWS"
            | "TAKE"
            | "DROP"
            | "EXPAND"
            | "WRAPROWS"
            | "WRAPCOLS"
            | "MMULT"
            | "MDETERM"
            | "MINVERSE"
            | "MUNIT"
            | "FREQUENCY"
            | "MODE.MULT"
            | "SUMPRODUCT"
            | "SUMX2MY2"
            | "SUMX2PY2"
            | "SUMXMY2"
            | "SERIESSUM"
            | "INDEX"
            | "HLOOKUP"
            | "XMATCH"
            | "LOOKUP"
            | "TEXTSPLIT"
            | "LINEST"
            | "LOGEST"
            | "TREND"
            | "GROWTH"
            | "REGEX"
            | "REGEXTEST"
            | "REGEXEXTRACT"
            | "REGEXREPLACE"
    )
}
pub(super) fn dynamic(name: &str) -> bool {
    matches!(
        name,
        "FILTER"
            | "SORT"
            | "SORTBY"
            | "UNIQUE"
            | "SEQUENCE"
            | "RANDARRAY"
            | "TRANSPOSE"
            | "TEXTSPLIT"
            | "MODE.MULT"
            | "LINEST"
            | "TREND"
            | "GROWTH"
            | "MMULT"
            | "MINVERSE"
            | "MUNIT"
            | "FREQUENCY"
            | "XLOOKUP"
            | "VSTACK"
            | "HSTACK"
            | "TOCOL"
            | "TOROW"
            | "CHOOSECOLS"
            | "CHOOSEROWS"
            | "TAKE"
            | "DROP"
            | "EXPAND"
            | "WRAPROWS"
            | "WRAPCOLS"
            | "LET"
            | "REGEXEXTRACT"
            | "LOGEST"
    )
}

struct Args<'a>(&'a [Option<Value>]);
impl Args<'_> {
    fn count(&self, lo: usize, hi: usize) -> Result<()> {
        if (lo..=hi).contains(&self.0.len()) {
            Ok(())
        } else {
            Err("#VALUE!")
        }
    }
    fn has(&self, i: usize) -> bool {
        self.0.get(i).is_some_and(Option::is_some)
    }
    fn value(&self, i: usize) -> Result<Value> {
        self.0.get(i).and_then(Clone::clone).ok_or("#VALUE!")
    }
    fn grid(&self, i: usize) -> Result<Grid> {
        Ok(Grid::scalar(self.value(i)?))
    }
    fn scalar(&self, i: usize) -> Result<Value> {
        match self.value(i)? {
            Value::Array(m) => Ok(m.at(0, 0)),
            v => Ok(v),
        }
    }
    fn number(&self, i: usize, default: Option<f64>) -> Result<f64> {
        if !self.has(i) {
            return default.ok_or("#VALUE!");
        }
        statistics::number(Some(&Arg::Scalar(self.scalar(i)?)), None)
    }
    fn integer(&self, i: usize, default: Option<i64>) -> Result<i64> {
        let n = self.number(i, default.map(|n| n as f64))?.trunc();
        if n.abs() >= 9e15 {
            Err("#NUM!")
        } else {
            Ok(n as i64)
        }
    }
    fn boolean(&self, i: usize, default: bool) -> Result<bool> {
        if !self.has(i) {
            Ok(default)
        } else {
            information::truth(self.scalar(i)?)
        }
    }
}
pub(super) fn call(name: &str, args: &[Option<Value>], steps: &mut usize) -> Value {
    calculate(name, Args(args), steps).unwrap_or_else(Value::Error)
}
fn calculate(name: &str, x: Args<'_>, steps: &mut usize) -> Result<Value> {
    if matches!(
        name,
        "REGEX" | "REGEXTEST" | "REGEXEXTRACT" | "REGEXREPLACE"
    ) {
        return text_regex::calculate(name, &x, steps);
    }
    if name == "TEXTSPLIT" {
        return text_split::calculate(&x, steps);
    }
    if matches!(name, "LINEST" | "LOGEST" | "TREND" | "GROWTH") {
        return regression::calculate(name, &x, steps);
    }
    if matches!(name, "SEQUENCE" | "RANDARRAY" | "MUNIT") {
        x.count(
            if matches!(name, "MUNIT" | "SEQUENCE") {
                1
            } else {
                0
            },
            if name == "MUNIT" {
                1
            } else if name == "SEQUENCE" {
                4
            } else {
                5
            },
        )?;
        let rows = x.integer(0, Some(1))?;
        let cols = if name == "MUNIT" {
            rows
        } else {
            x.integer(1, Some(1))?
        };
        if rows < 1 || cols < 1 {
            return Err("#CALC!");
        }
        let count = size(rows as usize, cols as usize)?;
        charge(steps, count)?;
        let (a, b) = (
            x.number(2, Some(if name == "RANDARRAY" { 0.0 } else { 1.0 }))?,
            x.number(3, Some(1.0))?,
        );
        let integers = name == "RANDARRAY" && x.boolean(4, false)?;
        if name == "RANDARRAY" && (a > b || !(b - a).is_finite()) {
            return Err("#VALUE!");
        }
        if integers && (a.ceil() > b.floor() || a.abs() >= 9e15 || b.abs() >= 9e15) {
            return Err("#NUM!");
        }
        return Ok(Grid::new(
            rows as usize,
            cols as usize,
            (0..count)
                .map(|i| {
                    finite(if name == "MUNIT" {
                        f64::from(i / (cols as usize) == i % (cols as usize))
                    } else if name == "SEQUENCE" {
                        a + i as f64 * b
                    } else if integers {
                        fastrand::i64(a.ceil() as i64..=b.floor() as i64) as f64
                    } else {
                        a + (b - a) * fastrand::f64()
                    })
                })
                .collect(),
        )?
        .value());
    }
    if name == "TRANSPOSE" {
        x.count(1, 1)?;
        return Ok(x.grid(0)?.transpose()?.value());
    }
    if matches!(name, "SUMPRODUCT" | "SUMX2MY2" | "SUMX2PY2" | "SUMXMY2") {
        x.count(
            if name == "SUMPRODUCT" { 1 } else { 2 },
            if name == "SUMPRODUCT" { 255 } else { 2 },
        )?;
        let matrices: Vec<_> = (0..x.0.len()).map(|i| x.grid(i)).collect::<Result<_>>()?;
        let first = &matrices[0];
        if matrices
            .iter()
            .any(|m| m.rows != first.rows || m.cols != first.cols)
        {
            return Err("#VALUE!");
        }
        charge(steps, first.values.len() * matrices.len())?;
        let mut total = 0.0;
        for i in 0..first.values.len() {
            let mut nums = Vec::new();
            let mut good = true;
            for m in &matrices {
                match &m.values[i] {
                    Value::Error(e) => return Err(e),
                    Value::Number(n) if !m.blanks[i] => nums.push(*n),
                    _ => {
                        good = false;
                        nums.push(0.0);
                    }
                }
            }
            total += if name == "SUMPRODUCT" {
                nums.iter().product::<f64>()
            } else if !good {
                0.0
            } else {
                match name {
                    "SUMX2MY2" => nums[0] * nums[0] - nums[1] * nums[1],
                    "SUMX2PY2" => nums[0] * nums[0] + nums[1] * nums[1],
                    _ => (nums[0] - nums[1]).powi(2),
                }
            };
        }
        return Ok(finite(total));
    }
    if name == "SERIESSUM" {
        x.count(4, 4)?;
        let (base, start, step) = (x.number(0, None)?, x.number(1, None)?, x.number(2, None)?);
        let m = x.grid(3)?;
        charge(steps, m.values.len())?;
        let mut sum = 0.0;
        let mut index = 0;
        for v in &m.values {
            match v {
                Value::Error(e) => return Err(e),
                Value::Number(n) => {
                    sum += n * base.powf(start + index as f64 * step);
                    index += 1;
                }
                _ => {}
            }
        }
        return Ok(finite(sum));
    }
    if let Some(result) = lookup::call(name, &x, steps) {
        return result;
    }
    if let Some(result) = matrix::call(name, &x, steps) {
        return result;
    }
    let m = x.grid(0)?;
    charge(steps, m.values.len())?;
    if let Some(result) = transforms::call(name, &x, &m, steps) {
        return result;
    }
    match name {
        "INDEX" => {
            x.count(2, 3)?;
            let mut r = x.integer(1, Some(0))?;
            let mut c = x.integer(2, Some(0))?;
            if !x.has(2) && m.rows == 1 && m.cols > 1 {
                c = r;
                r = 1;
            }
            if c == 0 && m.cols == 1 {
                c = 1;
            }
            if r == 0 && m.rows == 1 {
                r = 1;
            }
            if r < 0 || c < 0 || r > m.rows as i64 || c > m.cols as i64 {
                return Err("#REF!");
            }
            let rows = if r == 0 {
                (0..m.rows).collect::<Vec<_>>()
            } else {
                vec![r as usize - 1]
            };
            let cols = if c == 0 {
                (0..m.cols).collect::<Vec<_>>()
            } else {
                vec![c as usize - 1]
            };
            let mut result = m.select(&rows, &cols)?;
            result.origin = m.origin.map(|(r, c)| (r + rows[0], c + cols[0]));
            Ok(result.value())
        }
        "FILTER" => {
            x.count(2, 3)?;
            let include = x.grid(1)?;
            let columns = if include.cols == 1 && include.rows == m.rows {
                false
            } else if include.rows == 1 && include.cols == m.cols {
                true
            } else {
                return Err("#VALUE!");
            };
            let mut kept = Vec::new();
            for (i, v) in include.values.into_iter().enumerate() {
                if include.blanks[i] {
                    continue;
                }
                if matches!(v, Value::Text(_)) {
                    return Err("#VALUE!");
                }
                if information::truth(v)? {
                    kept.push(i);
                }
            }
            if kept.is_empty() {
                return if x.has(2) { x.value(2) } else { Err("#CALC!") };
            }
            Ok(if columns {
                m.select(&(0..m.rows).collect::<Vec<_>>(), &kept)?
            } else {
                m.select(&kept, &(0..m.cols).collect::<Vec<_>>())?
            }
            .value())
        }
        "TAKE" | "DROP" => {
            x.count(2, 3)?;
            let cut = |n: usize, k: i64| -> Vec<usize> {
                let k = k.unsigned_abs().min(n as u64) as usize;
                let (a, b) = if name == "TAKE" {
                    if k == 0 {
                        (0, 0)
                    } else {
                        (0, k)
                    }
                } else {
                    (k, n)
                };
                (a..b).collect()
            };
            let choose = |n: usize, k: i64| -> Vec<usize> {
                let mut result = cut(n, k);
                if k < 0 {
                    result = result.into_iter().map(|v| n - 1 - v).collect();
                    result.reverse();
                }
                result
            };
            let rows = if x.has(1) {
                choose(m.rows, x.integer(1, None)?)
            } else {
                (0..m.rows).collect()
            };
            let cols = if x.has(2) {
                choose(m.cols, x.integer(2, None)?)
            } else {
                (0..m.cols).collect()
            };
            Ok(m.select(&rows, &cols)?.value())
        }
        "CHOOSECOLS" | "CHOOSEROWS" => {
            x.count(2, 255)?;
            let by_col = name == "CHOOSECOLS";
            let n = if by_col { m.cols } else { m.rows };
            let mut selected = Vec::new();
            for i in 1..x.0.len() {
                for v in x.grid(i)?.values {
                    let k = statistics::number(Some(&Arg::Scalar(v)), None)?.trunc();
                    if k == 0.0 || k.abs() > n as f64 {
                        return Err("#VALUE!");
                    }
                    selected.push(if k > 0.0 {
                        k as usize - 1
                    } else {
                        (n as i64 + k as i64) as usize
                    });
                    if selected.len() > MAX_CELLS {
                        return Err("#LIMIT!");
                    }
                }
            }
            Ok(if by_col {
                m.select(&(0..m.rows).collect::<Vec<_>>(), &selected)?
            } else {
                m.select(&selected, &(0..m.cols).collect::<Vec<_>>())?
            }
            .value())
        }
        "EXPAND" => {
            x.count(2, 4)?;
            let rows = x.integer(1, Some(m.rows as i64))?;
            let cols = x.integer(2, Some(m.cols as i64))?;
            if rows < m.rows as i64 || cols < m.cols as i64 {
                return Err("#VALUE!");
            }
            let count = size(rows as usize, cols as usize)?;
            charge(steps, count)?;
            let pad = if x.has(3) {
                x.scalar(3)?
            } else {
                Value::Error("#N/A")
            };
            replication_text_budget(&m, &pad, count - m.values.len())?;
            Ok(Grid::new(
                rows as usize,
                cols as usize,
                (0..rows as usize)
                    .flat_map(|r| {
                        let pad = &pad;
                        let m = &m;
                        (0..cols as usize).map(move |c| {
                            if r < m.rows && c < m.cols {
                                m.at(r, c)
                            } else {
                                pad.clone()
                            }
                        })
                    })
                    .collect(),
            )?
            .value())
        }
        "TOCOL" | "TOROW" => {
            x.count(1, 3)?;
            let ignore = x.integer(1, Some(0))?;
            if !(0..=3).contains(&ignore) {
                return Err("#VALUE!");
            }
            let m = if x.boolean(2, false)? {
                m.transpose()?
            } else {
                m
            };
            let mut values = Vec::new();
            for (i, v) in m.values.into_iter().enumerate() {
                if matches!(ignore, 1 | 3) && m.blanks[i]
                    || matches!(ignore, 2 | 3) && matches!(v, Value::Error(_))
                {
                    continue;
                }
                values.push(if m.blanks[i] { Value::Number(0.0) } else { v });
            }
            Ok(Grid::new(
                if name == "TOCOL" { values.len() } else { 1 },
                if name == "TOROW" { values.len() } else { 1 },
                values,
            )?
            .value())
        }
        "WRAPROWS" | "WRAPCOLS" => {
            x.count(2, 3)?;
            if m.rows != 1 && m.cols != 1 {
                return Err("#VALUE!");
            }
            let k = x.integer(1, None)?;
            if k < 1 || k > MAX_CELLS as i64 {
                return Err("#NUM!");
            }
            let k = k as usize;
            let rows = m.values.len().div_ceil(k);
            let count = size(rows, k)?;
            charge(steps, count)?;
            let pad = if x.has(2) {
                x.scalar(2)?
            } else {
                Value::Error("#N/A")
            };
            replication_text_budget(&m, &pad, count - m.values.len())?;
            let result = Grid::new(
                rows,
                k,
                (0..count)
                    .map(|i| m.values.get(i).cloned().unwrap_or_else(|| pad.clone()))
                    .collect(),
            )?;
            Ok(if name == "WRAPCOLS" {
                result.transpose()?
            } else {
                result
            }
            .value())
        }
        "VSTACK" | "HSTACK" => {
            x.count(1, 255)?;
            let mut parts = Vec::new();
            for i in 0..x.0.len() {
                if x.has(i) {
                    let m = x.grid(i)?;
                    parts.push(if name == "HSTACK" { m.transpose()? } else { m });
                }
            }
            let cols = parts.iter().map(|m| m.cols).max().ok_or("#VALUE!")?;
            let rows = parts
                .iter()
                .try_fold(0usize, |n, m| n.checked_add(m.rows))
                .ok_or("#LIMIT!")?;
            let count = size(rows, cols)?;
            charge(steps, count)?;
            let mut values = Vec::with_capacity(count);
            for part in parts {
                for r in 0..part.rows {
                    for c in 0..cols {
                        values.push(if c < part.cols {
                            part.at(r, c)
                        } else {
                            Value::Error("#N/A")
                        });
                    }
                }
            }
            let result = Grid::new(rows, cols, values)?;
            Ok(if name == "HSTACK" {
                result.transpose()?
            } else {
                result
            }
            .value())
        }
        _ => Err("#NAME?"),
    }
}

pub(super) fn dynamic_formula(raw: &str) -> bool {
    spill::dynamic_formula(raw)
}
