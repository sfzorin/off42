use super::task::Point;
use super::*;
use std::collections::BTreeSet;
fn minimize<F>(mut evaluate: F, start: &[f64]) -> Result<Vec<f64>, String>
where
    F: FnMut(&[f64]) -> Result<f64, String>,
{
    let n = start.len();
    let mut best = start.to_vec();
    for restart in 0..4 {
        let mut simplex = vec![best.clone()];
        for j in 0..n {
            let mut p = best.clone();
            p[j] += 0.5f64.max(p[j].abs() * 0.1) / (restart + 1) as f64;
            simplex.push(p);
        }
        let mut values = simplex
            .iter()
            .map(|p| evaluate(p))
            .collect::<Result<Vec<_>, _>>()?;
        for _ in 0..400 * n.max(1) {
            let mut order: Vec<_> = (0..=n).collect();
            order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
            simplex = order.iter().map(|&i| simplex[i].clone()).collect();
            values = order.iter().map(|&i| values[i]).collect();
            if values[0].is_finite()
                && values[n].is_finite()
                && (values[n] - values[0]).abs() <= 1e-13 * (values[0].abs() + 1e-13)
            {
                break;
            }
            let centroid: Vec<_> = (0..n)
                .map(|j| simplex[..n].iter().map(|p| p[j] / n as f64).sum::<f64>())
                .collect();
            let along = |t: f64| {
                (0..n)
                    .map(|j| centroid[j] + t * (simplex[n][j] - centroid[j]))
                    .collect::<Vec<_>>()
            };
            let reflected = along(-1.);
            let fr = evaluate(&reflected)?;
            if fr < values[0] {
                let expanded = along(-2.);
                let fe = evaluate(&expanded)?;
                if fe < fr {
                    simplex[n] = expanded;
                    values[n] = fe;
                } else {
                    simplex[n] = reflected;
                    values[n] = fr;
                }
            } else if fr < values[n - 1] {
                simplex[n] = reflected;
                values[n] = fr;
            } else {
                let contracted = along(if fr < values[n] { -0.5 } else { 0.5 });
                let fc = evaluate(&contracted)?;
                if fc < fr.min(values[n]) {
                    simplex[n] = contracted;
                    values[n] = fc;
                } else {
                    for i in 1..=n {
                        for j in 0..n {
                            simplex[i][j] = simplex[0][j] + 0.5 * (simplex[i][j] - simplex[0][j]);
                        }
                        values[i] = evaluate(&simplex[i])?;
                    }
                }
            }
        }
        let index = (0..=n)
            .min_by(|&a, &b| values[a].total_cmp(&values[b]))
            .unwrap();
        if evaluate(&simplex[index])? <= evaluate(&best)? {
            best = simplex[index].clone();
        }
    }
    Ok(best)
}
fn violation(values: &[f64], point: &Point, ops: &[SolverOp], non_negative: bool) -> f64 {
    if point.rows.len() != ops.len() {
        return f64::INFINITY;
    }
    let mut total = if non_negative {
        values
            .iter()
            .filter(|v| **v < 0.)
            .map(|v| v * v)
            .sum::<f64>()
    } else {
        0.
    };
    for (&value, op) in point.rows.iter().zip(ops) {
        let invalid = match op {
            SolverOp::Le => value.max(0.),
            SolverOp::Ge => value.min(0.),
            _ => value,
        };
        total += invalid * invalid;
    }
    if total.is_finite() {
        total
    } else {
        f64::INFINITY
    }
}
pub(super) fn solve<F>(
    model: &SolverModel,
    ops: &[SolverOp],
    integer: &BTreeSet<usize>,
    binary: &BTreeSet<usize>,
    start: &[f64],
    evaluate: &mut F,
    budget: &mut Budget<'_>,
) -> Result<engine::Outcome, String>
where
    F: FnMut(&[f64], &mut Budget<'_>) -> Result<Option<Point>, String>,
{
    let sign = match model.goal {
        SolverGoal::Maximum => 1.,
        SolverGoal::Minimum => -1.,
        SolverGoal::Value(_) => 0.,
    };
    let mut values: Vec<_> = start
        .iter()
        .map(|&value| {
            if model.non_negative {
                value.max(0.)
            } else {
                value
            }
        })
        .collect();
    let mut mu = 10.;
    for _ in 0..8 {
        values = minimize(
            |values| score(values, mu, sign, model, ops, evaluate, budget),
            &values,
        )?;
        let invalid = evaluate(&values, budget)?
            .map(|point| violation(&values, &point, ops, model.non_negative))
            .unwrap_or(f64::INFINITY);
        if invalid < 1e-14 {
            break;
        }
        mu *= 100.;
    }
    for &j in integer {
        values[j] = values[j].round();
        if binary.contains(&j) {
            values[j] = values[j].clamp(0., 1.);
        }
    }
    for _ in 0..200 {
        let mut changed = false;
        let mut current = score(&values, 1e6, sign, model, ops, evaluate, budget)?;
        for &j in integer {
            for delta in [-1., 1.] {
                let mut point = values.clone();
                point[j] += delta;
                if binary.contains(&j) {
                    point[j] = point[j].clamp(0., 1.);
                } else if model.non_negative {
                    point[j] = point[j].max(0.);
                }
                if point == values {
                    continue;
                }
                let next = score(&point, 1e6, sign, model, ops, evaluate, budget)?;
                if next < current - 1e-12 {
                    values = point;
                    current = next;
                    changed = true;
                    break;
                }
            }
        }
        if !changed {
            break;
        }
    }
    let Some(point) = evaluate(&values, budget)? else {
        return Ok(engine::Outcome {
            values: Vec::new(),
            objective: 0.,
            status: task::SolverStatus::Failed,
            engine: "Nelder–Mead",
        });
    };
    let status = if engine::feasible(&values, &point, ops, model.non_negative, integer) {
        task::SolverStatus::Approximate
    } else {
        task::SolverStatus::Infeasible
    };
    Ok(engine::Outcome {
        values,
        objective: point.objective,
        status,
        engine: "Nelder–Mead",
    })
}

fn score<F>(
    values: &[f64],
    mu: f64,
    sign: f64,
    model: &SolverModel,
    ops: &[SolverOp],
    evaluate: &mut F,
    budget: &mut Budget<'_>,
) -> Result<f64, String>
where
    F: FnMut(&[f64], &mut Budget<'_>) -> Result<Option<Point>, String>,
{
    let Some(point) = evaluate(values, budget)? else {
        return Ok(f64::INFINITY);
    };
    let score = -sign * point.objective + mu * violation(values, &point, ops, model.non_negative);
    Ok(if score.is_finite() {
        score
    } else {
        f64::INFINITY
    })
}
