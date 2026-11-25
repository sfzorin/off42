use super::*;
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SolverStatus {
    Optimal,
    Approximate,
    Infeasible,
    Unbounded,
    Failed,
}
pub struct SolverResult {
    source: State,
    source_id: u64,
    model: SolverModel,
    values: Vec<f64>,
    objective: f64,
    status: SolverStatus,
    engine: &'static str,
}
impl SolverResult {
    pub fn model(&self) -> &SolverModel {
        &self.model
    }
    pub fn values(&self) -> &[f64] {
        &self.values
    }
    pub fn objective(&self) -> Option<f64> {
        self.can_keep().then_some(self.objective)
    }
    pub fn status(&self) -> SolverStatus {
        self.status
    }
    pub fn engine(&self) -> &'static str {
        self.engine
    }
    pub fn can_keep(&self) -> bool {
        matches!(
            self.status,
            SolverStatus::Optimal | SolverStatus::Approximate
        )
    }
}
#[derive(Clone, Copy)]
enum Operand {
    Constant(f64),
    Cell(usize),
}
struct Check {
    left: usize,
    right: Operand,
}
pub(super) struct Point {
    pub objective: f64,
    pub rows: Vec<f64>,
}
pub struct SolverTask {
    source: State,
    source_id: u64,
    model: SolverModel,
    trial: Workbook,
    now: chrono::NaiveDateTime,
    cells: Vec<(usize, usize)>,
    checks: Vec<Check>,
    ops: Vec<SolverOp>,
    integer: BTreeSet<usize>,
    binary: BTreeSet<usize>,
    start: Vec<f64>,
}
fn references(range: Range) -> impl Iterator<Item = (usize, usize)> {
    (range.top..=range.bottom)
        .flat_map(move |row| (range.left..=range.right).map(move |col| (row, col)))
}
impl Workbook {
    pub fn solver_task(&self, model: SolverModel) -> Result<SolverTask, Error> {
        self.require_editable()?;
        if self.is_protected() {
            return Err(Error::Protected);
        }
        if !valid_model(&model, self.rows(), self.cols()) {
            return Err(Error::Invalid("Invalid Solver cells or constraints"));
        }
        let start = model
            .variables
            .iter()
            .map(|&(row, col)| match self.cell_value(row, col) {
                Some(CellValue::Blank) => Ok(0.),
                Some(CellValue::Number(value)) if value.is_finite() => Ok(value),
                _ => Err(Error::Invalid(
                    "Solver variables must be empty or numeric cells",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        for &(row, col) in &model.variables {
            if self.history.current().sheet().merges.iter().any(|range| {
                row >= range.top
                    && row <= range.bottom
                    && col >= range.left
                    && col <= range.right
                    && (row, col) != (range.top, range.left)
            }) {
                return Err(Error::Invalid("Choose the top-left cell of a merged range"));
            }
        }
        let mut cells = vec![model.objective];
        let mut checks = Vec::new();
        let mut ops = Vec::new();
        let mut integer = BTreeSet::new();
        let mut binary = BTreeSet::new();
        for constraint in &model.constraints {
            let lhs: Vec<_> = references(constraint.lhs).collect();
            if matches!(constraint.op, SolverOp::Integer | SolverOp::Binary) {
                for cell in lhs {
                    let index = model
                        .variables
                        .iter()
                        .position(|value| *value == cell)
                        .ok_or(Error::Invalid(
                            "Integer/binary constraints must refer to variable cells",
                        ))?;
                    integer.insert(index);
                    if constraint.op == SolverOp::Binary {
                        binary.insert(index);
                    }
                }
                continue;
            }
            let constant = constraint
                .rhs
                .trim()
                .replace(',', ".")
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite());
            let rhs = if constant.is_none() {
                Some(
                    references(
                        range(&constraint.rhs, self.rows(), self.cols())
                            .ok_or(Error::Invalid("Invalid Solver right-hand side"))?,
                    )
                    .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            for (index, cell) in lhs.iter().enumerate() {
                let left = cell_index(&mut cells, *cell);
                let right = if let Some(value) = constant {
                    Operand::Constant(value)
                } else {
                    let rhs = rhs.as_ref().unwrap();
                    Operand::Cell(cell_index(
                        &mut cells,
                        rhs[if rhs.len() == lhs.len() { index } else { 0 }],
                    ))
                };
                checks.push(Check { left, right });
                ops.push(constraint.op);
            }
        }
        for _ in &binary {
            ops.extend([SolverOp::Le, SolverOp::Le]);
        }
        if matches!(model.goal, SolverGoal::Value(_)) {
            ops.push(SolverOp::Eq);
        }
        let source = self.history.current().clone();
        let trial = Workbook {
            history: SnapshotHistory::new(source.clone(), 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        };
        Ok(SolverTask {
            source,
            source_id: self.history.current_id(),
            model,
            trial,
            now: chrono::Local::now().naive_local(),
            cells,
            checks,
            ops,
            integer,
            binary,
            start,
        })
    }
    /// Keep the model with the sheet on either choice; values change only when explicitly accepted.
    pub fn apply_solver_result(
        &mut self,
        result: &SolverResult,
        keep_values: bool,
    ) -> Result<bool, Error> {
        if self.history.current_id() != result.source_id || self.history.current() != &result.source
        {
            return Err(Error::Invalid("The workbook changed; run Solver again"));
        }
        self.require_editable()?;
        if self.is_protected() {
            return Err(Error::Protected);
        }
        if keep_values && !result.can_keep() {
            return Err(Error::Invalid("Solver has no feasible result to apply"));
        }
        let mut candidate = Workbook {
            history: SnapshotHistory::new(self.history.current().clone(), 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: self.external_source.clone(),
            external_baseline: self.external_baseline.clone(),
            external_format: self.external_format.clone(),
            external_cache: self.external_cache.clone(),
        };
        if keep_values {
            for (&(row, col), value) in result.model.variables.iter().zip(&result.values) {
                candidate.set_cell(row, col, value.to_string())?;
            }
        }
        let mut state = candidate.history.current().clone();
        state.sheet_mut().solver = Some(result.model.clone());
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        Ok(self.history.apply(state))
    }
}
fn cell_index(cells: &mut Vec<(usize, usize)>, cell: (usize, usize)) -> usize {
    if let Some(index) = cells.iter().position(|value| *value == cell) {
        index
    } else {
        cells.push(cell);
        cells.len() - 1
    }
}
impl SolverTask {
    fn point(&mut self, values: &[f64], budget: &mut Budget<'_>) -> Result<Option<Point>, String> {
        budget.tick()?;
        if values.len() != self.model.variables.len()
            || values.iter().any(|value| !value.is_finite())
        {
            return Ok(None);
        }
        let mut state = self.source.clone();
        for (&cell, value) in self.model.variables.iter().zip(values) {
            let target = state.sheet_mut().cells.entry(cell).or_insert_with(|| Cell {
                text: CellContent::default(),
                style: 0,
            });
            target.text = CellContent::from_raw(value.to_string());
        }
        self.trial.history.replace_current(state);
        let Some(calculated) = formula::optimization_values_at(&self.trial, &self.cells, self.now)
        else {
            return Ok(None);
        };
        let objective = calculated[0];
        let mut rows: Vec<_> = self
            .checks
            .iter()
            .map(|check| {
                calculated[check.left]
                    - match check.right {
                        Operand::Constant(value) => value,
                        Operand::Cell(index) => calculated[index],
                    }
            })
            .collect();
        for &index in &self.binary {
            rows.extend([values[index] - 1., -values[index]]);
        }
        if let SolverGoal::Value(target) = self.model.goal {
            rows.push(objective - target);
        }
        if rows.iter().any(|value| !value.is_finite()) {
            return Ok(None);
        }
        Ok(Some(Point { objective, rows }))
    }
    pub fn solve(mut self, cancel: &AtomicBool) -> Result<SolverResult, String> {
        let model = self.model.clone();
        let ops = self.ops.clone();
        let integer = self.integer.clone();
        let binary = self.binary.clone();
        let start = self.start.clone();
        let mut budget = Budget::new(cancel);
        let result = engine::solve(
            &model,
            &ops,
            &integer,
            &binary,
            &start,
            |values, budget| self.point(values, budget),
            &mut budget,
        )?;
        Ok(SolverResult {
            source: self.source,
            source_id: self.source_id,
            model: self.model,
            values: result.values,
            objective: result.objective,
            status: result.status,
            engine: result.engine,
        })
    }
}
