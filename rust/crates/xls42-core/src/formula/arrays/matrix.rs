use super::*;
fn numbers(m: &Grid, strict: bool) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    for (i, v) in m.values.iter().enumerate() {
        match v {
            Value::Number(n) if !m.blanks[i] => out.push(*n),
            Value::Error(e) => return Err(e),
            _ if strict => return Err("#VALUE!"),
            _ => {}
        }
    }
    Ok(out)
}
pub(super) fn call(name: &str, x: &Args<'_>, steps: &mut usize) -> Option<Result<Value>> {
    if !matches!(
        name,
        "MMULT" | "MDETERM" | "MINVERSE" | "FREQUENCY" | "MODE.MULT"
    ) {
        return None;
    }
    Some((|| {
        if name == "MODE.MULT" {
            x.count(1, 255)?;
            let mut counts = std::collections::HashMap::<u64, (f64, usize)>::new();
            for i in 0..x.0.len() {
                for n in numbers(&x.grid(i)?, false)? {
                    charge(steps, 1)?;
                    let key = if n == 0.0 { 0 } else { n.to_bits() };
                    let e = counts.entry(key).or_insert((n, 0));
                    e.1 += 1;
                }
            }
            let max = counts.values().map(|(_, count)| *count).max().unwrap_or(0);
            if max < 2 {
                return Err("#N/A");
            }
            let mut values: Vec<_> = counts
                .into_values()
                .filter(|(_, n)| *n == max)
                .map(|(v, _)| v)
                .collect();
            values.sort_by(f64::total_cmp);
            return Ok(Grid::new(
                values.len(),
                1,
                values.into_iter().map(Value::Number).collect(),
            )?
            .value());
        }
        x.count(
            if matches!(name, "MMULT" | "FREQUENCY") {
                2
            } else {
                1
            },
            if matches!(name, "MMULT" | "FREQUENCY") {
                2
            } else {
                1
            },
        )?;
        let a = x.grid(0)?;
        if name == "FREQUENCY" {
            let data = numbers(&a, false)?;
            let given = numbers(&x.grid(1)?, false)?;
            let mut bins = given.clone();
            bins.sort_by(f64::total_cmp);
            if bins.len() >= MAX_CELLS {
                return Err("#LIMIT!");
            }
            let mut counts = vec![0.0; bins.len() + 1];
            charge(
                steps,
                data.len()
                    .saturating_mul((usize::BITS - bins.len().max(1).leading_zeros()) as usize),
            )?;
            for n in data {
                let i = bins.partition_point(|b| *b < n);
                counts[i] += 1.0;
            }
            let mut seen = std::collections::HashSet::new();
            let mut values: Vec<_> = given
                .iter()
                .map(|n| {
                    let key = if *n == 0.0 { 0 } else { n.to_bits() };
                    Value::Number(if seen.insert(key) {
                        counts[bins.partition_point(|b| *b < *n)]
                    } else {
                        0.0
                    })
                })
                .collect();
            values.push(Value::Number(counts[bins.len()]));
            return Ok(Grid::new(values.len(), 1, values)?.value());
        }
        let mut av = numbers(&a, true)?;
        if name == "MMULT" {
            let b = x.grid(1)?;
            let bv = numbers(&b, true)?;
            if a.cols != b.rows {
                return Err("#VALUE!");
            }
            let count = size(a.rows, b.cols)?;
            charge(steps, count.checked_mul(a.cols).ok_or("#LIMIT!")?)?;
            let mut values = Vec::with_capacity(count);
            for r in 0..a.rows {
                for c in 0..b.cols {
                    let mut sum = 0.0;
                    let mut correction = 0.0;
                    for k in 0..a.cols {
                        let term = av[r * a.cols + k] * bv[k * b.cols + c] - correction;
                        let next = sum + term;
                        correction = (next - sum) - term;
                        sum = next;
                    }
                    values.push(finite(sum));
                }
            }
            return Ok(Grid::new(a.rows, b.cols, values)?.value());
        }
        if a.rows != a.cols {
            return Err("#VALUE!");
        }
        let n = a.rows;
        charge(
            steps,
            n.checked_mul(n)
                .and_then(|v| v.checked_mul(n))
                .and_then(|v| v.checked_mul(3))
                .ok_or("#LIMIT!")?,
        )?;
        let mut determinant = 1.0;
        let mut inverse: Vec<_> = (0..n * n).map(|i| f64::from(i / n == i % n)).collect();
        for c in 0..n {
            let p = (c..n)
                .max_by(|a, b| av[a * n + c].abs().total_cmp(&av[b * n + c].abs()))
                .unwrap();
            let pivot = av[p * n + c];
            if pivot == 0.0 {
                return if name == "MDETERM" {
                    Ok(Value::Number(0.0))
                } else {
                    Err("#NUM!")
                };
            }
            if p != c {
                for k in 0..n {
                    av.swap(p * n + k, c * n + k);
                    inverse.swap(p * n + k, c * n + k);
                }
                determinant = -determinant;
            }
            determinant *= pivot;
            if name == "MDETERM" {
                for r in c + 1..n {
                    let factor = av[r * n + c] / pivot;
                    for k in c + 1..n {
                        av[r * n + k] -= factor * av[c * n + k];
                    }
                }
            } else {
                for k in 0..n {
                    av[c * n + k] /= pivot;
                    inverse[c * n + k] /= pivot;
                }
                for r in 0..n {
                    if r == c {
                        continue;
                    }
                    let factor = av[r * n + c];
                    if factor == 0.0 {
                        continue;
                    }
                    for k in 0..n {
                        av[r * n + k] -= factor * av[c * n + k];
                        inverse[r * n + k] -= factor * inverse[c * n + k];
                    }
                }
            }
        }
        if name == "MDETERM" {
            Ok(finite(determinant))
        } else if inverse.iter().any(|n| !n.is_finite()) {
            Err("#NUM!")
        } else {
            Ok(Grid::new(n, n, inverse.into_iter().map(Value::Number).collect())?.value())
        }
    })())
}
