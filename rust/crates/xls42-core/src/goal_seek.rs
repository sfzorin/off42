use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub struct GoalSeekTask {
    source: State,
    source_id: u64,
    trial: Workbook,
    formula: (usize, usize),
    variable: (usize, usize),
    target: f64,
    start: f64,
    now: chrono::NaiveDateTime,
}
pub struct GoalSeekResult {
    source: State,
    source_id: u64,
    formula: (usize, usize),
    variable: (usize, usize),
    target: f64,
    value: f64,
    calculated: f64,
    evaluations: usize,
}
impl GoalSeekResult {
    pub fn formula(&self) -> (usize, usize) {
        self.formula
    }
    pub fn variable(&self) -> (usize, usize) {
        self.variable
    }
    pub fn target(&self) -> f64 {
        self.target
    }
    pub fn value(&self) -> f64 {
        self.value
    }
    pub fn calculated(&self) -> f64 {
        self.calculated
    }
    pub fn evaluations(&self) -> usize {
        self.evaluations
    }
}
impl Workbook {
    /// Freeze a readonly numerical job; only apply_goal_seek can change the live model.
    pub fn goal_seek_task(
        &self,
        formula: (usize, usize),
        target: f64,
        variable: (usize, usize),
    ) -> Result<GoalSeekTask, Error> {
        self.require_editable()?;
        if self.is_protected() {
            return Err(Error::Protected);
        }
        if !target.is_finite() {
            return Err(Error::Invalid("Enter a finite target value"));
        }
        if !matches!(
            self.cell_value(formula.0, formula.1),
            Some(CellValue::Formula(_))
        ) {
            return Err(Error::Invalid("Choose a formula cell"));
        }
        let start = match self.cell_value(variable.0, variable.1) {
            Some(CellValue::Number(value)) if value.is_finite() => value,
            Some(CellValue::Blank) => 0.,
            _ => {
                return Err(Error::Invalid(
                    "The variable must be an empty or numeric cell",
                ))
            }
        };
        if self.history.current().sheet().merges.iter().any(|r| {
            variable.0 >= r.top
                && variable.0 <= r.bottom
                && variable.1 >= r.left
                && variable.1 <= r.right
                && variable != (r.top, r.left)
        }) {
            return Err(Error::Invalid("Choose the top-left cell of a merged range"));
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
        Ok(GoalSeekTask {
            source,
            source_id: self.history.current_id(),
            trial,
            formula,
            variable,
            target,
            start,
            now: chrono::Local::now().naive_local(),
        })
    }
    pub fn apply_goal_seek(&mut self, result: &GoalSeekResult) -> Result<bool, Error> {
        if self.history.current_id() != result.source_id || self.history.current() != &result.source
        {
            return Err(Error::Invalid("The workbook changed; run Goal Seek again"));
        }
        self.set_cell(
            result.variable.0,
            result.variable.1,
            result.value.to_string(),
        )
    }
}
impl GoalSeekTask {
    pub fn solve(mut self, cancel: &AtomicBool) -> Result<GoalSeekResult, String> {
        let began = Instant::now();
        let mut evaluations = 0usize;
        let tolerance = 1e-10 * self.target.abs().max(1.);
        let mut evaluate = |x: f64| -> Result<Option<(f64, f64)>, String> {
            if cancel.load(Ordering::Relaxed) {
                return Err("Goal Seek canceled".into());
            }
            if evaluations >= 1000 || began.elapsed() > Duration::from_secs(10) {
                return Err("Goal Seek reached its calculation limit".into());
            }
            if !x.is_finite() {
                return Ok(None);
            }
            evaluations += 1;
            let mut state = self.trial.history.current().clone();
            let cell = state
                .sheet_mut()
                .cells
                .entry(self.variable)
                .or_insert_with(|| Cell {
                    text: CellContent::default(),
                    style: 0,
                });
            cell.text = CellContent::from_raw(x.to_string());
            self.trial.history.replace_current(state);
            let value = match formula::numeric_cell_at(&self.trial, self.formula, self.now) {
                Ok(value) => value,
                Err(_) => return Ok(None),
            };
            let residual = value - self.target;
            Ok(residual.is_finite().then_some((residual, value)))
        };
        let mut x0 = self.start;
        let Some((mut y0, initial)) = evaluate(x0)? else {
            return Err(
                "The formula does not produce a finite number at the starting value".into(),
            );
        };
        let mut found = (y0.abs() <= tolerance).then_some((x0, initial));
        let mut x1 = if x0 == 0. { 0.01 } else { x0 * 1.01 };
        let mut last = None;
        if found.is_none() {
            if let Some((mut y1, mut value1)) = evaluate(x1)? {
                for _ in 0..200 {
                    if y1.abs() <= tolerance {
                        found = Some((x1, value1));
                        break;
                    }
                    last = Some((x1, y1, value1));
                    let delta = y1 - y0;
                    if delta == 0. {
                        break;
                    }
                    let x2 = x1 - y1 * (x1 - x0) / delta;
                    let Some((y2, value2)) = evaluate(x2)? else {
                        break;
                    };
                    x0 = x1;
                    y0 = y1;
                    x1 = x2;
                    y1 = y2;
                    value1 = value2;
                }
                if found.is_none() && y1.abs() <= tolerance {
                    found = Some((x1, value1));
                }
            }
        }
        if found.is_none() {
            if let Some((x, error, value)) =
                last.filter(|(_, error, _)| error.abs() <= 1e-7 * self.target.abs().max(1.))
            {
                let _ = error;
                found = Some((x, value));
            }
        }
        let mut width = self.start.abs().max(1.);
        for _ in 0..60 {
            if found.is_some() {
                break;
            }
            let lo = self.start - width;
            let hi = self.start + width;
            width *= 2.;
            let (Some((fa, va)), Some((fb, vb))) = (evaluate(lo)?, evaluate(hi)?) else {
                continue;
            };
            if fa.abs() <= tolerance {
                found = Some((lo, va));
                break;
            }
            if fb.abs() <= tolerance {
                found = Some((hi, vb));
                break;
            }
            if fa.is_sign_negative() == fb.is_sign_negative() {
                continue;
            }
            let (mut lo, mut hi, mut flo) = (lo, hi, fa);
            for _ in 0..200 {
                let mid = lo / 2. + hi / 2.;
                let Some((fm, value)) = evaluate(mid)? else {
                    break;
                };
                if fm.abs() <= tolerance {
                    found = Some((mid, value));
                    break;
                }
                // A tiny interval alone is not convergence: discontinuous formulas can jump over the target.
                if mid == lo || mid == hi {
                    break;
                }
                if fm.is_sign_negative() == flo.is_sign_negative() {
                    lo = mid;
                    flo = fm;
                } else {
                    hi = mid;
                }
            }
        }
        let Some((value, calculated)) = found else {
            return Err("Goal Seek could not find a value that gives the target".into());
        };
        if cancel.load(Ordering::Relaxed) {
            return Err("Goal Seek canceled".into());
        }
        Ok(GoalSeekResult {
            source: self.source,
            source_id: self.source_id,
            formula: self.formula,
            variable: self.variable,
            target: self.target,
            value,
            calculated,
            evaluations,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn solve(
        book: &Workbook,
        formula: (usize, usize),
        target: f64,
        variable: (usize, usize),
    ) -> GoalSeekResult {
        book.goal_seek_task(formula, target, variable)
            .unwrap()
            .solve(&AtomicBool::new(false))
            .unwrap()
    }
    #[test]
    fn square_root_trial_is_readonly_and_accept_is_one_complete_undo_step() {
        let mut book = Workbook::new(2, 3).unwrap();
        book.set_cell(0, 0, "1".into()).unwrap();
        book.set_cell(0, 1, "=A1^2".into()).unwrap();
        let source = book.history.current().clone();
        let id = book.history.current_id();
        let result = solve(&book, (0, 1), 2., (0, 0));
        assert!((result.value - 2f64.sqrt()).abs() < 1e-8);
        assert!((result.calculated - 2.).abs() < 1e-9);
        assert!(book.history.current() == &source);
        assert_eq!(book.history.current_id(), id);
        book.apply_goal_seek(&result).unwrap();
        let changed = book.history.current().clone();
        book.undo();
        assert!(book.history.current() == &source);
        book.redo();
        assert!(book.history.current() == &changed);
    }
    #[test]
    fn mac_loan_example_and_cross_sheet_dependencies_match_numeric_targets() {
        let mut book = Workbook::new(1, 4).unwrap();
        for (c, value) in ["0.05", "10", "=PMT(A1/12,B1*12,D1)", "1000"]
            .iter()
            .enumerate()
        {
            book.set_cell(0, c, (*value).into()).unwrap();
        }
        let result = solve(&book, (0, 2), -500., (0, 3));
        let rate = 0.05f64 / 12.;
        let exact = 500. * (1. - (1. + rate).powi(-120)) / rate;
        assert!((result.value - exact).abs() < 1e-6);
        let first = book.active_sheet();
        let second = book.add_sheet("Other").unwrap();
        book.select_sheet(second).unwrap();
        book.set_cell(0, 0, "=Sheet1!D1*2".into()).unwrap();
        book.select_sheet(first).unwrap();
        book.set_cell(0, 2, "=Other!A1+1".into()).unwrap();
        assert!((solve(&book, (0, 2), 17., (0, 3)).value - 8.).abs() < 1e-9);
    }
    #[test]
    fn bisection_refuses_discontinuity_and_failure_cancel_or_stale_results_never_change_history() {
        let mut book = Workbook::new(2, 3).unwrap();
        book.set_cell(0, 0, "0".into()).unwrap();
        book.set_cell(0, 1, "=IF(A1<0,-1,1)".into()).unwrap();
        let source = book.history.current().clone();
        assert!(book
            .goal_seek_task((0, 1), 0., (0, 0))
            .unwrap()
            .solve(&AtomicBool::new(false))
            .is_err());
        assert!(book.history.current() == &source);
        book.set_cell(0, 1, "=A1*2".into()).unwrap();
        assert!(book
            .goal_seek_task((0, 1), 10., (0, 0))
            .unwrap()
            .solve(&AtomicBool::new(true))
            .is_err());
        let result = solve(&book, (0, 1), 10., (0, 0));
        book.set_cell(1, 0, "changed".into()).unwrap();
        let current = book.history.current().clone();
        assert!(book.apply_goal_seek(&result).is_err());
        assert!(book.history.current() == &current);
        assert!(book.goal_seek_task((0, 0), 0., (0, 1)).is_err());
        assert!(book.goal_seek_task((0, 1), f64::NAN, (0, 0)).is_err());
        assert!(book.set_protected(true));
        assert!(book.goal_seek_task((0, 1), 10., (0, 0)).is_err());
    }
    #[test]
    fn bisection_blank_cell_types_and_final_validation_use_real_trial_values() {
        let mut book = Workbook::new(1, 3).unwrap();
        book.set_cell(0, 1, "=IF(A1<0,A1+1,1)".into()).unwrap();
        assert!((solve(&book, (0, 1), 0.3, (0, 0)).value + 0.7).abs() < 1e-8);
        book.set_cell(0, 1, "=IF(ISBLANK(A1),99,A1*2)".into())
            .unwrap();
        let result = solve(&book, (0, 1), 10., (0, 0));
        assert!((result.value - 5.).abs() < 1e-8);
        assert_eq!(book.cell(0, 0), Some(""));
        book.add_validation(DataValidation {
            range: Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            allow_blank: true,
            kind: ValidationKind::Number {
                minimum: 0.,
                maximum: 1.,
            },
        })
        .unwrap();
        let result = solve(&book, (0, 1), 10., (0, 0));
        let source = book.history.current().clone();
        assert!(book.apply_goal_seek(&result).is_err());
        assert!(book.history.current() == &source);
    }
}
