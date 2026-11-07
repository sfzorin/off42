//! Bounded real-valued math. Domain and overflow failures remain formula errors.
use super::{finite, Value};
type Result<T> = std::result::Result<T, &'static str>;
const EXACT_INTEGER: f64 = (1u64 << 53) as f64;
pub(super) fn aggregate(name: &str) -> bool {
    matches!(name, "GCD" | "LCM" | "SUMSQ" | "MULTINOMIAL")
}
pub(super) fn handles(name: &str) -> bool {
    aggregate(name)
        || matches!(
            name,
            "RAND"
                | "RANDBETWEEN"
                | "ASIN"
                | "ACOS"
                | "ATAN"
                | "ATAN2"
                | "SINH"
                | "COSH"
                | "TANH"
                | "ASINH"
                | "ACOSH"
                | "ATANH"
                | "COT"
                | "CSC"
                | "COTH"
                | "CSCH"
                | "SEC"
                | "SECH"
                | "ACOT"
                | "ACOTH"
                | "SQRTPI"
                | "FACTDOUBLE"
                | "COMBIN"
                | "COMBINA"
                | "PERMUT"
                | "PERMUTATIONA"
                | "QUOTIENT"
                | "CEILING.PRECISE"
                | "ISO.CEILING"
                | "FLOOR.PRECISE"
                | "CEILING.MATH"
                | "FLOOR.MATH"
                | "MROUND"
                | "EVEN"
                | "ODD"
                | "TRUNC"
        )
}
fn integer(n: f64) -> Result<u64> {
    if n < 0. || n >= EXACT_INTEGER {
        Err("#NUM!")
    } else {
        Ok(n.trunc() as u64)
    }
}
fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}
fn combin(n: u64, k: u64) -> Result<f64> {
    if k > n {
        return Err("#NUM!");
    }
    let k = k.min(n - k);
    // n >= 2k, so choose(n,k) >= 2^k; larger loops cannot produce a finite f64.
    if k > 1023 {
        return Err("#NUM!");
    }
    let mut result = 1.;
    for i in 1..=k {
        result *= (n - k + i) as f64 / i as f64;
        if !result.is_finite() {
            return Err("#NUM!");
        }
    }
    Ok(result.round())
}
fn calculate(name: &str, values: &[f64], count: usize) -> Result<f64> {
    let need = |low, high| {
        if (low..=high).contains(&count) {
            Ok(())
        } else {
            Err("#VALUE!")
        }
    };
    if name == "RAND" {
        need(0, 0)?;
        return Ok(fastrand::f64());
    }
    if name == "RANDBETWEEN" {
        need(2, 2)?;
        let (lo, hi) = (values[0].ceil(), values[1].floor());
        if lo > hi || lo.abs() >= EXACT_INTEGER || hi.abs() >= EXACT_INTEGER {
            return Err("#NUM!");
        }
        return Ok(fastrand::i64(lo as i64..=hi as i64) as f64);
    }
    if aggregate(name) {
        need(1, 255)?;
        return match name {
            "SUMSQ" => Ok(values.iter().map(|v| v * v).sum()),
            "GCD" => values
                .iter()
                .try_fold(0, |acc, &n| Ok(gcd(acc, integer(n)?)))
                .map(|n| n as f64),
            "LCM" => {
                let numbers = values
                    .iter()
                    .map(|n| integer(*n))
                    .collect::<Result<Vec<_>>>()?;
                if numbers.contains(&0) {
                    return Ok(0.);
                }
                numbers
                    .into_iter()
                    .try_fold(1u64, |acc, n| {
                        (acc / gcd(acc, n))
                            .checked_mul(n)
                            .filter(|v| *v < 1u64 << 53)
                            .ok_or("#NUM!")
                    })
                    .map(|n| n as f64)
            }
            "MULTINOMIAL" => {
                let mut total = 0u64;
                let mut result = 1.;
                for &n in values {
                    let n = integer(n)?;
                    total = total
                        .checked_add(n)
                        .filter(|v| *v < 1u64 << 53)
                        .ok_or("#NUM!")?;
                    result *= combin(total, n)?;
                    if !result.is_finite() {
                        return Err("#NUM!");
                    }
                }
                Ok(result.round())
            }
            _ => unreachable!(),
        };
    }
    if matches!(name, "COMBIN" | "COMBINA" | "PERMUT" | "PERMUTATIONA") {
        need(2, 2)?;
        let n = integer(values[0])?;
        let k = integer(values[1])?;
        return match name {
            "COMBIN" => combin(n, k),
            "COMBINA" if k == 0 => Ok(1.),
            "COMBINA" if n == 0 => Err("#NUM!"),
            "COMBINA" => combin(
                n.checked_add(k - 1)
                    .filter(|v| *v < 1u64 << 53)
                    .ok_or("#NUM!")?,
                k,
            ),
            "PERMUT" => {
                if k > n || k > 170 {
                    return Err("#NUM!");
                }
                let mut product = 1.;
                for i in 0..k {
                    product *= (n - i) as f64;
                }
                Ok(product)
            }
            _ => Ok((n as f64).powf(k as f64)),
        };
    }
    if matches!(
        name,
        "CEILING.PRECISE" | "FLOOR.PRECISE" | "ISO.CEILING" | "CEILING.MATH" | "FLOOR.MATH"
    ) {
        need(1, if name.ends_with("MATH") { 3 } else { 2 })?;
        let n = values[0];
        let significance = values.get(1).copied().unwrap_or(1.).abs();
        let mode = values.get(2).copied().unwrap_or(0.);
        if significance == 0. {
            return Ok(0.);
        }
        let q = n / significance;
        if !q.is_finite() {
            return Ok(n);
        }
        let up = if name.starts_with("FLOOR") {
            n < 0. && mode != 0.
        } else {
            !(n < 0. && mode != 0.)
        };
        return Ok((if up { q.ceil() } else { q.floor() }) * significance);
    }
    if matches!(name, "ATAN2" | "QUOTIENT" | "MROUND") {
        need(2, 2)?;
        let (a, b) = (values[0], values[1]);
        return match name {
            "ATAN2" if a == 0. && b == 0. => Err("#DIV/0!"),
            "ATAN2" => {
                let angle = b.atan2(a);
                Ok(if angle == -std::f64::consts::PI {
                    std::f64::consts::PI
                } else {
                    angle
                })
            }
            "QUOTIENT" if b == 0. => Err("#DIV/0!"),
            "QUOTIENT" => Ok((a / b).trunc()),
            "MROUND" if b == 0. || a == 0. => Ok(0.),
            "MROUND" if a.is_sign_negative() != b.is_sign_negative() => Err("#NUM!"),
            _ => {
                let q = a / b;
                Ok(if q.is_finite() { q.round() * b } else { a })
            }
        };
    }
    if name == "TRUNC" {
        need(1, 2)?;
        let n = values[0];
        let digits = values.get(1).copied().unwrap_or(0.).trunc();
        if digits > 308. {
            return Ok(n);
        }
        if digits < -308. {
            return Ok(0.);
        }
        if digits >= 0. {
            let scale = 10f64.powi(digits as i32);
            let scaled = n * scale;
            return Ok(if scaled.is_finite() {
                scaled.trunc() / scale
            } else {
                n
            });
        }
        let scale = 10f64.powi(-digits as i32);
        return Ok((n / scale).trunc() * scale);
    }
    need(1, 1)?;
    let n = values[0];
    Ok(match name {
        "ASIN" if n.abs() <= 1. => n.asin(),
        "ACOS" if n.abs() <= 1. => n.acos(),
        "ASIN" | "ACOS" => return Err("#NUM!"),
        "ATAN" => n.atan(),
        "SINH" => n.sinh(),
        "COSH" => n.cosh(),
        "TANH" => n.tanh(),
        "ASINH" => n.asinh(),
        "ACOSH" if n >= 1. => n.acosh(),
        "ACOSH" => return Err("#NUM!"),
        "ATANH" if n.abs() < 1. => n.atanh(),
        "ATANH" => return Err("#NUM!"),
        "ACOT" => std::f64::consts::FRAC_PI_2 - n.atan(),
        "ACOTH" if n.abs() > 1. => (1. / n).atanh(),
        "ACOTH" => return Err("#NUM!"),
        "COT" | "CSC" | "COTH" | "CSCH" if n == 0. => return Err("#DIV/0!"),
        "COT" => 1. / n.tan(),
        "CSC" => 1. / n.sin(),
        "COTH" => 1. / n.tanh(),
        "CSCH" => 1. / n.sinh(),
        "SEC" => 1. / n.cos(),
        "SECH" => 1. / n.cosh(),
        "SQRTPI" if n >= 0. => n.sqrt() * std::f64::consts::PI.sqrt(),
        "SQRTPI" => return Err("#NUM!"),
        "FACTDOUBLE" => {
            if n < 0. || n.trunc() > 300. {
                return Err("#NUM!");
            }
            let mut i = n.trunc() as u32;
            let mut product = 1.;
            while i > 1 {
                product *= i as f64;
                i -= 2;
            }
            product
        }
        "EVEN" => {
            let result = (n.abs() / 2.).ceil() * 2.;
            if n < 0. {
                -result
            } else {
                result
            }
        }
        "ODD" => {
            let mut result = n.abs().ceil();
            if result % 2. == 0. {
                result += 1.;
            }
            if n < 0. {
                -result
            } else {
                result
            }
        }
        _ => return Err("#NAME?"),
    })
}
pub(super) fn call(name: &str, values: &[f64], count: usize) -> Value {
    match calculate(name, values, count) {
        Ok(n) => finite(n),
        Err(error) => Value::Error(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    #[test]
    fn every_supported_math_reference_case_matches_its_expected_result() {
        let data: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/macos-function-cases.json")).unwrap();
        let cases: Vec<_> = data["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| {
                let formula = case["formula"].as_str().unwrap();
                handles(formula.trim_start_matches('=').split('(').next().unwrap())
            })
            .collect();
        assert!(cases.len() >= 40);
        let mut book = Workbook::new(cases.len().max(10), 24).unwrap();
        for (i, n) in [3, 7, 2, 9, 4, 7, 1, 8, 5, 6].into_iter().enumerate() {
            book.set_cell(i, 10, n.to_string()).unwrap();
            book.set_cell(i, 14, (i + 1).to_string()).unwrap();
        }
        for (i, case) in cases.iter().enumerate() {
            book.set_cell(i, 0, case["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = book.display_grid();
        for (i, case) in cases.into_iter().enumerate() {
            let expected = case["expected"].as_str().unwrap();
            let actual = &grid[i][0];
            if let (Ok(a), Ok(b)) = (actual.parse::<f64>(), expected.parse::<f64>()) {
                assert!(
                    (a - b).abs() <= 1e-8 * b.abs().max(1.),
                    "{}: expected {b}, actual {a}",
                    case["formula"]
                );
            } else {
                assert_eq!(actual, expected, "{}", case["formula"]);
            }
        }
    }
    #[test]
    fn domains_arities_overflow_and_large_finite_values_are_distinct() {
        let cases = [
            ("=ASIN(1.1)", "#NUM!"),
            ("=ACOS(-1.1)", "#NUM!"),
            ("=ACOSH(0.5)", "#NUM!"),
            ("=ATANH(1)", "#NUM!"),
            ("=ACOTH(-1)", "#NUM!"),
            ("=COT(0)", "#DIV/0!"),
            ("=COTH(0)", "#DIV/0!"),
            ("=CSCH(0)", "#DIV/0!"),
            ("=CSC(0)", "#DIV/0!"),
            ("=ATAN2(0,0)", "#DIV/0!"),
            ("=QUOTIENT(1,0)", "#DIV/0!"),
            ("=ATAN(1,2)", "#VALUE!"),
            ("=ATAN()", "#VALUE!"),
            ("=GCD()", "#VALUE!"),
            ("=SINH(1000)", "#NUM!"),
            ("=SECH(1000)", "0"),
            ("=SQRTPI(-1)", "#NUM!"),
            ("=FACTDOUBLE(-1)", "#NUM!"),
            ("=FACTDOUBLE(301)", "#NUM!"),
            ("=FACTDOUBLE(0)", "1"),
            ("=COMBIN(3,4)", "#NUM!"),
            ("=COMBIN(0,0)", "1"),
            ("=COMBINA(0,2)", "#NUM!"),
            ("=COMBINA(0,0)", "1"),
            ("=COMBIN(1000000000,1)", "1000000000"),
            ("=PERMUT(171,171)", "#NUM!"),
            ("=PERMUTATIONA(0,3)", "0"),
            ("=GCD(9007199254740992,2)", "#NUM!"),
            ("=LCM(9007199254740991,2)", "#NUM!"),
            ("=GCD(8.9,12.9)", "4"),
            ("=LCM(0,8)", "0"),
            ("=MULTINOMIAL(-1,2)", "#NUM!"),
            ("=MROUND(-10,3)", "#NUM!"),
            ("=MROUND(10,0)", "0"),
            ("=MROUND(-10,-3)", "-9"),
            ("=CEILING.MATH(-5.5,2,1)", "-6"),
            ("=FLOOR.MATH(-5.5,2,1)", "-4"),
            ("=ISO.CEILING(-4.3,-2)", "-4"),
            ("=FLOOR.PRECISE(2,0)", "0"),
            ("=EVEN(-1)", "-2"),
            ("=ODD(-2)", "-3"),
            ("=ODD(0)", "1"),
            ("=TRUNC(3.14159,3)", "3.141"),
            ("=TRUNC(-1234.56,-2)", "-1200"),
            ("=TRUNC(1.25,999)", "1.25"),
            ("=TRUNC(1.25,-999)", "0"),
            ("=COMBIN(1/0,2)", "#DIV/0!"),
        ];
        let mut book = Workbook::new(cases.len(), 2).unwrap();
        for (i, (formula, _)) in cases.iter().enumerate() {
            book.set_cell(i, 0, formula.to_string()).unwrap();
        }
        let grid = book.display_grid();
        for (i, (formula, expected)) in cases.into_iter().enumerate() {
            assert_eq!(grid[i][0], expected, "{formula}");
        }
        for (name, values, expected) in [
            ("SQRTPI", vec![1e308], 1.772453850905516e154),
            ("COMBIN", vec![1024., 512.], 4.481254552098972e306),
            ("MULTINOMIAL", vec![100., 100.], 9.054851465610328e58),
            ("ACOTH", vec![1e100], 1e-100),
        ] {
            let Value::Number(actual) = call(name, &values, values.len()) else {
                panic!("{name} rejected finite result")
            };
            assert!(
                (actual / expected - 1.).abs() < 1e-12,
                "{name}: {actual} != {expected}"
            );
        }
    }
    #[test]
    fn aggregates_keep_range_filtering_and_independent_office_formulas_recalculate() {
        let mut book = Workbook::new(20, 8).unwrap();
        for (i, value) in ["6", "text", "true", "", "8"].into_iter().enumerate() {
            book.set_cell(i, 0, value.into()).unwrap();
        }
        for (i, formula) in [
            "=GCD(A1:A5)",
            "=LCM(A1:A5)",
            "=SUMSQ(A1:A5)",
            "=MULTINOMIAL(A1,A5)",
            "=TRUNC(-8.95,1)",
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(i, 1, formula.into()).unwrap();
        }
        let before = book.display_grid();
        assert_eq!(
            (
                before[0][1].as_str(),
                before[1][1].as_str(),
                before[2][1].as_str(),
                before[3][1].as_str()
            ),
            ("2", "24", "100", "3003")
        );
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" {
                book.xlsx_bytes()
            } else {
                book.ods_bytes()
            }
            .unwrap();
            let mut source = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for i in 0..source.len() {
                let entry = source.by_index(i).unwrap();
                if entry.name() != "xls42/project.json" {
                    output.raw_copy_file(entry).unwrap();
                }
            }
            let mut external =
                Workbook::from_office_bytes(&output.finish().unwrap().into_inner(), kind).unwrap();
            assert_eq!(external.display_grid(), before);
            external.set_cell(0, 0, "10".into()).unwrap();
            let after = external.display_grid();
            assert_eq!(
                (
                    after[0][1].as_str(),
                    after[1][1].as_str(),
                    after[2][1].as_str()
                ),
                ("2", "40", "164")
            );
        }
    }
}

#[cfg(test)]
mod random_bounds_tests {
    use super::*;
    #[test]
    fn random_values_stay_finite_and_integer_bounds_are_inclusive() {
        for _ in 0..200 {
            let Value::Number(v) = call("RAND", &[], 0) else {
                panic!()
            };
            assert!((0.0..1.0).contains(&v));
            let Value::Number(v) = call("RANDBETWEEN", &[-2.3, 2.8], 2) else {
                panic!()
            };
            assert!((-2.0..=2.0).contains(&v));
            assert_eq!(v.fract(), 0.0);
        }
        assert_eq!(call("RANDBETWEEN", &[2.1, 2.8], 2), Value::Error("#NUM!"));
        assert_eq!(call("RAND", &[1.0], 1), Value::Error("#VALUE!"));
    }
}
