//! Bounded, readonly constrained optimization over the ordinary workbook evaluator.
use super::*;
mod engine;
pub(crate) mod files;
mod linear;
mod nonlinear;
mod task;
#[cfg(test)]
mod tests;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
pub use task::{SolverResult, SolverStatus, SolverTask};
struct Budget<'a> {
    cancel: &'a AtomicBool,
    began: Instant,
    steps: usize,
}
impl<'a> Budget<'a> {
    fn new(cancel: &'a AtomicBool) -> Self {
        Self {
            cancel,
            began: Instant::now(),
            steps: 0,
        }
    }
    fn tick(&mut self) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("Solver canceled".into());
        }
        self.steps += 1;
        if self.steps > 100_000 || self.began.elapsed() > Duration::from_secs(15) {
            return Err("Solver reached its calculation limit".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SolverGoal {
    Maximum,
    Minimum,
    Value(f64),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SolverOp {
    Le,
    Ge,
    Eq,
    Integer,
    Binary,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolverConstraint {
    pub lhs: Range,
    pub op: SolverOp,
    pub rhs: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolverModel {
    pub objective: (usize, usize),
    pub goal: SolverGoal,
    pub variables: Vec<(usize, usize)>,
    pub constraints: Vec<SolverConstraint>,
    pub non_negative: bool,
    pub linear: bool,
    #[serde(default)]
    pub extra: BTreeMap<String, String>,
}
fn address(value: &str, rows: usize, cols: usize) -> Option<(usize, usize)> {
    let value = value.trim().replace('$', "");
    if value.len() > 20 {
        return None;
    }
    let split = value.find(|c: char| !c.is_ascii_alphabetic())?;
    let (column, row) = value.split_at(split);
    if column.is_empty() || row.is_empty() || !row.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let column = column
        .bytes()
        .try_fold(0usize, |n, c| {
            n.checked_mul(26)?
                .checked_add((c.to_ascii_uppercase() - b'A' + 1) as usize)
        })?
        .checked_sub(1)?;
    let row = row.parse::<usize>().ok()?.checked_sub(1)?;
    (row < rows && column < cols).then_some((row, column))
}
pub(super) fn range(value: &str, rows: usize, cols: usize) -> Option<Range> {
    let mut parts = value.trim().split(':');
    let a = address(parts.next()?, rows, cols)?;
    let b = parts
        .next()
        .map(|part| address(part, rows, cols))
        .unwrap_or(Some(a))?;
    if parts.next().is_some() || a.0 > b.0 || a.1 > b.1 {
        return None;
    }
    Some(Range {
        top: a.0,
        left: a.1,
        bottom: b.0,
        right: b.1,
    })
}
fn count(range: Range) -> usize {
    (range.bottom - range.top + 1) * (range.right - range.left + 1)
}
pub(super) fn valid_model(model: &SolverModel, rows: usize, cols: usize) -> bool {
    if model.objective.0 >= rows
        || model.objective.1 >= cols
        || model.variables.is_empty()
        || model.variables.len() > 32
        || model.constraints.len() > 128
        || matches!(model.goal,SolverGoal::Value(value)if !value.is_finite())
        || model.variables.iter().any(|&(r, c)| r >= rows || c >= cols)
        || model
            .variables
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            != model.variables.len()
        || model.constraints.iter().map(|c| c.rhs.len()).sum::<usize>() > 16384
        || model.extra.len() > 128
        || model.extra.iter().any(|(key, value)| {
            !key.starts_with("solver_") || key.len() > 128 || value.len() > 4096
        })
        || model
            .extra
            .iter()
            .map(|(key, value)| key.len() + value.len())
            .sum::<usize>()
            > 16384
    {
        return false;
    }
    let mut scalar_rows = 0usize;
    for constraint in &model.constraints {
        let lhs = constraint.lhs;
        if lhs.top > lhs.bottom
            || lhs.left > lhs.right
            || lhs.bottom >= rows
            || lhs.right >= cols
            || constraint.rhs.len() > 4096
        {
            return false;
        }
        if matches!(constraint.op, SolverOp::Integer | SolverOp::Binary) {
            if count(lhs) > 32 {
                return false;
            }
            continue;
        }
        scalar_rows += count(lhs);
        if scalar_rows > 256 {
            return false;
        }
        if !constraint
            .rhs
            .trim()
            .replace(',', ".")
            .parse::<f64>()
            .is_ok_and(f64::is_finite)
        {
            let Some(rhs) = range(&constraint.rhs, rows, cols) else {
                return false;
            };
            if count(rhs) != 1 && count(rhs) != count(lhs) {
                return false;
            }
        }
    }
    true
}
impl Workbook {
    pub fn solver_model(&self) -> Option<&SolverModel> {
        self.history.current().sheet().solver.as_ref()
    }
    pub fn set_solver_model(&mut self, model: Option<SolverModel>) -> Result<bool, Error> {
        self.require_editable()?;
        if self.is_protected() {
            return Err(Error::Protected);
        }
        if model
            .as_ref()
            .is_some_and(|model| !valid_model(model, self.rows(), self.cols()))
        {
            return Err(Error::Invalid("Invalid Solver cells or constraints"));
        }
        let mut state = self.history.current().clone();
        state.sheet_mut().solver = model;
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        Ok(self.history.apply(state))
    }
}

pub(super) fn model_bytes(m: &SolverModel) -> usize {
    m.variables.len() * 16
        + m.constraints
            .iter()
            .map(|c| 32 + c.rhs.len())
            .sum::<usize>()
        + m.extra
            .iter()
            .map(|(k, v)| k.len() + v.len())
            .sum::<usize>()
        + 128
}
pub(super) fn shift(
    mut m: SolverModel,
    rows: bool,
    at: usize,
    count: usize,
    delete: bool,
) -> Option<SolverModel> {
    let point = |(r, c)| {
        let out = shift_range(
            Range {
                top: r,
                left: c,
                bottom: r,
                right: c,
            },
            rows,
            at,
            count,
            delete,
        )?;
        Some((out.top, out.left))
    };
    m.objective = point(m.objective)?;
    m.variables = m.variables.into_iter().filter_map(point).collect();
    if m.variables.is_empty() {
        return None;
    }
    m.constraints = m
        .constraints
        .into_iter()
        .filter_map(|mut c| {
            c.lhs = shift_range(c.lhs, rows, at, count, delete)?;
            if let Some(r) = range(&c.rhs, MAX_ROWS, MAX_COLS) {
                let r = shift_range(r, rows, at, count, delete)?;
                c.rhs = format!(
                    "${}${}:${}${}",
                    office::col_name(r.left),
                    r.top + 1,
                    office::col_name(r.right),
                    r.bottom + 1
                );
            }
            Some(c)
        })
        .collect();
    Some(m)
}
