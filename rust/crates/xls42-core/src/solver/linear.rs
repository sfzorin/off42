//! Two-phase simplex with Bland's rule; no live workbook state is involved.
use super::*;
#[derive(Clone)]
pub(super) struct Row {
    pub coefficients: Vec<f64>,
    pub op: SolverOp,
    pub rhs: f64,
}
pub(super) enum Solution {
    Optimal(Vec<f64>, f64),
    Infeasible,
    Unbounded(Vec<f64>),
}
fn pivot(table: &mut [Vec<f64>], basis: &mut [usize], row: usize, column: usize) {
    let p = table[row][column];
    for value in &mut table[row] {
        *value /= p;
    }
    for i in 0..table.len() {
        if i == row {
            continue;
        }
        let factor = table[i][column];
        if factor == 0. {
            continue;
        }
        for j in 0..table[i].len() {
            table[i][j] -= factor * table[row][j];
        }
    }
    basis[row] = column;
}
fn run(
    table: &mut [Vec<f64>],
    basis: &mut [usize],
    objective: &[f64],
    allowed: &[bool],
    budget: &mut Budget<'_>,
) -> Result<bool, String> {
    for _ in 0..5000 {
        budget.tick()?;
        let enter = (0..objective.len()).find(|&j| {
            allowed[j]
                && !basis.contains(&j)
                && objective[j]
                    - basis
                        .iter()
                        .enumerate()
                        .map(|(i, &b)| objective[b] * table[i][j])
                        .sum::<f64>()
                    > 1e-10
        });
        let Some(column) = enter else {
            return Ok(true);
        };
        let mut leave = None;
        let mut ratio = f64::INFINITY;
        for i in 0..table.len() {
            if table[i][column] <= 1e-12 {
                continue;
            }
            let value = table[i][table[i].len() - 1] / table[i][column];
            if value < ratio - 1e-12
                || (value - ratio).abs() <= 1e-12
                    && leave.is_none_or(|r: usize| basis[i] < basis[r])
            {
                ratio = value;
                leave = Some(i);
            }
        }
        let Some(row) = leave else {
            return Ok(false);
        };
        pivot(table, basis, row, column);
        if table.iter().flatten().any(|value| !value.is_finite()) {
            return Err("Solver encountered unstable linear coefficients".into());
        }
    }
    Err("Solver reached its pivot limit".into())
}
pub(super) fn maximize(
    coefficients: &[f64],
    rows: &[Row],
    budget: &mut Budget<'_>,
) -> Result<Solution, String> {
    let n = coefficients.len();
    if n == 0
        || n > 64
        || rows.len() > 512
        || coefficients.iter().any(|v| !v.is_finite())
        || rows.iter().any(|r| {
            r.coefficients.len() != n
                || !r.rhs.is_finite()
                || r.coefficients.iter().any(|v| !v.is_finite())
                || !matches!(r.op, SolverOp::Le | SolverOp::Ge | SolverOp::Eq)
        })
    {
        return Err("Invalid linear Solver model".into());
    }
    let rows: Vec<_> = rows
        .iter()
        .map(|row| {
            if row.rhs < 0. {
                Row {
                    coefficients: row.coefficients.iter().map(|v| -v).collect(),
                    op: match row.op {
                        SolverOp::Le => SolverOp::Ge,
                        SolverOp::Ge => SolverOp::Le,
                        other => other,
                    },
                    rhs: -row.rhs,
                }
            } else {
                row.clone()
            }
        })
        .collect();
    let slack = rows.iter().filter(|row| row.op != SolverOp::Eq).count();
    let artificial_count = rows.iter().filter(|row| row.op != SolverOp::Le).count();
    let width = n + slack + artificial_count + 1;
    let mut table = vec![vec![0.; width]; rows.len()];
    let mut basis = vec![0; rows.len()];
    let mut artificial = vec![false; width - 1];
    let (mut s, mut a) = (n, n + slack);
    for (i, row) in rows.iter().enumerate() {
        table[i][..n].copy_from_slice(&row.coefficients);
        table[i][width - 1] = row.rhs;
        match row.op {
            SolverOp::Le => {
                table[i][s] = 1.;
                basis[i] = s;
                s += 1;
            }
            SolverOp::Ge => {
                table[i][s] = -1.;
                s += 1;
                table[i][a] = 1.;
                basis[i] = a;
                artificial[a] = true;
                a += 1;
            }
            _ => {
                table[i][a] = 1.;
                basis[i] = a;
                artificial[a] = true;
                a += 1;
            }
        }
    }
    if artificial_count > 0 {
        let objective: Vec<_> = artificial
            .iter()
            .map(|value| if *value { -1. } else { 0. })
            .collect();
        if !run(
            &mut table,
            &mut basis,
            &objective,
            &vec![true; width - 1],
            budget,
        )? {
            return Err("Solver phase one is unstable".into());
        }
        let residual = basis
            .iter()
            .enumerate()
            .filter(|(_, b)| artificial[**b])
            .map(|(i, _)| table[i][width - 1].abs())
            .sum::<f64>();
        if residual > 1e-7 {
            return Ok(Solution::Infeasible);
        }
        for i in 0..table.len() {
            if artificial[basis[i]] {
                if let Some(column) = (0..n + slack).find(|&column| table[i][column].abs() > 1e-10)
                {
                    pivot(&mut table, &mut basis, i, column);
                }
            }
        }
    }
    let mut objective = vec![0.; width - 1];
    objective[..n].copy_from_slice(coefficients);
    let allowed: Vec<_> = artificial.iter().map(|value| !*value).collect();
    let bounded = run(&mut table, &mut basis, &objective, &allowed, budget)?;
    let mut values = vec![0.; n];
    for (i, &column) in basis.iter().enumerate() {
        if column < n {
            values[column] = table[i][width - 1];
        }
    }
    let value: f64 = coefficients.iter().zip(&values).map(|(a, b)| a * b).sum();
    if !value.is_finite() || values.iter().any(|v| !v.is_finite()) {
        return Err("Solver encountered unstable linear values".into());
    }
    Ok(if bounded {
        Solution::Optimal(values, value)
    } else {
        Solution::Unbounded(values)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_phase_simplex_matches_mac_textbook_example_and_signed_equality_rows() {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel);
        let rows = vec![
            Row {
                coefficients: vec![1., 0.],
                op: SolverOp::Le,
                rhs: 4.,
            },
            Row {
                coefficients: vec![0., 2.],
                op: SolverOp::Le,
                rhs: 12.,
            },
            Row {
                coefficients: vec![3., 2.],
                op: SolverOp::Le,
                rhs: 18.,
            },
        ];
        let Solution::Optimal(values, value) = maximize(&[3., 5.], &rows, &mut budget).unwrap()
        else {
            panic!("feasible LP")
        };
        assert!((value - 36.).abs() < 1e-8);
        assert!((values[0] - 2.).abs() < 1e-8);
        assert!((values[1] - 6.).abs() < 1e-8);
        let rows = vec![
            Row {
                coefficients: vec![1., 1.],
                op: SolverOp::Eq,
                rhs: 5.,
            },
            Row {
                coefficients: vec![-1., 0.],
                op: SolverOp::Le,
                rhs: -2.,
            },
        ];
        let Solution::Optimal(values, value) = maximize(&[0., 1.], &rows, &mut budget).unwrap()
        else {
            panic!("phase one")
        };
        assert!((value - 3.).abs() < 1e-8);
        assert!((values[0] - 2.).abs() < 1e-8);
    }
    #[test]
    fn infeasibility_unboundedness_and_cancellation_are_distinct() {
        let cancel = AtomicBool::new(false);
        let mut budget = Budget::new(&cancel);
        let rows = vec![
            Row {
                coefficients: vec![1.],
                op: SolverOp::Le,
                rhs: 1.,
            },
            Row {
                coefficients: vec![1.],
                op: SolverOp::Ge,
                rhs: 2.,
            },
        ];
        assert!(matches!(
            maximize(&[1.], &rows, &mut budget).unwrap(),
            Solution::Infeasible
        ));
        assert!(matches!(
            maximize(&[1.], &[], &mut budget).unwrap(),
            Solution::Unbounded(_)
        ));
        cancel.store(true, Ordering::Relaxed);
        assert!(maximize(&[1.], &[], &mut budget).is_err());
    }
}
