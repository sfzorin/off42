//! Bounded cash-flow, annuity and depreciation calculations.
use super::{finite, statistics::number, text::Arg, Value, MAX_STEPS};
type Result<T> = std::result::Result<T, &'static str>;
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "PMT"
            | "PV"
            | "FV"
            | "NPER"
            | "IPMT"
            | "PPMT"
            | "ISPMT"
            | "CUMIPMT"
            | "CUMPRINC"
            | "RATE"
            | "NPV"
            | "XNPV"
            | "IRR"
            | "XIRR"
            | "MIRR"
            | "SLN"
            | "SYD"
            | "DB"
            | "DDB"
            | "EFFECT"
            | "NOMINAL"
            | "FVSCHEDULE"
            | "PDURATION"
            | "RRI"
    )
}
struct Budget<'a>(&'a mut usize);
impl Budget<'_> {
    fn charge(&mut self, n: usize) -> Result<()> {
        *self.0 = self
            .0
            .checked_add(n)
            .filter(|v| *v <= MAX_STEPS)
            .ok_or("#LIMIT!")?;
        Ok(())
    }
}
fn checked(v: f64) -> Result<f64> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err("#NUM!")
    }
}
fn growth(rate: f64, n: f64) -> (f64, f64) {
    if rate > -1.0 {
        let t = n * rate.ln_1p();
        (t.exp(), t.exp_m1())
    } else {
        let g = (1.0 + rate).powf(n);
        (g, g - 1.0)
    }
}
fn payment(rate: f64, n: f64, pv: f64, fv: f64, due: f64) -> Result<f64> {
    if n == 0.0 {
        return Err("#DIV/0!");
    }
    if rate == 0.0 {
        return checked(-(pv + fv) / n);
    }
    let (g, delta) = growth(rate, n);
    let den = (1.0 + rate * due) * delta;
    if den == 0.0 {
        return Err("#DIV/0!");
    }
    checked(-rate * (pv * g + fv) / den)
}
fn future(rate: f64, n: f64, pmt: f64, pv: f64, due: f64) -> Result<f64> {
    if rate == 0.0 {
        return checked(-(pv + pmt * n));
    }
    let (g, delta) = growth(rate, n);
    if (n * rate).abs() < 0.5 {
        return checked(-(pv * g + pmt * (1.0 + rate * due) * delta / rate));
    }
    // Retain the small residual of two large, nearly cancelling annuity terms.
    let factor = 1.0 + rate * due;
    let adjustment = pmt * factor;
    let factor_error = (1.0 - factor) + rate * due;
    let adjustment_error = pmt.mul_add(factor, -adjustment) + pmt * factor_error;
    let residual = pv.mul_add(rate, adjustment) + adjustment_error;
    checked(-(residual.mul_add(g, -adjustment) - adjustment_error) / rate)
}
fn interest(rate: f64, per: f64, n: f64, pv: f64, fv: f64, due: f64) -> Result<f64> {
    if per < 1.0 || per > n {
        return Err("#NUM!");
    }
    if rate == 0.0 || per == 1.0 && due == 1.0 {
        return Ok(0.0);
    }
    let p = payment(rate, n, pv, fv, due)?;
    if per > 1.0 && rate > -1.0 {
        // Discount the remaining payments; rolling the rounded payment forward amplifies error.
        let exponent = -(n - per + 1.0) * rate.ln_1p();
        return checked(p * (-exponent.exp_m1()) + fv * rate * exponent.exp() / (1.0 + rate * due));
    }
    let balance = if per == 1.0 {
        -pv
    } else if due == 1.0 {
        future(rate, per - 2.0, p, pv, 1.0)? - p
    } else {
        future(rate, per - 1.0, p, pv, 0.0)?
    };
    checked(balance * rate)
}
fn numbers(arg: &Arg) -> Result<Vec<f64>> {
    match arg {
        Arg::Range(v, b) => v
            .iter()
            .zip(b)
            .filter(|(_, b)| !**b)
            .filter_map(|(v, _)| match v {
                Value::Number(n) => Some(Ok(*n)),
                Value::Error(e) => Some(Err(*e)),
                _ => None,
            })
            .collect(),
        _ => Ok(vec![number(Some(arg), None)?]),
    }
}
fn paired(values: &Arg, dates: &Arg) -> Result<(Vec<f64>, Vec<f64>)> {
    let v = numbers(values)?;
    let d = numbers(dates)?;
    let count = match values {
        Arg::Range(v, _) => v.len(),
        _ => 1,
    };
    let dates_count = match dates {
        Arg::Range(v, _) => v.len(),
        _ => 1,
    };
    if v.is_empty() || v.len() != d.len() || v.len() != count || d.len() != dates_count {
        return Err("#NUM!");
    }
    let d = d.into_iter().map(f64::floor).collect::<Vec<_>>();
    if d.iter().any(|x| *x < 0.0 || *x > 2_958_465.0 || *x < d[0]) {
        return Err("#NUM!");
    }
    Ok((v, d))
}
fn discounted(rate: f64, values: &[f64], powers: &[f64], budget: &mut Budget<'_>) -> Result<f64> {
    budget.charge(values.len())?;
    if rate <= -1.0 {
        return Err("#NUM!");
    }
    checked(
        values
            .iter()
            .zip(powers)
            .map(|(v, t)| v * (-t * rate.ln_1p()).exp())
            .sum(),
    )
}
/// Solve in log(1+rate) space, preserving the rate>-1 domain and a shared work bound.
fn solve(guess: f64, values: &[f64], powers: &[f64], budget: &mut Budget<'_>) -> Result<f64> {
    if guess <= -1.0 {
        return Err("#NUM!");
    }
    let scale = values.iter().copied().map(f64::abs).fold(0.0, f64::max);
    if scale == 0.0 {
        return Err("#NUM!");
    }
    let normalized = values.iter().map(|v| v / scale).collect::<Vec<_>>();
    let mut f = |x: f64| -> Result<(f64, f64)> {
        budget.charge(values.len())?;
        let mut y = 0.0;
        let mut d = 0.0;
        let mut magnitude = 0.0;
        for (v, t) in normalized.iter().zip(powers) {
            let z = v * (-t * x).exp();
            y += z;
            d -= t * z;
            magnitude += z.abs();
        }
        Ok((y / magnitude, d / magnitude))
    };
    let start = guess.ln_1p();
    let mut x = start;
    for _ in 0..100 {
        let (y, d) = f(x)?;
        if y.abs() < 1e-12 {
            return checked(x.exp_m1());
        }
        if !y.is_finite() || !d.is_finite() || d == 0.0 {
            break;
        }
        let next = x - (y / d).clamp(-1.0, 1.0);
        if next.abs() > 700.0 {
            break;
        }
        if (next - x).abs() < 1e-14 {
            return Err("#NUM!");
        }
        x = next;
    }
    let mut previous = None;
    // Search outward from the supplied guess; multiple roots remain guess-dependent.
    for width in [
        0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0, 64.0,
    ] {
        for (lo, hi) in [(start - width, start), (start, start + width)] {
            let (mut left, mut right) = (lo, hi);
            let (mut a, _) = f(left)?;
            let (b, _) = f(right)?;
            if !a.is_finite() || !b.is_finite() || a.signum() == b.signum() {
                continue;
            }
            for _ in 0..180 {
                let mid = (left + right) / 2.0;
                let (y, _) = f(mid)?;
                if !y.is_finite() {
                    return Err("#NUM!");
                }
                if y.abs() < 1e-12 || (right - left).abs() < 1e-14 {
                    return checked(mid.exp_m1());
                }
                if a.signum() == y.signum() {
                    left = mid;
                    a = y;
                } else {
                    right = mid;
                }
            }
            previous = Some((left + right) / 2.0);
        }
    }
    previous
        .map(|x| x.exp_m1())
        .filter(|x| x.is_finite())
        .ok_or("#NUM!")
}
pub(super) fn call(name: &str, args: &[Arg], steps: &mut usize) -> Value {
    match calculate(name, args, &mut Budget(steps)) {
        Ok(v) => finite(v),
        Err(e) => Value::Error(e),
    }
}
fn calculate(name: &str, a: &[Arg], budget: &mut Budget<'_>) -> Result<f64> {
    let (min, max) = match name {
        "PMT" | "PV" | "FV" | "NPER" => (3, 5),
        "IPMT" | "PPMT" => (4, 6),
        "RATE" => (3, 6),
        "NPV" => (2, 1024),
        "XNPV" | "MIRR" => (3, 3),
        "IRR" => (1, 2),
        "XIRR" => (2, 3),
        "CUMIPMT" | "CUMPRINC" => (6, 6),
        "DB" | "DDB" => (4, 5),
        "ISPMT" | "SYD" => (4, 4),
        "SLN" | "PDURATION" | "RRI" => (3, 3),
        _ => (2, 2),
    };
    if !(min..=max).contains(&a.len()) {
        return Err("#VALUE!");
    }
    let n = |i: usize| number(a.get(i), None);
    let opt = |i: usize, d: f64| number(a.get(i), Some(d));
    let due = |i: usize| -> Result<f64> {
        let d = opt(i, 0.0)?;
        if d == 0.0 || d == 1.0 {
            Ok(d)
        } else {
            Err("#NUM!")
        }
    };
    let result = match name {
        "PMT" => payment(n(0)?, n(1)?, n(2)?, opt(3, 0.0)?, due(4)?)?,
        "FV" => future(n(0)?, n(1)?, n(2)?, opt(3, 0.0)?, due(4)?)?,
        "PV" => {
            let (r, t, p, f, d) = (n(0)?, n(1)?, n(2)?, opt(3, 0.0)?, due(4)?);
            if r == 0.0 {
                -(f + p * t)
            } else {
                let (g, delta) = growth(r, t);
                if g == 0.0 {
                    return Err("#DIV/0!");
                }
                -(f + p * (1.0 + r * d) * delta / r) / g
            }
        }
        "NPER" => {
            let (r, p, pv, f, d) = (n(0)?, n(1)?, n(2)?, opt(3, 0.0)?, due(4)?);
            if r == 0.0 {
                if p == 0.0 {
                    return Err("#NUM!");
                }
                -(pv + f) / p
            } else {
                let ratio = (p * (1.0 + r * d) - f * r) / (p * (1.0 + r * d) + pv * r);
                if r <= -1.0 || ratio <= 0.0 {
                    return Err("#NUM!");
                }
                ratio.ln() / r.ln_1p()
            }
        }
        "IPMT" | "PPMT" => {
            let (r, per, t, pv, f, d) = (n(0)?, n(1)?, n(2)?, n(3)?, opt(4, 0.0)?, due(5)?);
            let i = interest(r, per, t, pv, f, d)?;
            if name == "IPMT" {
                i
            } else {
                payment(r, t, pv, f, d)? - i
            }
        }
        "ISPMT" => {
            let t = n(2)?;
            if t == 0.0 {
                return Err("#DIV/0!");
            }
            n(3)? * n(0)? * (n(1)? / t - 1.0)
        }
        "CUMIPMT" | "CUMPRINC" => {
            let (r, t, pv, s, e, d) = (
                n(0)?,
                n(1)?.trunc(),
                n(2)?,
                n(3)?.trunc(),
                n(4)?.trunc(),
                due(5)?,
            );
            if r <= 0.0 || t <= 0.0 || pv <= 0.0 || s < 1.0 || e < s || e > t {
                return Err("#NUM!");
            }
            if e - s >= MAX_STEPS as f64 {
                return Err("#LIMIT!");
            }
            budget.charge((e - s + 1.0) as usize)?;
            let p = payment(r, t, pv, 0.0, d)?;
            let mut sum = 0.0;
            for k in 0..(e - s + 1.0) as usize {
                let i = interest(r, s + k as f64, t, pv, 0.0, d)?;
                sum += if name == "CUMIPMT" { i } else { p - i };
            }
            sum
        }
        "RATE" => {
            let (t, p, pv, f, d, g) = (n(0)?, n(1)?, n(2)?, opt(3, 0.0)?, due(4)?, opt(5, 0.1)?);
            if t <= 0.0 {
                return Err("#NUM!");
            }
            // RATE's analytic derivative in log-rate space avoids constructing nper cash flows.
            let scale = pv.abs().max(f.abs()).max(p.abs());
            if scale == 0.0 {
                return Err("#NUM!");
            }
            let mut x = if g > -1.0 {
                g.ln_1p()
            } else {
                return Err("#NUM!");
            };
            let balance = |x: f64| {
                let r = x.exp_m1();
                if r.abs() < 1e-10 {
                    pv / scale + p / scale * t + f / scale
                } else {
                    pv / scale * (t * x).exp()
                        + p / scale * (1.0 + r * d) * (t * x).exp_m1() / r
                        + f / scale
                }
            };
            let mut root = None;
            for _ in 0..200 {
                budget.charge(3)?;
                let y = balance(x);
                if y.abs() < 1e-12 {
                    root = Some(x.exp_m1());
                    break;
                }
                let h = 1e-6;
                let deriv = (balance(x + h) - balance(x - h)) / (2.0 * h);
                if !y.is_finite() || !deriv.is_finite() || deriv == 0.0 {
                    break;
                }
                let next = x - (y / deriv).clamp(-1.0, 1.0);
                if next.abs() > 700.0 {
                    break;
                }
                x = next;
            }
            if root.is_none() {
                let start = g.ln_1p();
                'search: for width in [0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0, 16.0, 32.0] {
                    for (mut lo, mut hi) in [(start - width, start), (start, start + width)] {
                        budget.charge(2)?;
                        let mut left = balance(lo);
                        let right = balance(hi);
                        if !left.is_finite()
                            || !right.is_finite()
                            || left.signum() == right.signum()
                        {
                            continue;
                        }
                        for _ in 0..180 {
                            budget.charge(1)?;
                            let mid = (lo + hi) / 2.0;
                            let y = balance(mid);
                            if !y.is_finite() {
                                break;
                            }
                            if y.abs() < 1e-12 || (hi - lo).abs() < 1e-14 {
                                root = Some(mid.exp_m1());
                                break 'search;
                            }
                            if y.signum() == left.signum() {
                                lo = mid;
                                left = y;
                            } else {
                                hi = mid;
                            }
                        }
                    }
                }
            }
            root.ok_or("#NUM!")?
        }
        "NPV" => {
            let r = n(0)?;
            if r == -1.0 {
                return Err("#DIV/0!");
            }
            let mut v = Vec::new();
            for arg in &a[1..] {
                v.extend(numbers(arg)?);
            }
            let p = (1..=v.len()).map(|x| x as f64).collect::<Vec<_>>();
            if r < -1.0 {
                budget.charge(v.len())?;
                checked(v.iter().zip(&p).map(|(v, p)| v / (1.0 + r).powf(*p)).sum())?
            } else {
                discounted(r, &v, &p, budget)?
            }
        }
        "IRR" | "XIRR" | "XNPV" => {
            let (v, p): (Vec<f64>, Vec<f64>) = if name == "IRR" {
                let v = numbers(&a[0])?;
                let p = (0..v.len()).map(|i| i as f64).collect();
                (v, p)
            } else {
                let i = usize::from(name == "XNPV");
                let (v, d) = paired(&a[i], &a[i + 1])?;
                let p = d.iter().map(|t| (t - d[0]) / 365.0).collect();
                (v, p)
            };
            if name == "XNPV" {
                discounted(n(0)?, &v, &p, budget)?
            } else {
                if !v.iter().any(|x| *x > 0.0) || !v.iter().any(|x| *x < 0.0) {
                    return Err("#NUM!");
                }
                solve(opt(if name == "IRR" { 1 } else { 2 }, 0.1)?, &v, &p, budget)?
            }
        }
        "MIRR" => {
            let v = numbers(&a[0])?;
            let (fr, rr) = (n(1)?, n(2)?);
            if v.len() < 2 {
                return Err("#DIV/0!");
            }
            if fr <= -1.0 || rr <= -1.0 {
                return Err("#NUM!");
            }
            budget.charge(v.len())?;
            let mut pos = 0.0;
            let mut neg = 0.0;
            for (i, c) in v.iter().enumerate() {
                if *c > 0.0 {
                    pos += c * ((v.len() - 1 - i) as f64 * rr.ln_1p()).exp();
                } else {
                    neg += c * (-(i as f64) * fr.ln_1p()).exp();
                }
            }
            if neg == 0.0 || pos == 0.0 {
                return Err("#DIV/0!");
            }
            (-pos / neg).powf(1.0 / (v.len() - 1) as f64) - 1.0
        }
        "SLN" => {
            let life = n(2)?;
            if life == 0.0 {
                return Err("#DIV/0!");
            }
            (n(0)? - n(1)?) / life
        }
        "SYD" => {
            let (c, s, l, p) = (n(0)?, n(1)?, n(2)?, n(3)?);
            if l <= 0.0 || p <= 0.0 || p > l {
                return Err("#NUM!");
            }
            (c - s) * (l - p + 1.0) * 2.0 / (l * (l + 1.0))
        }
        "DB" | "DDB" => {
            let (c, s, l, p) = (n(0)?, n(1)?, n(2)?, n(3)?);
            let option = opt(4, if name == "DB" { 12.0 } else { 2.0 })?;
            let p = if name == "DB" { p.trunc() } else { p.ceil() };
            if c < 0.0
                || s < 0.0
                || l <= 0.0
                || p < 1.0
                || option <= 0.0
                || name == "DB" && (option.trunc() < 1.0 || option.trunc() > 12.0)
                || p > l + f64::from(name == "DB" && option.trunc() < 12.0)
            {
                return Err("#NUM!");
            }
            if p > MAX_STEPS as f64 {
                return Err("#LIMIT!");
            }
            budget.charge(p as usize)?;
            if c == 0.0 {
                return Ok(0.0);
            }
            let mut remaining = c;
            let mut dep = 0.0;
            let rate = if name == "DB" {
                ((1.0 - (s / c).powf(1.0 / l)) * 1000.0).round() / 1000.0
            } else {
                option / l
            };
            for k in 1..=p as usize {
                dep = if name == "DDB" {
                    (remaining * rate).min((remaining - s).max(0.0))
                } else {
                    remaining
                        * rate
                        * if k == 1 {
                            option.trunc() / 12.0
                        } else if k as f64 == l + 1.0 {
                            (12.0 - option.trunc()) / 12.0
                        } else {
                            1.0
                        }
                };
                remaining -= dep;
            }
            dep
        }
        "EFFECT" | "NOMINAL" => {
            let (r, k) = (n(0)?, n(1)?.trunc());
            if r <= 0.0 || k < 1.0 {
                return Err("#NUM!");
            }
            if name == "EFFECT" {
                (k * (r / k).ln_1p()).exp_m1()
            } else {
                k * ((r.ln_1p() / k).exp_m1())
            }
        }
        "FVSCHEDULE" => {
            let mut p = n(0)?;
            let rates = numbers(&a[1])?;
            budget.charge(rates.len())?;
            for r in rates {
                p *= 1.0 + r;
            }
            p
        }
        "PDURATION" => {
            let (r, p, f) = (n(0)?, n(1)?, n(2)?);
            if r <= 0.0 || p <= 0.0 || f <= 0.0 {
                return Err("#NUM!");
            }
            (f.ln() - p.ln()) / r.ln_1p()
        }
        "RRI" => {
            let (t, p, f) = (n(0)?, n(1)?, n(2)?);
            if t <= 0.0 || p == 0.0 || f / p < 0.0 {
                return Err("#NUM!");
            }
            if f == 0.0 {
                -1.0
            } else {
                ((f / p).ln() / t).exp_m1()
            }
        }
        _ => return Err("#NAME?"),
    };
    checked(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    fn close(actual: &str, expected: f64) {
        let v = actual
            .parse::<f64>()
            .unwrap_or_else(|_| panic!("{actual} != {expected}"));
        assert!(
            (v - expected).abs() < 1e-8 * expected.abs().max(1.0),
            "{v} != {expected}"
        );
    }
    #[test]
    fn independent_high_precision_annuity_cases() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/independent-finance-cases.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() >= 100);
        let mut b = Workbook::new(cases.len(), 2).unwrap();
        for (i, c) in cases.iter().enumerate() {
            b.set_cell(i, 0, c["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = b.display_grid();
        for (i, c) in cases.iter().enumerate() {
            let expected = c["expected"].as_f64().unwrap();
            let actual = grid[i][0]
                .parse::<f64>()
                .unwrap_or_else(|_| panic!("{}: {}", c["formula"], grid[i][0]));
            assert!(
                (actual - expected).abs() <= 1e-8 + 2e-10 * expected.abs(),
                "{}: {actual} != {expected}",
                c["formula"]
            );
        }
    }
    #[test]
    fn finance_and_calendar_reference_cases() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/macos-function-cases.json")).unwrap();
        let cases = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| {
                let f = c["formula"].as_str().unwrap();
                let name = f.trim_start_matches('=').split('(').next().unwrap();
                (handles(name) || super::super::calendar::handles(name)) && !f.contains('{')
            })
            .collect::<Vec<_>>();
        assert!(cases.len() >= 60);
        let mut b = Workbook::new(cases.len(), 24).unwrap();
        for (col, values) in [
            (15, vec!["-1000", "300", "400", "400", "300", "100"]),
            (
                16,
                vec![
                    "2024-01-01",
                    "2024-03-01",
                    "2024-10-30",
                    "2025-02-15",
                    "2025-04-01",
                    "2025-06-30",
                ],
            ),
            (23, vec!["2024-12-25", "2025-01-01", "2024-11-28"]),
        ] {
            for (i, v) in values.into_iter().enumerate() {
                b.set_cell(i, col, v.into()).unwrap();
            }
        }
        for (i, c) in cases.iter().enumerate() {
            b.set_cell(i, 0, c["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = b.display_grid();
        for (i, c) in cases.iter().enumerate() {
            let expected = c["expected"].as_str().unwrap();
            if let Ok(v) = expected.parse::<f64>() {
                close(&grid[i][0], v);
            } else {
                assert_eq!(grid[i][0], expected, "{}", c["formula"]);
            }
        }
    }
    #[test]
    fn financial_domains_small_rates_and_bounded_solver() {
        let mut b = Workbook::new(30, 6).unwrap();
        for (i, v) in ["-100", "0", "121"].iter().enumerate() {
            b.set_cell(i, 0, (*v).into()).unwrap();
        }
        for (i, (f, expected)) in [
            ("=IRR(A1:A3)", 0.1),
            ("=IRR(A1:A3,-0.8)", 0.1),
            ("=PMT(1e-14,10,100)", -10.00000000000055),
            ("=PV(1e-14,10,-10)", 99.9999999999945),
            ("=RATE(10,-10,100)", 0.0),
            ("=RATE(10,-1e-300,1e-299)", 0.0),
            ("=FV(0,10,-10,100)", 0.0),
            ("=IPMT(0,2,10,100)", 0.0),
            ("=IPMT(.1,1,10,100,0,1)", 0.0),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 1, (*f).into()).unwrap();
            close(&b.display_grid()[i][1], *expected);
        }
        for (i, (f, e)) in [
            ("=IRR(A2:A3)", "#NUM!"),
            ("=XNPV(.1,A1:A3,A1:A2)", "#NUM!"),
            ("=IPMT(.1,0,10,100)", "#NUM!"),
            ("=CUMIPMT(.1,10,100,2,1,0)", "#NUM!"),
            ("=CUMIPMT(.1,2000000,100,1,1000001,0)", "#LIMIT!"),
            ("=EFFECT(.1,0)", "#NUM!"),
            ("=DB(100,10,10,2,13)", "#NUM!"),
            ("=PMT(0,0,100)", "#DIV/0!"),
            ("=PV(.1,10,1,0,2)", "#NUM!"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 2, (*f).into()).unwrap();
            assert_eq!(b.display_grid()[i][2], *e, "{f}");
        }
        let args = [Arg::Range(
            vec![Value::Number(-100.0), Value::Number(110.0)],
            vec![false; 2],
        )];
        let mut steps = MAX_STEPS - 1;
        assert_eq!(call("IRR", &args, &mut steps), Value::Error("#LIMIT!"));
    }
}
