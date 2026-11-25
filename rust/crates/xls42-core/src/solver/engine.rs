use super::task::{Point, SolverStatus};
use super::*;
use std::collections::BTreeSet;
pub(super) struct Outcome {
    pub values: Vec<f64>,
    pub objective: f64,
    pub status: SolverStatus,
    pub engine: &'static str,
}
struct Linear {
    objective: Vec<f64>,
    rows: Vec<linear::Row>,
}
fn linearize<F>(
    n: usize,
    ops: &[SolverOp],
    evaluate: &mut F,
    budget: &mut Budget<'_>,
) -> Result<Option<Linear>, String>
where
    F: FnMut(&[f64], &mut Budget<'_>) -> Result<Option<Point>, String>,
{
    let zero = vec![0.; n];
    let Some(base) = evaluate(&zero, budget)? else {
        return Ok(None);
    };
    if base.rows.len() != ops.len() {
        return Ok(None);
    }
    let mut objective = vec![0.; n];
    let mut rows: Vec<_> = base
        .rows
        .iter()
        .zip(ops)
        .map(|(value, &op)| linear::Row {
            coefficients: vec![0.; n],
            op,
            rhs: -value,
        })
        .collect();
    for j in 0..n {
        let mut point = zero.clone();
        point[j] = 1.;
        let Some(value) = evaluate(&point, budget)? else {
            return Ok(None);
        };
        if value.rows.len() != ops.len() {
            return Ok(None);
        }
        objective[j] = value.objective - base.objective;
        for (k, row) in rows.iter_mut().enumerate() {
            row.coefficients[j] = value.rows[k] - base.rows[k];
        }
    }
    let close = |a: f64, b: f64| {
        a.is_finite() && b.is_finite() && (a - b).abs() <= 1e-7 * a.abs().max(b.abs()).max(1.)
    };
    for scale in [3.7, -1.3] {
        let point: Vec<_> = (0..n).map(|i| (i + 1) as f64 * scale).collect();
        let Some(value) = evaluate(&point, budget)? else {
            return Ok(None);
        };
        if !close(
            value.objective,
            base.objective
                + objective
                    .iter()
                    .zip(&point)
                    .map(|(a, b)| a * b)
                    .sum::<f64>(),
        ) || value.rows.len() != ops.len()
        {
            return Ok(None);
        }
        for (k, row) in rows.iter().enumerate() {
            if !close(
                value.rows[k],
                base.rows[k]
                    + row
                        .coefficients
                        .iter()
                        .zip(&point)
                        .map(|(a, b)| a * b)
                        .sum::<f64>(),
            ) {
                return Ok(None);
            }
        }
    }
    Ok(Some(Linear { objective, rows }))
}
pub(super) fn feasible(
    values: &[f64],
    point: &Point,
    ops: &[SolverOp],
    non_negative: bool,
    integer: &BTreeSet<usize>,
) -> bool {
    point.objective.is_finite()
        && point.rows.len() == ops.len()
        && values
            .iter()
            .all(|v| v.is_finite() && (!non_negative || *v >= -1e-7))
        && integer
            .iter()
            .all(|&j| (values[j] - values[j].round()).abs() <= 1e-7)
        && point.rows.iter().zip(ops).all(|(&value, op)| {
            value.is_finite()
                && match op {
                    SolverOp::Le => value <= 1e-6,
                    SolverOp::Ge => value >= -1e-6,
                    SolverOp::Eq => value.abs() <= 1e-6,
                    _ => false,
                }
        })
}
fn linear_solve<F>(
    model: &SolverModel,
    problem: Linear,
    ops: &[SolverOp],
    integer: &BTreeSet<usize>,
    evaluate: &mut F,
    budget: &mut Budget<'_>,
) -> Result<Outcome, String>
where
    F: FnMut(&[f64], &mut Budget<'_>) -> Result<Option<Point>, String>,
{
    let n = model.variables.len();
    let expand = |row: &[f64]| {
        if model.non_negative {
            row.to_vec()
        } else {
            row.iter().copied().chain(row.iter().map(|v| -v)).collect()
        }
    };
    let collapse = |values: &[f64]| {
        if model.non_negative {
            values.to_vec()
        } else {
            (0..n).map(|j| values[j] - values[j + n]).collect()
        }
    };
    let sign = match model.goal {
        SolverGoal::Maximum => 1.,
        SolverGoal::Minimum => -1.,
        SolverGoal::Value(_) => 0.,
    };
    let costs: Vec<_> = problem.objective.iter().map(|value| value * sign).collect();
    let objective = expand(&costs);
    let base: Vec<_> = problem
        .rows
        .into_iter()
        .map(|row| linear::Row {
            coefficients: expand(&row.coefficients),
            op: row.op,
            rhs: row.rhs,
        })
        .collect();
    let mut pending = vec![Vec::<linear::Row>::new()];
    let mut nodes = 0;
    let mut best: Option<(Vec<f64>, f64, f64)> = None;
    while let Some(extra) = pending.pop() {
        budget.tick()?;
        nodes += 1;
        if nodes > 4000 {
            pending.push(extra);
            break;
        }
        let mut rows = base.clone();
        rows.extend(extra.iter().cloned());
        let (solution, score, bounded) = match linear::maximize(&objective, &rows, budget)? {
            linear::Solution::Infeasible => continue,
            linear::Solution::Optimal(values, score) => (collapse(&values), score, true),
            linear::Solution::Unbounded(values) => (collapse(&values), f64::INFINITY, false),
        };
        if bounded
            && best
                .as_ref()
                .is_some_and(|(_, value, _)| score <= *value + 1e-9)
        {
            continue;
        }
        if let Some(&j) = integer
            .iter()
            .find(|&&j| (solution[j] - solution[j].round()).abs() > 1e-7)
        {
            if extra.len() >= 128 {
                return Err("Solver reached its integer search depth limit".into());
            }
            let mut coefficients = vec![0.; n];
            coefficients[j] = 1.;
            let coefficients = expand(&coefficients);
            let mut lower = extra.clone();
            lower.push(linear::Row {
                coefficients: coefficients.clone(),
                op: SolverOp::Le,
                rhs: solution[j].floor(),
            });
            let mut upper = extra;
            upper.push(linear::Row {
                coefficients,
                op: SolverOp::Ge,
                rhs: solution[j].ceil(),
            });
            pending.push(upper);
            pending.push(lower);
            continue;
        }
        let values: Vec<_> = solution
            .into_iter()
            .enumerate()
            .map(|(j, v)| if integer.contains(&j) { v.round() } else { v })
            .collect();
        let Some(point) = evaluate(&values, budget)? else {
            return Err("Solver could not verify its linear result".into());
        };
        if !feasible(&values, &point, ops, model.non_negative, integer) {
            return Err("Solver could not verify all result constraints".into());
        }
        if !bounded {
            return Ok(Outcome {
                values: Vec::new(),
                objective: 0.,
                status: SolverStatus::Unbounded,
                engine: "simplex",
            });
        }
        best = Some((values, score, point.objective));
    }
    Ok(if let Some((values, _, objective)) = best {
        Outcome {
            values,
            objective,
            status: if pending.is_empty() {
                SolverStatus::Optimal
            } else {
                SolverStatus::Approximate
            },
            engine: "simplex",
        }
    } else {
        Outcome {
            values: Vec::new(),
            objective: 0.,
            status: if pending.is_empty() {
                SolverStatus::Infeasible
            } else {
                SolverStatus::Failed
            },
            engine: "simplex",
        }
    })
}
pub(super) fn solve<F>(
    model: &SolverModel,
    ops: &[SolverOp],
    integer: &BTreeSet<usize>,
    binary: &BTreeSet<usize>,
    start: &[f64],
    mut evaluate: F,
    budget: &mut Budget<'_>,
) -> Result<Outcome, String>
where
    F: FnMut(&[f64], &mut Budget<'_>) -> Result<Option<Point>, String>,
{
    if model.linear {
        if let Some(problem) = linearize(model.variables.len(), ops, &mut evaluate, budget)? {
            return linear_solve(model, problem, ops, integer, &mut evaluate, budget);
        }
    }
    nonlinear::solve(model, ops, integer, binary, start, &mut evaluate, budget)
}
