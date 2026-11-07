//! Centered, scaled least squares. Householder QR avoids squaring the condition number.
use super::*;
fn numeric(m: &Grid) -> Result<Vec<f64>> {
    m.values
        .iter()
        .enumerate()
        .map(|(i, v)| match v {
            Value::Number(n) if !m.blanks[i] => Ok(*n),
            Value::Error(e) => Err(*e),
            _ => Err("#VALUE!"),
        })
        .collect()
}
fn sum(values: impl Iterator<Item = f64>) -> f64 {
    let (mut sum, mut correction) = (0.0, 0.0);
    for value in values {
        let v = value - correction;
        let next = sum + v;
        correction = (next - sum) - v;
        sum = next;
    }
    sum
}
struct Fit {
    slopes: Vec<f64>,
    intercept: f64,
    covariance: Vec<f64>,
    intercept_variance: f64,
}
fn fit(y: &[f64], xs: &[Vec<f64>], constant: bool, steps: &mut usize) -> Result<Fit> {
    let n = y.len();
    let k = xs.first().map_or(0, Vec::len);
    if k == 0 || n < k + usize::from(constant) {
        return Err("#NUM!");
    }
    charge(
        steps,
        n.checked_mul(k)
            .and_then(|v| v.checked_mul(k))
            .and_then(|v| v.checked_mul(8))
            .ok_or("#LIMIT!")?,
    )?;
    let mean_y = if constant {
        sum(y.iter().map(|v| v / n as f64))
    } else {
        0.0
    };
    let means: Vec<_> = (0..k)
        .map(|c| {
            if constant {
                sum(xs.iter().map(|r| r[c] / n as f64))
            } else {
                0.0
            }
        })
        .collect();
    let scales: Vec<_> = (0..k)
        .map(|c| xs.iter().fold(0.0_f64, |v, r| v.hypot(r[c] - means[c])))
        .collect();
    if scales.iter().any(|s| *s == 0.0 || !s.is_finite()) {
        return Err("#NUM!");
    }
    let mut a: Vec<_> = xs
        .iter()
        .flat_map(|r| (0..k).map(|c| (r[c] - means[c]) / scales[c]))
        .collect();
    let mut b: Vec<_> = y.iter().map(|v| v - mean_y).collect();
    for c in 0..k {
        let norm = (c..n).fold(0.0_f64, |v, r| v.hypot(a[r * k + c]));
        if !norm.is_finite() || norm <= f64::EPSILON * (n.max(k) as f64) * 8.0 {
            return Err("#NUM!");
        }
        let alpha = -norm.copysign(a[c * k + c]);
        let mut v: Vec<_> = (c..n).map(|r| a[r * k + c]).collect();
        v[0] -= alpha;
        let vn = v.iter().fold(0.0_f64, |n, v| n.hypot(*v));
        for value in &mut v {
            *value /= vn;
        }
        for col in c..k {
            let dot = 2.0 * sum(v.iter().enumerate().map(|(r, v)| v * a[(r + c) * k + col]));
            for (r, v) in v.iter().enumerate() {
                a[(r + c) * k + col] -= dot * v;
            }
        }
        let dot = 2.0 * sum(v.iter().enumerate().map(|(r, v)| v * b[r + c]));
        for (r, v) in v.iter().enumerate() {
            b[r + c] -= dot * v;
        }
        a[c * k + c] = alpha;
    }
    let mut beta = vec![0.0; k];
    for r in (0..k).rev() {
        beta[r] = (b[r] - sum((r + 1..k).map(|c| a[r * k + c] * beta[c]))) / a[r * k + r];
    }
    let slopes: Vec<_> = beta.iter().zip(&scales).map(|(b, s)| b / s).collect();
    let intercept = mean_y - sum(means.iter().zip(&slopes).map(|(m, b)| m * b));
    let mut inv = vec![0.0; k * k];
    for c in 0..k {
        for r in (0..k).rev() {
            inv[r * k + c] = (f64::from(r == c)
                - sum((r + 1..k).map(|j| a[r * k + j] * inv[j * k + c])))
                / a[r * k + r];
        }
    }
    let mut covariance = vec![0.0; k * k];
    for r in 0..k {
        for c in 0..k {
            covariance[r * k + c] =
                sum((0..k).map(|j| inv[r * k + j] * inv[c * k + j])) / scales[r] / scales[c];
        }
    }
    let intercept_variance = if constant {
        1.0 / n as f64
            + sum((0..k)
                .flat_map(|r| (0..k).map(move |c| (r, c)))
                .map(|(r, c)| means[r] * covariance[r * k + c] * means[c]))
    } else {
        0.0
    };
    if slopes
        .iter()
        .chain([&intercept, &intercept_variance])
        .chain(covariance.iter())
        .any(|v| !v.is_finite())
    {
        return Err("#NUM!");
    }
    Ok(Fit {
        slopes,
        intercept,
        covariance,
        intercept_variance,
    })
}
pub(super) fn calculate(name: &str, x: &Args<'_>, steps: &mut usize) -> Result<Value> {
    x.count(1, 4)?;
    let ym = x.grid(0)?;
    let mut y = numeric(&ym)?;
    let logarithmic = matches!(name, "LOGEST" | "GROWTH");
    if logarithmic {
        for v in &mut y {
            if *v <= 0.0 {
                return Err("#NUM!");
            }
            *v = v.ln();
        }
    }
    let n = y.len();
    let by_rows = ym.rows == 1 && ym.cols > 1;
    let xs: Vec<Vec<f64>> = if x.has(1) {
        let m = x.grid(1)?;
        let v = numeric(&m)?;
        if v.len() == n {
            v.into_iter().map(|v| vec![v]).collect()
        } else if ym.cols == 1 && m.rows == n {
            v.chunks(m.cols).map(<[f64]>::to_vec).collect()
        } else if ym.rows == 1 && m.cols == n {
            (0..n)
                .map(|c| (0..m.rows).map(|r| v[r * m.cols + c]).collect())
                .collect()
        } else {
            return Err("#REF!");
        }
    } else {
        (1..=n).map(|i| vec![i as f64]).collect()
    };
    let regression = matches!(name, "LINEST" | "LOGEST");
    let constant = x.boolean(if regression { 2 } else { 3 }, true)?;
    let fit = fit(&y, &xs, constant, steps)?;
    let k = fit.slopes.len();
    if !regression {
        let (mut rows, mut cols) = (if by_rows { 1 } else { n }, if by_rows { n } else { 1 });
        let new_x = if x.has(2) {
            let m = x.grid(2)?;
            let v = numeric(&m)?;
            if k == 1 {
                rows = m.rows;
                cols = m.cols;
                v.into_iter().map(|v| vec![v]).collect()
            } else if !by_rows && m.cols == k {
                rows = m.rows;
                cols = 1;
                v.chunks(k).map(<[f64]>::to_vec).collect()
            } else if by_rows && m.rows == k {
                rows = 1;
                cols = m.cols;
                (0..m.cols)
                    .map(|c| (0..k).map(|r| v[r * m.cols + c]).collect())
                    .collect()
            } else {
                return Err("#REF!");
            }
        } else {
            xs
        };
        charge(steps, new_x.len().saturating_mul(k))?;
        let values = new_x
            .iter()
            .map(|row| {
                let v = sum(row.iter().zip(&fit.slopes).map(|(x, b)| x * b)) + fit.intercept;
                finite(if logarithmic { v.exp() } else { v })
            })
            .collect();
        return Ok(Grid::new(rows, cols, values)?.value());
    }
    let stats = x.boolean(3, false)?;
    let cols = k + 1;
    size(if stats { 5 } else { 1 }, cols)?;
    let transform = |v: f64| finite(if logarithmic { v.exp() } else { v });
    let mut values: Vec<_> = fit.slopes.iter().rev().copied().map(transform).collect();
    values.push(transform(fit.intercept));
    if stats {
        let residual = sum(xs.iter().zip(&y).map(|(row, y)| {
            let e = y - fit.intercept - sum(row.iter().zip(&fit.slopes).map(|(x, b)| x * b));
            e * e
        }));
        let mean = if constant {
            sum(y.iter().map(|v| v / n as f64))
        } else {
            0.0
        };
        let total = sum(y.iter().map(|y| (y - mean).powi(2)));
        let explained = (total - residual).max(0.0);
        let df = n - k - usize::from(constant);
        let variance = if df > 0 { residual / df as f64 } else { 0.0 };
        let sey = variance.sqrt();
        values.extend(
            (0..k)
                .rev()
                .map(|c| finite((fit.covariance[c * k + c].max(0.0) * variance).sqrt())),
        );
        values.push(if constant {
            finite((fit.intercept_variance.max(0.0) * variance).sqrt())
        } else {
            Value::Error("#N/A")
        });
        for row in [
            [if total > 0.0 { explained / total } else { 1.0 }, sey],
            [
                if df > 0 && residual > 0.0 {
                    (explained / k as f64) / variance
                } else {
                    0.0
                },
                df as f64,
            ],
            [explained, residual],
        ] {
            values.extend(row.into_iter().map(finite));
            values.extend(std::iter::repeat_n(Value::Error("#N/A"), cols - 2));
        }
    }
    Ok(Grid::new(if stats { 5 } else { 1 }, cols, values)?.value())
}
#[cfg(test)]
mod tests {
    use crate::Workbook;
    #[test]
    fn regression_shapes_errors_and_centered_large_offsets() {
        let mut b = Workbook::new(20, 12).unwrap();
        for (r, n) in [1., 2., 3., 4., 5.].into_iter().enumerate() {
            b.set_cell(r, 0, format!("{}", 1e10 + n)).unwrap();
            b.set_cell(r, 1, format!("{}", 3. * n + 7.)).unwrap();
        }
        for (r, f, expected) in [
            (7, "=INDEX(LINEST(B1:B5,A1:A5),1)", 3.),
            (8, "=TREND(B1:B5,A1:A5,10000000006)", 25.),
            (9, "=INDEX(LINEST(B1:B5,A1:A5,TRUE,TRUE),3,1)", 1.),
            (10, "=INDEX(LOGEST({2;4;8},{1;2;3}),1)", 2.),
        ] {
            b.set_cell(r, 0, f.into()).unwrap();
            let actual = crate::formula::display_cell(&b, r, 0)
                .parse::<f64>()
                .unwrap();
            assert!((actual - expected).abs() < 1e-6, "{f}: {actual}");
        }
        for (f, e) in [
            ("=LINEST({1;2},{1,2;2,4})", "#NUM!"),
            ("=GROWTH({0;1},{1;2})", "#NUM!"),
            ("=TREND({1;2;3},{1;2})", "#REF!"),
            ("=LINEST({1;2;3},{1;1;1})", "#NUM!"),
        ] {
            b.set_cell(15, 0, f.into()).unwrap();
            assert_eq!(crate::formula::display_cell(&b, 15, 0), e, "{f}");
        }
        b.set_cell(15, 0, "=LINEST({2,4,6},{1,2,3},FALSE,TRUE)".into())
            .unwrap();
        let g = b.display_grid();
        assert!((g[15][0].parse::<f64>().unwrap() - 2.0).abs() < 1e-12);
        assert_eq!(g[15][1], "0");
        assert_eq!(g[16][1], "#N/A");
    }
}
