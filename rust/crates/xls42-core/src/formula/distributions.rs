//! Real-valued distributions, sharing the evaluator work budget. Algorithms follow the macOS engine.
use super::{finite, statistics::number, text::Arg, Value, MAX_STEPS};
use std::f64::consts::PI;
type Result<T> = std::result::Result<T, &'static str>;
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "NORM.DIST"
            | "NORMDIST"
            | "NORM.S.DIST"
            | "NORMSDIST"
            | "PHI"
            | "GAUSS"
            | "NORM.INV"
            | "NORMINV"
            | "NORM.S.INV"
            | "NORMSINV"
            | "LOGNORM.DIST"
            | "LOGNORMDIST"
            | "LOGNORM.INV"
            | "LOGINV"
            | "CONFIDENCE"
            | "CONFIDENCE.NORM"
            | "CONFIDENCE.T"
            | "T.DIST"
            | "T.DIST.2T"
            | "T.DIST.RT"
            | "TDIST"
            | "T.INV"
            | "T.INV.2T"
            | "TINV"
            | "CHISQ.DIST"
            | "CHISQ.DIST.RT"
            | "CHIDIST"
            | "CHISQ.INV"
            | "CHISQ.INV.RT"
            | "CHIINV"
            | "F.DIST"
            | "F.DIST.RT"
            | "FDIST"
            | "F.INV"
            | "F.INV.RT"
            | "FINV"
            | "GAMMA.DIST"
            | "GAMMADIST"
            | "GAMMA.INV"
            | "GAMMAINV"
            | "BETA.DIST"
            | "BETADIST"
            | "BETA.INV"
            | "BETAINV"
            | "WEIBULL.DIST"
            | "WEIBULL"
            | "EXPON.DIST"
            | "EXPONDIST"
            | "BINOM.DIST"
            | "BINOMDIST"
            | "BINOM.INV"
            | "CRITBINOM"
            | "POISSON.DIST"
            | "POISSON"
            | "FISHER"
            | "FISHERINV"
            | "GAMMA"
            | "GAMMALN"
            | "GAMMALN.PRECISE"
            | "ERF"
            | "ERF.PRECISE"
            | "ERFC"
            | "ERFC.PRECISE"
    )
}
struct Budget<'a>(&'a mut usize);
impl Budget<'_> {
    fn use_steps(&mut self, n: usize) -> Result<()> {
        *self.0 = self
            .0
            .checked_add(n)
            .filter(|v| *v <= MAX_STEPS)
            .ok_or("#LIMIT!")?;
        Ok(())
    }
}
fn checked(x: f64) -> Result<f64> {
    if x.is_finite() {
        Ok(x)
    } else {
        Err("#NUM!")
    }
}
fn probability(p: f64, open: bool) -> Result<()> {
    if (0.0..=1.0).contains(&p) && (!open || (p > 0.0 && p < 1.0)) {
        Ok(())
    } else {
        Err("#NUM!")
    }
}
fn boolean(arg: Option<&Arg>, default: bool) -> Result<bool> {
    match arg {
        None | Some(Arg::Missing) => Ok(default),
        Some(arg) => match arg.scalar()? {
            Value::Boolean(b) => Ok(b),
            Value::Number(n) => Ok(n != 0.0),
            Value::Text(s) if s.eq_ignore_ascii_case("TRUE") => Ok(true),
            Value::Text(s) if s.eq_ignore_ascii_case("FALSE") => Ok(false),
            _ => number(Some(arg), None).map(|n| n != 0.0),
        },
    }
}
fn norm_cdf(z: f64) -> f64 {
    0.5 * libm::erfc(-z / 2.0f64.sqrt())
}
fn norm_pdf(z: f64) -> f64 {
    (-z * z / 2.0).exp() / (2.0 * PI).sqrt()
}
fn poly(x: f64, coefficients: &[f64]) -> f64 {
    coefficients.iter().fold(0.0, |a, c| a * x + c)
}
/// Wichura AS 241, with the same coefficient sets as macOS.
fn norm_inv(p: f64) -> f64 {
    let q = p - 0.5;
    if q.abs() <= 0.425 {
        let r = 0.180625 - q * q;
        return q * poly(
            r,
            &[
                2509.0809287301226727,
                33430.575583588128105,
                67265.770927008700853,
                45921.953931549871457,
                13731.693765509461125,
                1971.5909503065514427,
                133.14166789178437745,
                3.387132872796366608,
            ],
        ) / poly(
            r,
            &[
                5226.495278852545925,
                28729.085735721942674,
                39307.89580009271061,
                21213.794301586595867,
                5394.1960214247511077,
                687.1870074920579083,
                42.313330701600911252,
                1.0,
            ],
        );
    }
    let mut r = (-(if q < 0.0 { p } else { 1.0 - p }).ln()).sqrt();
    let v = if r <= 5.0 {
        r -= 1.6;
        poly(
            r,
            &[
                7.7454501427834140764e-4,
                0.0227238449892691845833,
                0.24178072517745061177,
                1.27045825245236838258,
                3.64784832476320460504,
                5.7694972214606914055,
                4.6303378461565452959,
                1.42343711074968357734,
            ],
        ) / poly(
            r,
            &[
                1.05075007164441684324e-9,
                5.475938084995344946e-4,
                0.0151986665636164571966,
                0.14810397642748007459,
                0.68976733498510000455,
                1.6763848301838038494,
                2.05319162663775882187,
                1.0,
            ],
        )
    } else {
        r -= 5.0;
        poly(
            r,
            &[
                2.01033439929228813265e-7,
                2.71155556874348757815e-5,
                0.0012426609473880784386,
                0.026532189526576123093,
                0.29656057182850489123,
                1.7848265399172913358,
                5.4637849111641143699,
                6.6579046435011037772,
            ],
        ) / poly(
            r,
            &[
                2.04426310338993978564e-15,
                1.4215117583164458887e-7,
                1.8463183175100546818e-5,
                7.868691311456132591e-4,
                0.0148753612908506148525,
                0.13692988092273580531,
                0.59983220655588793769,
                1.0,
            ],
        )
    };
    if q < 0.0 {
        -v
    } else {
        v
    }
}
fn gamma_p(a: f64, x: f64, budget: &mut Budget<'_>) -> Result<f64> {
    if x <= 0.0 {
        return Ok(0.0);
    }
    if x < a + 1.0 {
        let mut sum = 1.0 / a;
        let mut term = sum;
        let mut n = a;
        for _ in 0..10000 {
            budget.use_steps(1)?;
            n += 1.0;
            term *= x / n;
            sum += term;
            if term.abs() < sum.abs() * 1e-16 {
                return checked(sum * (-x + a * x.ln() - libm::lgamma(a)).exp());
            }
        }
        return Err("#NUM!");
    }
    Ok(1.0 - gamma_q(a, x, budget)?)
}
fn gamma_q(a: f64, x: f64, budget: &mut Budget<'_>) -> Result<f64> {
    if x <= 0.0 {
        return Ok(1.0);
    }
    if x < a + 1.0 {
        return Ok(1.0 - gamma_p(a, x, budget)?);
    }
    let tiny = 1e-300;
    let mut b = x + 1.0 - a;
    let mut c = 1.0 / tiny;
    let mut d = 1.0 / b;
    let mut h = d;
    for i in 1..=10000 {
        budget.use_steps(1)?;
        let i = i as f64;
        let an = -i * (i - a);
        b += 2.0;
        d = an * d + b;
        if d.abs() < tiny {
            d = tiny;
        }
        c = b + an / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            return checked((-x + a * x.ln() - libm::lgamma(a)).exp() * h);
        }
    }
    Err("#NUM!")
}
fn beta_cf(x: f64, a: f64, b: f64, budget: &mut Budget<'_>) -> Result<f64> {
    let tiny = 1e-300;
    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;
    let mut c = 1.0;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;
    for m in 1..=10000 {
        budget.use_steps(2)?;
        let m = m as f64;
        let m2 = 2.0 * m;
        let mut aa = m * (b - m) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;
        aa = -(a + m) * (qab + m) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let delta = d * c;
        h *= delta;
        if (delta - 1.0).abs() < 1e-16 {
            return checked(h);
        }
    }
    Err("#NUM!")
}
fn beta_i(x: f64, a: f64, b: f64, budget: &mut Budget<'_>) -> Result<f64> {
    if x <= 0.0 {
        return Ok(0.0);
    }
    if x >= 1.0 {
        return Ok(1.0);
    }
    let bt = checked(
        (libm::lgamma(a + b) - libm::lgamma(a) - libm::lgamma(b) + a * x.ln() + b * (-x).ln_1p())
            .exp(),
    )?;
    checked(if x < (a + 1.0) / (a + b + 2.0) {
        bt * beta_cf(x, a, b, budget)? / a
    } else {
        1.0 - bt * beta_cf(1.0 - x, b, a, budget)? / b
    })
}
fn t_tail(t: f64, df: f64, budget: &mut Budget<'_>) -> Result<f64> {
    let tail = 0.5 * beta_i(df / (df + t * t), df / 2.0, 0.5, budget)?;
    Ok(if t >= 0.0 { tail } else { 1.0 - tail })
}
fn t_cdf(t: f64, df: f64, budget: &mut Budget<'_>) -> Result<f64> {
    let tail = 0.5 * beta_i(df / (df + t * t), df / 2.0, 0.5, budget)?;
    Ok(if t >= 0.0 { 1.0 - tail } else { tail })
}
fn t_pdf(t: f64, df: f64) -> f64 {
    (libm::lgamma((df + 1.0) / 2.0)
        - libm::lgamma(df / 2.0)
        - 0.5 * (df * PI).ln()
        - (df + 1.0) / 2.0 * (t * t / df).ln_1p())
    .exp()
}
fn gamma_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x == 0.0 {
        return if a == 1.0 {
            1.0 / b
        } else if a < 1.0 {
            f64::INFINITY
        } else {
            0.0
        };
    }
    ((a - 1.0) * x.ln() - x / b - libm::lgamma(a) - a * b.ln()).exp()
}
fn beta_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 || x >= 1.0 {
        return 0.0;
    }
    ((a - 1.0) * x.ln() + (b - 1.0) * (-x).ln_1p() - libm::lgamma(a) - libm::lgamma(b)
        + libm::lgamma(a + b))
    .exp()
}
fn f_cdf(x: f64, a: f64, b: f64, budget: &mut Budget<'_>) -> Result<f64> {
    if x <= 0.0 {
        return Ok(0.0);
    }
    let ratio = (a / b) * x;
    beta_i(
        if ratio.is_infinite() {
            1.0
        } else {
            ratio / (1.0 + ratio)
        },
        a / 2.0,
        b / 2.0,
        budget,
    )
}
fn f_tail(x: f64, a: f64, b: f64, budget: &mut Budget<'_>) -> Result<f64> {
    if x <= 0.0 {
        return Ok(1.0);
    }
    let ratio = (b / a) / x;
    beta_i(
        if ratio.is_infinite() {
            1.0
        } else {
            ratio / (1.0 + ratio)
        },
        b / 2.0,
        a / 2.0,
        budget,
    )
}
fn f_pdf(x: f64, a: f64, b: f64) -> f64 {
    if x == 0.0 {
        return if a == 2.0 {
            1.0
        } else if a < 2.0 {
            f64::INFINITY
        } else {
            0.0
        };
    }
    let ln_beta = libm::lgamma(a / 2.0) + libm::lgamma(b / 2.0) - libm::lgamma((a + b) / 2.0);
    ((a / 2.0) * (a / b).ln() + (a / 2.0 - 1.0) * x.ln()
        - (a + b) / 2.0 * (a * x / b).ln_1p()
        - ln_beta)
        .exp()
}
fn invert(
    p: f64,
    mut lo: f64,
    mut hi: f64,
    budget: &mut Budget<'_>,
    mut cdf: impl FnMut(f64, &mut Budget<'_>) -> Result<f64>,
) -> Result<f64> {
    let mut ceiling = cdf(hi, budget)?;
    let mut count = 0;
    while ceiling < p && count < 200 {
        budget.use_steps(1)?;
        lo = hi;
        hi = checked(hi * 2.0)?;
        ceiling = cdf(hi, budget)?;
        count += 1;
    }
    if ceiling < p {
        return Err("#NUM!");
    }
    for _ in 0..300 {
        budget.use_steps(1)?;
        let mid = lo + (hi - lo) / 2.0;
        if mid == lo || mid == hi {
            break;
        }
        if cdf(mid, budget)? < p {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo <= 1e-15 * mid.abs().max(1.0) {
            break;
        }
    }
    checked(lo + (hi - lo) / 2.0)
}
fn invert_tail(
    p: f64,
    mut lo: f64,
    mut hi: f64,
    budget: &mut Budget<'_>,
    mut tail: impl FnMut(f64, &mut Budget<'_>) -> Result<f64>,
) -> Result<f64> {
    if p <= 0.0 {
        return Err("#NUM!");
    }
    let mut ceiling = tail(hi, budget)?;
    let mut count = 0;
    while ceiling > p && count < 200 {
        budget.use_steps(1)?;
        lo = hi;
        hi = checked(hi * 2.0)?;
        ceiling = tail(hi, budget)?;
        count += 1;
    }
    if ceiling > p {
        return Err("#NUM!");
    }
    for _ in 0..300 {
        budget.use_steps(1)?;
        let mid = lo + (hi - lo) / 2.0;
        if mid == lo || mid == hi {
            break;
        }
        if tail(mid, budget)? > p {
            lo = mid;
        } else {
            hi = mid;
        }
        if hi - lo <= 1e-15 * mid.abs().max(1.0) {
            break;
        }
    }
    checked(lo + (hi - lo) / 2.0)
}
fn binom_pmf(k: f64, n: f64, p: f64) -> f64 {
    if p == 0.0 {
        return if k == 0.0 { 1.0 } else { 0.0 };
    }
    if p == 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    (libm::lgamma(n + 1.0) - libm::lgamma(k + 1.0) - libm::lgamma(n - k + 1.0)
        + k * p.ln()
        + (n - k) * (-p).ln_1p())
    .exp()
}
fn poisson_pmf(k: f64, mean: f64) -> f64 {
    if mean == 0.0 {
        return if k == 0.0 { 1.0 } else { 0.0 };
    }
    (-mean + k * mean.ln() - libm::lgamma(k + 1.0)).exp()
}
fn calculate(name: &str, args: &[Arg], budget: &mut Budget<'_>) -> Result<f64> {
    budget.use_steps(1)?;
    let (low, high) = match name {
        "NORM.DIST" | "LOGNORM.DIST" | "WEIBULL" | "WEIBULL.DIST" | "F.DIST" => (3, 4),
        "NORMDIST" | "BINOM.DIST" | "BINOMDIST" | "GAMMA.DIST" | "GAMMADIST" => (4, 4),
        "NORM.S.DIST" | "ERF" => (1, 2),
        "NORM.INV" | "NORMINV" | "LOGNORMDIST" | "LOGNORM.INV" | "LOGINV" | "CONFIDENCE"
        | "CONFIDENCE.NORM" | "CONFIDENCE.T" | "T.DIST" | "TDIST" | "CHISQ.DIST" | "F.DIST.RT"
        | "FDIST" | "F.INV" | "F.INV.RT" | "FINV" | "GAMMA.INV" | "GAMMAINV" | "EXPON.DIST"
        | "EXPONDIST" | "BINOM.INV" | "CRITBINOM" | "POISSON.DIST" | "POISSON" => (3, 3),
        "BETA.DIST" => (3, 6),
        "BETADIST" | "BETA.INV" | "BETAINV" => (3, 5),
        "T.DIST.2T" | "T.DIST.RT" | "T.INV" | "T.INV.2T" | "TINV" | "CHISQ.DIST.RT" | "CHIDIST"
        | "CHISQ.INV" | "CHISQ.INV.RT" | "CHIINV" => (2, 2),
        _ => (1, 1),
    };
    if !(low..=high).contains(&args.len()) {
        return Err("#VALUE!");
    }
    let n = |i| number(args.get(i), None);
    let default = |i, d| number(args.get(i), Some(d));
    let b = |i, d| boolean(args.get(i), d);
    match name {
        "NORM.DIST" | "NORMDIST" => {
            let x = n(0)?;
            let m = n(1)?;
            let s = n(2)?;
            if s <= 0.0 {
                return Err("#NUM!");
            }
            let z = (x - m) / s;
            Ok(if b(3, false)? {
                norm_cdf(z)
            } else {
                norm_pdf(z) / s
            })
        }
        "NORM.S.DIST" => Ok(if b(1, true)? {
            norm_cdf(n(0)?)
        } else {
            norm_pdf(n(0)?)
        }),
        "NORMSDIST" => Ok(norm_cdf(n(0)?)),
        "PHI" => Ok(norm_pdf(n(0)?)),
        "GAUSS" => Ok(norm_cdf(n(0)?) - 0.5),
        "NORM.INV" | "NORMINV" => {
            let p = n(0)?;
            let m = n(1)?;
            let s = n(2)?;
            probability(p, true)?;
            if s <= 0.0 {
                return Err("#NUM!");
            }
            Ok(m + s * norm_inv(p))
        }
        "NORM.S.INV" | "NORMSINV" => {
            let p = n(0)?;
            probability(p, true)?;
            Ok(norm_inv(p))
        }
        "LOGNORM.DIST" | "LOGNORMDIST" => {
            let x = n(0)?;
            let m = n(1)?;
            let s = n(2)?;
            if x <= 0.0 || s <= 0.0 {
                return Err("#NUM!");
            }
            let z = (x.ln() - m) / s;
            Ok(if name == "LOGNORMDIST" || b(3, false)? {
                norm_cdf(z)
            } else {
                norm_pdf(z) / (s * x)
            })
        }
        "LOGNORM.INV" | "LOGINV" => {
            let p = n(0)?;
            let m = n(1)?;
            let s = n(2)?;
            probability(p, true)?;
            if s <= 0.0 {
                return Err("#NUM!");
            }
            Ok((m + s * norm_inv(p)).exp())
        }
        "CONFIDENCE" | "CONFIDENCE.NORM" | "CONFIDENCE.T" => {
            let alpha = n(0)?;
            let s = n(1)?;
            let count = n(2)?.trunc();
            probability(alpha, true)?;
            if s <= 0.0 || count < 1.0 {
                return Err("#NUM!");
            }
            let factor = if name == "CONFIDENCE.T" {
                if count < 2.0 {
                    return Err("#DIV/0!");
                }
                invert_tail(alpha / 2.0, 0.0, 10.0, budget, |v, b| {
                    t_tail(v, count - 1.0, b)
                })?
            } else {
                -norm_inv(alpha / 2.0)
            };
            Ok(factor * s / count.sqrt())
        }
        "T.DIST" => {
            let t = n(0)?;
            let df = n(1)?.trunc();
            if df < 1.0 {
                return Err("#NUM!");
            }
            if b(2, false)? {
                t_cdf(t, df, budget)
            } else {
                Ok(t_pdf(t, df))
            }
        }
        "T.DIST.2T" | "T.DIST.RT" | "TDIST" => {
            let t = n(0)?;
            let df = n(1)?.trunc();
            let tails = if name == "TDIST" {
                n(2)?.trunc()
            } else if name == "T.DIST.2T" {
                2.0
            } else {
                1.0
            };
            if df < 1.0 || (name != "T.DIST.RT" && t < 0.0) || !matches!(tails, 1.0 | 2.0) {
                return Err("#NUM!");
            }
            Ok(tails * t_tail(t, df, budget)?)
        }
        "T.INV" | "T.INV.2T" | "TINV" => {
            let p = n(0)?;
            let df = n(1)?.trunc();
            if df < 1.0 {
                return Err("#NUM!");
            }
            if name == "T.INV" {
                probability(p, true)?;
                if p == 0.5 {
                    return Ok(0.0);
                }
                let q = if p > 0.5 { p } else { 1.0 - p };
                let t = invert(q, 0.0, 10.0, budget, |v, b| t_cdf(v, df, b))?;
                Ok(if p > 0.5 { t } else { -t })
            } else {
                if p <= 0.0 || p > 1.0 {
                    return Err("#NUM!");
                }
                if p == 1.0 {
                    return Ok(0.0);
                }
                invert_tail(p / 2.0, 0.0, 10.0, budget, |v, b| t_tail(v, df, b))
            }
        }
        "CHISQ.DIST" | "CHISQ.DIST.RT" | "CHIDIST" => {
            let x = n(0)?;
            let df = n(1)?.trunc();
            if x < 0.0 || df < 1.0 {
                return Err("#NUM!");
            }
            if name != "CHISQ.DIST" {
                gamma_q(df / 2.0, x / 2.0, budget)
            } else if b(2, false)? {
                gamma_p(df / 2.0, x / 2.0, budget)
            } else {
                Ok(gamma_pdf(x, df / 2.0, 2.0))
            }
        }
        "CHISQ.INV" | "CHISQ.INV.RT" | "CHIINV" => {
            let p = n(0)?;
            let df = n(1)?.trunc();
            probability(p, false)?;
            if df < 1.0 {
                return Err("#NUM!");
            }
            if name != "CHISQ.INV" {
                if p == 1.0 {
                    return Ok(0.0);
                }
                return invert_tail(p, 0.0, (df * 4.0).max(10.0), budget, |v, b| {
                    gamma_q(df / 2.0, v / 2.0, b)
                });
            }
            if p == 0.0 {
                return Ok(0.0);
            }
            if p == 1.0 {
                return Err("#NUM!");
            }
            invert(p, 0.0, (df * 4.0).max(10.0), budget, |v, b| {
                gamma_p(df / 2.0, v / 2.0, b)
            })
        }
        "F.DIST" | "F.DIST.RT" | "FDIST" => {
            let x = n(0)?;
            let a = n(1)?.trunc();
            let c = n(2)?.trunc();
            if x < 0.0 || a < 1.0 || c < 1.0 {
                return Err("#NUM!");
            }
            if name != "F.DIST" {
                f_tail(x, a, c, budget)
            } else if b(3, false)? {
                f_cdf(x, a, c, budget)
            } else {
                Ok(f_pdf(x, a, c))
            }
        }
        "F.INV" | "F.INV.RT" | "FINV" => {
            let p = n(0)?;
            let a = n(1)?.trunc();
            let c = n(2)?.trunc();
            probability(p, false)?;
            if a < 1.0 || c < 1.0 {
                return Err("#NUM!");
            }
            if name != "F.INV" {
                if p == 1.0 {
                    return Ok(0.0);
                }
                return invert_tail(p, 0.0, 10.0, budget, |v, b| f_tail(v, a, c, b));
            }
            if p == 0.0 {
                return Ok(0.0);
            }
            if p == 1.0 {
                return Err("#NUM!");
            }
            invert(p, 0.0, 10.0, budget, |v, b| f_cdf(v, a, c, b))
        }
        "GAMMA.DIST" | "GAMMADIST" => {
            let x = n(0)?;
            let a = n(1)?;
            let scale = n(2)?;
            if x < 0.0 || a <= 0.0 || scale <= 0.0 {
                return Err("#NUM!");
            }
            if b(3, false)? {
                gamma_p(a, x / scale, budget)
            } else {
                Ok(gamma_pdf(x, a, scale))
            }
        }
        "GAMMA.INV" | "GAMMAINV" => {
            let p = n(0)?;
            let a = n(1)?;
            let scale = n(2)?;
            probability(p, false)?;
            if a <= 0.0 || scale <= 0.0 || p >= 1.0 {
                return Err("#NUM!");
            }
            if p == 0.0 {
                return Ok(0.0);
            }
            invert(p, 0.0, (a * scale * 4.0).max(10.0), budget, |v, b| {
                gamma_p(a, v / scale, b)
            })
        }
        "BETA.DIST" | "BETADIST" => {
            let x = n(0)?;
            let a = n(1)?;
            let c = n(2)?;
            let legacy = name == "BETADIST";
            let lo = default(if legacy { 3 } else { 4 }, 0.0)?;
            let hi = default(if legacy { 4 } else { 5 }, 1.0)?;
            if a <= 0.0 || c <= 0.0 || lo >= hi || x < lo || x > hi {
                return Err("#NUM!");
            }
            let z = (x - lo) / (hi - lo);
            if legacy || b(3, false)? {
                beta_i(z, a, c, budget)
            } else {
                Ok(beta_pdf(z, a, c) / (hi - lo))
            }
        }
        "BETA.INV" | "BETAINV" => {
            let p = n(0)?;
            let a = n(1)?;
            let c = n(2)?;
            let lo = default(3, 0.0)?;
            let hi = default(4, 1.0)?;
            probability(p, false)?;
            if a <= 0.0 || c <= 0.0 || lo >= hi {
                return Err("#NUM!");
            }
            if p == 0.0 {
                return Ok(lo);
            }
            if p == 1.0 {
                return Ok(hi);
            }
            Ok(lo + invert(p, 0.0, 1.0, budget, |v, b| beta_i(v, a, c, b))? * (hi - lo))
        }
        "WEIBULL.DIST" | "WEIBULL" => {
            let x = n(0)?;
            let a = n(1)?;
            let scale = n(2)?;
            if x < 0.0 || a <= 0.0 || scale <= 0.0 {
                return Err("#NUM!");
            }
            let exponent = -(x / scale).powf(a);
            Ok(if b(3, false)? {
                -exponent.exp_m1()
            } else {
                a / scale.powf(a) * x.powf(a - 1.0) * exponent.exp()
            })
        }
        "EXPON.DIST" | "EXPONDIST" => {
            let x = n(0)?;
            let lambda = n(1)?;
            if x < 0.0 || lambda <= 0.0 {
                return Err("#NUM!");
            }
            Ok(if b(2, false)? {
                -(-lambda * x).exp_m1()
            } else {
                lambda * (-lambda * x).exp()
            })
        }
        "BINOM.DIST" | "BINOMDIST" => {
            let k = n(0)?.trunc();
            let trials = n(1)?.trunc();
            let p = n(2)?;
            probability(p, false)?;
            if k < 0.0 || k > trials || trials < 0.0 {
                return Err("#NUM!");
            }
            if b(3, false)? {
                if k > 1_000_000.0 {
                    return Err("#NUM!");
                }
                let mut sum = 0.0;
                for i in 0..=k as usize {
                    budget.use_steps(4)?;
                    sum += binom_pmf(i as f64, trials, p);
                }
                Ok(sum.min(1.0))
            } else {
                Ok(binom_pmf(k, trials, p))
            }
        }
        "BINOM.INV" | "CRITBINOM" => {
            let trials = n(0)?.trunc();
            let p = n(1)?;
            let alpha = n(2)?;
            probability(p, false)?;
            probability(alpha, false)?;
            if !(0.0..=1_000_000.0).contains(&trials) {
                return Err("#NUM!");
            }
            if alpha == 0.0 {
                return Ok(0.0);
            }
            let mut sum = 0.0;
            for k in 0..=trials as usize {
                budget.use_steps(4)?;
                sum += binom_pmf(k as f64, trials, p);
                if sum >= alpha * (1.0 - 1e-14) {
                    return Ok(k as f64);
                }
            }
            Ok(trials)
        }
        "POISSON.DIST" | "POISSON" => {
            let k = n(0)?.trunc();
            let mean = n(1)?;
            if k < 0.0 || mean < 0.0 {
                return Err("#NUM!");
            }
            if b(2, false)? {
                if k > 1_000_000.0 {
                    return Err("#NUM!");
                }
                let mut sum = 0.0;
                for i in 0..=k as usize {
                    budget.use_steps(2)?;
                    sum += poisson_pmf(i as f64, mean);
                }
                Ok(sum.min(1.0))
            } else {
                Ok(poisson_pmf(k, mean))
            }
        }
        "FISHER" => {
            let x = n(0)?;
            if x <= -1.0 || x >= 1.0 {
                return Err("#NUM!");
            }
            Ok(x.atanh())
        }
        "FISHERINV" => Ok(n(0)?.tanh()),
        "GAMMA" => {
            let x = n(0)?;
            if x <= 0.0 && x == x.round() {
                return Err("#NUM!");
            }
            Ok(libm::tgamma(x))
        }
        "GAMMALN" | "GAMMALN.PRECISE" => {
            let x = n(0)?;
            if x <= 0.0 {
                return Err("#NUM!");
            }
            Ok(libm::lgamma(x))
        }
        "ERF" => Ok(if args.len() == 2 {
            {
                let lo = n(0)?;
                let hi = n(1)?;
                if lo > 0.0 && hi > 0.0 {
                    libm::erfc(lo) - libm::erfc(hi)
                } else if lo < 0.0 && hi < 0.0 {
                    libm::erfc(-hi) - libm::erfc(-lo)
                } else {
                    libm::erf(hi) - libm::erf(lo)
                }
            }
        } else {
            libm::erf(n(0)?)
        }),
        "ERF.PRECISE" => Ok(libm::erf(n(0)?)),
        "ERFC" | "ERFC.PRECISE" => Ok(libm::erfc(n(0)?)),
        _ => Err("#NAME?"),
    }
}
pub(super) fn call(name: &str, args: &[Arg], steps: &mut usize) -> Value {
    match calculate(name, args, &mut Budget(steps)) {
        Ok(n) => finite(n),
        Err(e) => Value::Error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    #[test]
    fn independent_high_precision_cases_cover_tails_and_quantiles() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!(
            "../../fixtures/independent-distribution-cases.json"
        ))
        .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() > 200);
        let mut book = Workbook::new(cases.len(), 2).unwrap();
        for (i, case) in cases.iter().enumerate() {
            book.set_cell(i, 0, case["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = book.display_grid();
        for (i, case) in cases.iter().enumerate() {
            let actual = grid[i][0]
                .parse::<f64>()
                .unwrap_or_else(|_| panic!("{}: {}", case["formula"], grid[i][0]));
            let expected = case["expected"].as_f64().unwrap();
            assert!(
                if expected == 0.0 {
                    actual.abs() < 1e-12
                } else {
                    (actual / expected - 1.0).abs() < 2e-9
                },
                "{}: expected {expected}, actual {actual}",
                case["formula"]
            );
        }
    }
    #[test]
    fn domains_missing_arguments_and_the_global_work_budget_are_enforced() {
        let mut book = Workbook::new(20, 2).unwrap();
        for (i, (formula, expected)) in [
            ("=NORM.S.INV(0)", "#NUM!"),
            ("=NORM.DIST(1,0,0,TRUE)", "#NUM!"),
            ("=BETA.DIST(2,2,3,TRUE)", "#NUM!"),
            ("=T.INV(0.5,0)", "#NUM!"),
            ("=CHISQ.INV.RT(0,2)", "#NUM!"),
            ("=GAMMA(-2)", "#NUM!"),
            ("=GAMMALN(0)", "#NUM!"),
            ("=POISSON(1,-2,FALSE)", "#NUM!"),
            ("=BINOM.DIST(3,2,0.5,FALSE)", "#NUM!"),
            ("=CONFIDENCE.T(0.05,1,1)", "#DIV/0!"),
            ("=NORM.INV(0.5,0)", "#VALUE!"),
            ("=ERF()", "#VALUE!"),
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(i, 0, formula.into()).unwrap();
            assert_eq!(book.display_grid()[i][0], expected, "{formula}");
        }
        let args = [
            Arg::Scalar(Value::Number(10.0)),
            Arg::Scalar(Value::Number(20.0)),
            Arg::Scalar(Value::Number(0.5)),
            Arg::Scalar(Value::Boolean(true)),
        ];
        let mut steps = MAX_STEPS - 2;
        assert_eq!(
            call("BINOM.DIST", &args, &mut steps),
            Value::Error("#LIMIT!")
        );
        assert!(steps <= MAX_STEPS);
    }
}
