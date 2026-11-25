use super::super::{Parser, MAX_DEPTH};
use super::*;
use crate::SheetId;
impl Parser<'_, '_> {
    pub(in crate::formula) fn array_literal(&mut self) -> Value {
        let result = (|| -> Result<Value> {
            let mut values = Vec::new();
            let mut rows = 1;
            let mut cols = None;
            let mut current = 0;
            loop {
                if values.len() >= MAX_CELLS {
                    return Err("#LIMIT!");
                }
                let value = self.comparison();
                if matches!(value, Value::Array(_)) {
                    return Err("#VALUE!");
                }
                if !self.charge_argument_text(&value) {
                    return Err("#LIMIT!");
                }
                values.push(value);
                current += 1;
                if self.take(b'}') {
                    if cols.is_some_and(|c| c != current) {
                        return Err("#VALUE!");
                    }
                    break;
                }
                if self.take(b';') {
                    if cols.is_some_and(|c| c != current) {
                        return Err("#VALUE!");
                    }
                    cols = Some(current);
                    current = 0;
                    rows += 1;
                } else if !self.take(b',') {
                    return Err("#ERROR!");
                }
            }
            charge(&mut self.eval.steps, values.len())?;
            Ok(Grid::new(rows, cols.unwrap_or(current), values)?.value())
        })();
        result.unwrap_or_else(Value::Error)
    }
    pub(in crate::formula) fn array_range(
        &mut self,
        range: (SheetId, usize, usize, usize, usize),
    ) -> Value {
        let result = (|| -> Result<Value> {
            let (sheet, top, left, bottom, right) = range;
            let (rows, cols) = (bottom - top + 1, right - left + 1);
            let count = size(rows, cols)?;
            charge(&mut self.eval.steps, count)?;
            let mut values = Vec::with_capacity(count);
            let mut blanks = Vec::with_capacity(count);
            for r in top..=bottom {
                for c in left..=right {
                    let (value, blank) = self.eval.range_value(sheet, r, c, self.depth + 1);
                    blanks.push(blank);
                    if !self.charge_argument_text(&value) {
                        return Err("#LIMIT!");
                    }
                    values.push(value);
                }
            }
            let mut m = Grid::new(rows, cols, values)?;
            m.blanks = blanks;
            m.origin = Some((top, left));
            Ok(m.value())
        })();
        result.unwrap_or_else(Value::Error)
    }
    pub(in crate::formula) fn array_function(&mut self, name: &str) -> Value {
        let mut values = Vec::new();
        if !self.take(b')') {
            loop {
                self.space();
                let value = if matches!(self.input.get(self.pos), Some(b',' | b')')) {
                    None
                } else {
                    Some(self.comparison())
                };
                if let Some(value) = &value {
                    if !self.charge_argument_text(value) {
                        return Value::Error("#LIMIT!");
                    }
                }
                values.push(value);
                if self.take(b')') {
                    break;
                }
                if !self.take(b',') || values.len() >= 1024 {
                    return Value::Error("#ERROR!");
                }
            }
        }
        call(name, &values, &mut self.eval.steps)
    }
    pub(in crate::formula) fn lift_scalar(&mut self, name: &str, values: &[Value]) -> Value {
        let grids: Vec<_> = values.iter().cloned().map(Grid::scalar).collect();
        let rows = grids.iter().map(|m| m.rows).max().unwrap_or(1);
        let cols = grids.iter().map(|m| m.cols).max().unwrap_or(1);
        let count = match size(rows, cols).and_then(|n| {
            charge(&mut self.eval.steps, n.saturating_mul(values.len().max(1))).map(|_| n)
        }) {
            Ok(n) => n,
            Err(e) => return Value::Error(e),
        };
        let mut out = Vec::with_capacity(count);
        for r in 0..rows {
            for c in 0..cols {
                let args: Vec<_> = grids.iter().map(|m| m.at(r, c)).collect();
                let value = if let Some(error) = args.iter().find(|v| matches!(v, Value::Error(_)))
                {
                    error.clone()
                } else {
                    self.scalar_function(name, &args)
                };
                if !self.charge_argument_text(&value) {
                    return Value::Error("#LIMIT!");
                }
                out.push(value);
            }
        }
        let mut result = Grid::new(rows, cols, out).unwrap();
        result.origin = grids
            .iter()
            .find(|m| m.rows == rows && m.cols == cols && m.origin.is_some())
            .and_then(|m| m.origin);
        result.value()
    }
    fn scalar_function(&mut self, name: &str, args: &[Value]) -> Value {
        if self.depth >= MAX_DEPTH {
            return Value::Error("#LIMIT!");
        }
        let literal = |v: &Value| match v {
            Value::Text(s) => format!("\"{}\"", s.replace('"', "\"\"")),
            Value::Array(_) => "#VALUE!".into(),
            v => v.clone().display(),
        };
        let source = format!(
            "{}({})",
            name,
            args.iter().map(literal).collect::<Vec<_>>().join(",")
        );
        if charge(&mut self.eval.steps, source.len().max(1)).is_err() {
            return Value::Error("#LIMIT!");
        }
        let mut parser = Parser {
            input: source.as_bytes(),
            pos: 0,
            eval: self.eval,
            depth: self.depth + 1,
            sheet: self.sheet,
            origin: self.origin,
            locals: self.locals.clone(),
        };
        let result = parser.comparison();
        parser.space();
        if parser.pos != parser.input.len() {
            Value::Error("#ERROR!")
        } else {
            result
        }
    }
    pub(in crate::formula) fn lookup_result(
        &mut self,
        range: (SheetId, usize, usize, usize, usize),
        index: usize,
        vertical: bool,
    ) -> Value {
        let (sheet, top, left, bottom, right) = range;
        if vertical && left == right || !vertical && top == bottom {
            return self.eval.cell(
                sheet,
                top + if vertical { index } else { 0 },
                left + if vertical { 0 } else { index },
                self.depth + 1,
            );
        }
        self.array_range(if vertical {
            (sheet, top + index, left, top + index, right)
        } else {
            (sheet, top, left + index, bottom, left + index)
        })
    }
    pub(in crate::formula) fn binary(&mut self, a: Value, b: Value, op: u8) -> Value {
        if matches!(a, Value::Array(_)) || matches!(b, Value::Array(_)) {
            broadcast(&[a, b], &mut self.eval.steps, |v| {
                super::super::calc(v[0].clone(), v[1].clone(), op)
            })
        } else {
            super::super::calc(a, b, op)
        }
    }
}
