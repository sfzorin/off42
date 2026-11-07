//! Statistical aggregates over typed scalar/reference arguments.
use super::{finite, text::Arg, Value};
type Result<T> = std::result::Result<T, &'static str>;
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "COUNTBLANK"
            | "MEDIAN"
            | "MODE"
            | "MODE.SNGL"
            | "STDEV"
            | "STDEV.S"
            | "STDEV.P"
            | "STDEVP"
            | "VAR"
            | "VAR.S"
            | "VAR.P"
            | "VARP"
            | "STDEVA"
            | "STDEVPA"
            | "VARA"
            | "VARPA"
            | "AVERAGEA"
            | "MAXA"
            | "MINA"
            | "AVEDEV"
            | "DEVSQ"
            | "GEOMEAN"
            | "HARMEAN"
            | "KURT"
            | "SKEW"
            | "SKEW.P"
            | "STANDARDIZE"
            | "LARGE"
            | "SMALL"
            | "RANK"
            | "RANK.EQ"
            | "RANK.AVG"
            | "PERCENTILE"
            | "PERCENTILE.INC"
            | "PERCENTILE.EXC"
            | "QUARTILE"
            | "QUARTILE.INC"
            | "QUARTILE.EXC"
            | "PERCENTRANK"
            | "PERCENTRANK.INC"
            | "PERCENTRANK.EXC"
            | "CORREL"
            | "PEARSON"
            | "RSQ"
            | "SLOPE"
            | "INTERCEPT"
            | "STEYX"
            | "FORECAST"
            | "FORECAST.LINEAR"
            | "COVAR"
            | "COVARIANCE.P"
            | "COVARIANCE.S"
    )
}
fn need(args: &[Arg], low: usize, high: usize) -> Result<()> {
    if (low..=high).contains(&args.len()) {
        Ok(())
    } else {
        Err("#VALUE!")
    }
}
pub(super) fn number(arg: Option<&Arg>, default: Option<f64>) -> Result<f64> {
    let arg = match arg {
        None | Some(Arg::Missing) => return default.ok_or("#VALUE!"),
        Some(arg) => arg,
    };
    let v = match arg {
        Arg::Range(values, blanks) if values.len() == 1 && blanks[0] => Value::Number(0.0),
        _ => arg.scalar()?,
    };
    match v {
        Value::Number(n) => Ok(n),
        Value::Boolean(b) => Ok(f64::from(b)),
        Value::Text(s) => s
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or("#VALUE!"),
        Value::Error(e) => Err(e),
    }
}
fn numbers(args: &[Arg], count_text: bool) -> Result<Vec<f64>> {
    let mut out = Vec::new();
    for arg in args {
        match arg {
            Arg::Missing => {}
            Arg::Range(values, blanks) => {
                for (v, blank) in values.iter().zip(blanks) {
                    if *blank {
                        continue;
                    }
                    match v {
                        Value::Number(n) => out.push(*n),
                        Value::Error(e) => return Err(e),
                        Value::Boolean(b) if count_text => out.push(f64::from(*b)),
                        Value::Text(_) if count_text => out.push(0.0),
                        _ => {}
                    }
                }
            }
            _ => out.push(number(Some(arg), None)?),
        }
    }
    Ok(out)
}
fn mean(v: &[f64]) -> f64 {
    v.iter().map(|n| n / v.len() as f64).sum()
}
fn variance(v: &[f64], sample: bool) -> Result<f64> {
    if v.len() <= usize::from(sample) {
        return Err("#DIV/0!");
    }
    let m = mean(v);
    Ok(v.iter().map(|n| (n - m).powi(2)).sum::<f64>() / (v.len() - usize::from(sample)) as f64)
}
fn percentile(v: &[f64], k: f64, exclusive: bool) -> Result<f64> {
    if v.is_empty() || !(0.0..=1.0).contains(&k) {
        return Err("#NUM!");
    }
    let n = v.len() as f64;
    let rank = if exclusive {
        if k <= 0.0 || k >= 1.0 || k < 1.0 / (n + 1.0) || k > n / (n + 1.0) {
            return Err("#NUM!");
        }
        (n + 1.0) * k - 1.0
    } else {
        (n - 1.0) * k
    };
    let lo = rank.floor().max(0.0) as usize;
    if lo + 1 >= v.len() {
        return Ok(v[v.len() - 1]);
    }
    Ok(v[lo] + (rank - lo as f64) * (v[lo + 1] - v[lo]))
}
fn numeric_positions(arg: &Arg) -> Result<Vec<Option<f64>>> {
    let (values, blanks) = match arg {
        Arg::Range(v, b) => (v.clone(), b.clone()),
        Arg::Missing => (vec![Value::Text(String::new())], vec![true]),
        Arg::Scalar(v) => (vec![v.clone()], vec![false]),
    };
    values
        .into_iter()
        .zip(blanks)
        .map(|(v, blank)| {
            if blank {
                Ok(None)
            } else {
                match v {
                    Value::Number(n) => Ok(Some(n)),
                    Value::Error(e) => Err(e),
                    _ => Ok(None),
                }
            }
        })
        .collect()
}
fn pairs(a: &Arg, b: &Arg) -> Result<(Vec<f64>, Vec<f64>)> {
    let a = numeric_positions(a)?;
    let b = numeric_positions(b)?;
    if a.len() != b.len() {
        return Err("#N/A");
    }
    let mut x = Vec::new();
    let mut y = Vec::new();
    for (a, b) in a.into_iter().zip(b) {
        if let (Some(a), Some(b)) = (a, b) {
            x.push(a);
            y.push(b);
        }
    }
    Ok((x, y))
}
fn calculate(name: &str, args: &[Arg]) -> Result<f64> {
    if name == "COUNTBLANK" {
        need(args, 1, 1)?;
        return Ok(match &args[0] {
            Arg::Missing => 1,
            Arg::Scalar(Value::Text(s)) if s.is_empty() => 1,
            Arg::Range(v, b) => v
                .iter()
                .zip(b)
                .filter(|(v, b)| **b || matches!(v,Value::Text(s)if s.is_empty()))
                .count(),
            _ => 0,
        } as f64);
    }
    if name == "STANDARDIZE" {
        need(args, 3, 3)?;
        let x = number(args.first(), None)?;
        let m = number(args.get(1), None)?;
        let s = number(args.get(2), None)?;
        return if s <= 0.0 {
            Err("#NUM!")
        } else {
            Ok((x - m) / s)
        };
    }
    if matches!(
        name,
        "LARGE"
            | "SMALL"
            | "RANK"
            | "RANK.EQ"
            | "RANK.AVG"
            | "PERCENTILE"
            | "PERCENTILE.INC"
            | "PERCENTILE.EXC"
            | "QUARTILE"
            | "QUARTILE.INC"
            | "QUARTILE.EXC"
            | "PERCENTRANK"
            | "PERCENTRANK.INC"
            | "PERCENTRANK.EXC"
    ) {
        need(
            args,
            2,
            if name.starts_with("RANK") || name.starts_with("PERCENTRANK") {
                3
            } else {
                2
            },
        )?;
        if name.starts_with("RANK") {
            let x = number(args.first(), None)?;
            let v = numbers(&args[1..2], false)?;
            let order = number(args.get(2), Some(0.0))?;
            let equal = v.iter().filter(|n| **n == x).count();
            if equal == 0 {
                return Err("#N/A");
            }
            let better = v
                .iter()
                .filter(|n| if order != 0.0 { **n < x } else { **n > x })
                .count();
            return Ok(better as f64
                + if name == "RANK.AVG" {
                    (equal + 1) as f64 / 2.0
                } else {
                    1.0
                });
        }
        let mut v = numbers(&args[..1], false)?;
        v.sort_by(f64::total_cmp);
        let mut k = number(args.get(1), None)?;
        if matches!(name, "LARGE" | "SMALL") {
            k = k.trunc();
            if k < 1.0 || k > v.len() as f64 {
                return Err("#NUM!");
            }
            return Ok(v[if name == "LARGE" {
                v.len() - k as usize
            } else {
                k as usize - 1
            }]);
        }
        if name.starts_with("PERCENTRANK") {
            let sig = number(args.get(2), Some(3.0))?.trunc();
            if v.is_empty() || !(1.0..=308.0).contains(&sig) {
                return Err("#NUM!");
            }
            if k < v[0] || k > v[v.len() - 1] {
                return Err("#N/A");
            }
            let less = v.partition_point(|n| *n < k);
            let n = v.len() as f64;
            let exc = name.ends_with("EXC");
            let rank = if v.contains(&k) {
                if v.len() == 1 {
                    1.0
                } else if exc {
                    (less + 1) as f64 / (n + 1.0)
                } else {
                    less as f64 / (n - 1.0)
                }
            } else {
                let fraction = (k - v[less - 1]) / (v[less] - v[less - 1]);
                if exc {
                    (less as f64 + fraction) / (n + 1.0)
                } else {
                    (less as f64 - 1.0 + fraction) / (n - 1.0)
                }
            };
            let factor = 10.0f64.powf(sig);
            return Ok((rank * factor + 1e-9).floor() / factor);
        }
        if name.starts_with("QUARTILE") {
            k = k.trunc();
            if if name.ends_with("EXC") {
                !(1.0..=3.0).contains(&k)
            } else {
                !(0.0..=4.0).contains(&k)
            } {
                return Err("#NUM!");
            }
            k /= 4.0;
        }
        return percentile(&v, k, name.ends_with("EXC"));
    }
    if matches!(
        name,
        "CORREL"
            | "PEARSON"
            | "RSQ"
            | "SLOPE"
            | "INTERCEPT"
            | "STEYX"
            | "FORECAST"
            | "FORECAST.LINEAR"
            | "COVAR"
            | "COVARIANCE.P"
            | "COVARIANCE.S"
    ) {
        let forecast = name.starts_with("FORECAST");
        need(
            args,
            if forecast { 3 } else { 2 },
            if forecast { 3 } else { 2 },
        )?;
        let offset = usize::from(forecast);
        let (a, b) = pairs(&args[offset], &args[offset + 1])?;
        let sample = name == "COVARIANCE.S";
        if a.len() <= usize::from(sample) {
            return Err("#DIV/0!");
        }
        let ma = mean(&a);
        let mb = mean(&b);
        let mut sab = 0.0;
        let mut saa = 0.0;
        let mut sbb = 0.0;
        for (&a, &b) in a.iter().zip(&b) {
            sab += (a - ma) * (b - mb);
            saa += (a - ma).powi(2);
            sbb += (b - mb).powi(2);
        }
        return match name {
            "COVAR" | "COVARIANCE.P" | "COVARIANCE.S" => {
                Ok(sab / (a.len() - usize::from(sample)) as f64)
            }
            "CORREL" | "PEARSON" | "RSQ" => {
                if a.len() < 2 || saa <= 0.0 || sbb <= 0.0 {
                    return Err("#DIV/0!");
                }
                let r = sab / (saa.sqrt() * sbb.sqrt());
                Ok(if name == "RSQ" { r * r } else { r })
            }
            _ => {
                if sbb <= 0.0 {
                    return Err("#DIV/0!");
                }
                let slope = sab / sbb;
                match name {
                    "SLOPE" => Ok(slope),
                    "INTERCEPT" => Ok(ma - slope * mb),
                    "STEYX" => {
                        if a.len() < 3 {
                            return Err("#DIV/0!");
                        }
                        Ok(((saa - sab * sab / sbb).max(0.0) / (a.len() - 2) as f64).sqrt())
                    }
                    _ => Ok(ma + slope * (number(args.first(), None)? - mb)),
                }
            }
        };
    }
    need(args, 1, 255)?;
    let count_text = matches!(
        name,
        "STDEVA" | "STDEVPA" | "VARA" | "VARPA" | "AVERAGEA" | "MAXA" | "MINA"
    );
    let v = numbers(args, count_text)?;
    match name {
        "MEDIAN" => {
            if v.is_empty() {
                return Err("#NUM!");
            }
            let mut v = v;
            v.sort_by(f64::total_cmp);
            Ok(if v.len() % 2 == 1 {
                v[v.len() / 2]
            } else {
                v[v.len() / 2 - 1] / 2.0 + v[v.len() / 2] / 2.0
            })
        }
        "MODE" | "MODE.SNGL" => {
            let mut counts = std::collections::HashMap::new();
            for &n in &v {
                *counts
                    .entry(if n == 0.0 { 0 } else { n.to_bits() })
                    .or_insert(0usize) += 1;
            }
            let max = counts.values().copied().max().unwrap_or(0);
            if max < 2 {
                return Err("#N/A");
            }
            Ok(*v
                .iter()
                .find(|n| counts[&if **n == 0.0 { 0 } else { n.to_bits() }] == max)
                .unwrap())
        }
        "STDEV" | "STDEV.S" | "STDEV.P" | "STDEVP" | "VAR" | "VAR.S" | "VAR.P" | "VARP"
        | "STDEVA" | "STDEVPA" | "VARA" | "VARPA" => {
            let sample = !matches!(
                name,
                "STDEV.P" | "STDEVP" | "VAR.P" | "VARP" | "STDEVPA" | "VARPA"
            );
            let variance = variance(&v, sample)?;
            Ok(if name.starts_with("VAR") {
                variance
            } else {
                variance.sqrt()
            })
        }
        "AVERAGEA" => {
            if v.is_empty() {
                Err("#DIV/0!")
            } else {
                Ok(mean(&v))
            }
        }
        "MAXA" => Ok(v.into_iter().reduce(f64::max).unwrap_or(0.0)),
        "MINA" => Ok(v.into_iter().reduce(f64::min).unwrap_or(0.0)),
        "AVEDEV" => {
            if v.is_empty() {
                Err("#NUM!")
            } else {
                let m = mean(&v);
                Ok(v.iter().map(|n| (n - m).abs() / v.len() as f64).sum())
            }
        }
        "DEVSQ" => {
            let m = mean(&v);
            Ok(v.iter().map(|n| (n - m).powi(2)).sum())
        }
        "GEOMEAN" | "HARMEAN" => {
            if v.is_empty() || v.iter().any(|n| *n <= 0.0) {
                return Err("#NUM!");
            }
            if name == "GEOMEAN" {
                Ok((v.iter().map(|n| n.ln() / v.len() as f64).sum::<f64>()).exp())
            } else {
                Ok(v.len() as f64 / v.iter().map(|n| 1.0 / n).sum::<f64>())
            }
        }
        "KURT" | "SKEW" | "SKEW.P" => {
            let n = v.len() as f64;
            let required = if name == "KURT" {
                4
            } else if name == "SKEW" {
                3
            } else {
                1
            };
            if v.len() < required {
                return Err("#DIV/0!");
            }
            let m = mean(&v);
            let std = variance(&v, name != "SKEW.P")?.sqrt();
            if std <= 0.0 {
                return Err("#DIV/0!");
            }
            if name == "KURT" {
                let sum = v.iter().map(|x| ((x - m) / std).powi(4)).sum::<f64>();
                Ok(n * (n + 1.0) / ((n - 1.0) * (n - 2.0) * (n - 3.0)) * sum
                    - 3.0 * (n - 1.0).powi(2) / ((n - 2.0) * (n - 3.0)))
            } else {
                let sum = v.iter().map(|x| ((x - m) / std).powi(3)).sum::<f64>();
                Ok(if name == "SKEW" {
                    n / ((n - 1.0) * (n - 2.0)) * sum
                } else {
                    sum / n
                })
            }
        }
        _ => Err("#NAME?"),
    }
}
pub(super) fn call(name: &str, args: &[Arg]) -> Value {
    match calculate(name, args) {
        Ok(n) => finite(n),
        Err(e) => Value::Error(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    fn approximately(actual: &str, expected: f64) {
        let actual = actual.parse::<f64>().unwrap();
        assert!(
            (actual - expected).abs() <= 1e-9 * expected.abs().max(1.0),
            "{actual} != {expected}"
        );
    }
    #[test]
    fn supported_statistics_and_distributions_match_the_mac_reference() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../fixtures/macos-function-cases.json")).unwrap();
        let cases = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| {
                let name = c["formula"]
                    .as_str()
                    .unwrap()
                    .trim_start_matches('=')
                    .split('(')
                    .next()
                    .unwrap();
                handles(name) || super::super::distributions::handles(name)
            })
            .collect::<Vec<_>>();
        assert!(cases.len() >= 110);
        let mut book = Workbook::new(cases.len().max(10), 24).unwrap();
        for (col, values) in [
            (10, vec!["3", "7", "2", "9", "4", "7", "1", "8", "5", "6"]),
            (
                11,
                vec![
                    "apple", "Banana", "cherry", "apple", "date", "Elder", "fig", "grape", "apple",
                    "kiwi",
                ],
            ),
            (
                13,
                vec![
                    "2.1", "3.9", "6.2", "7.8", "10.1", "12.2", "13.8", "16.1", "18", "20.2",
                ],
            ),
            (14, vec!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]),
            (17, vec!["TRUE", "text", "", "5", "=NA()", "0"]),
            (20, vec!["1", "2", "3", "4", "5"]),
            (21, vec!["5", "3", "8", "1", "9"]),
            (22, vec!["10", "12", "20", "14", "25"]),
        ] {
            for (row, value) in values.into_iter().enumerate() {
                book.set_cell(row, col, value.into()).unwrap();
            }
        }
        for (i, c) in cases.iter().enumerate() {
            book.set_cell(i, 0, c["formula"].as_str().unwrap().into())
                .unwrap();
        }
        let grid = book.display_grid();
        for (i, c) in cases.iter().enumerate() {
            let expected = c["expected"].as_str().unwrap();
            let actual = &grid[i][0];
            if let Ok(n) = expected.parse::<f64>() {
                approximately(actual, n);
            } else {
                assert_eq!(actual, expected, "{}", c["formula"]);
            }
        }
    }
    #[test]
    fn typed_ranges_blanks_pairs_optional_arguments_and_error_domains() {
        let mut book = Workbook::new(30, 8).unwrap();
        for (i, v) in ["1", "2", "text", "TRUE", "", "=\"\""]
            .into_iter()
            .enumerate()
        {
            book.set_cell(i, 0, v.into()).unwrap();
        }
        for (i, v) in ["2", "4", "6", "8", "10", "12"].into_iter().enumerate() {
            book.set_cell(i, 1, v.into()).unwrap();
        }
        for (i, (formula, expected)) in [
            ("=STDEV.S(A1:A6)", 2.0f64.sqrt() / 2.0),
            ("=AVERAGEA(A1:A6)", 0.8),
            ("=COUNTBLANK(A1:A6)", 2.0),
            ("=CORREL(A1:A6,B1:B6)", 1.0),
            ("=MAXA(A3:A5)", 1.0),
            ("=MINA(A1:A4)", 0.0),
            ("=STDEV.S(1,,3)", 2.0f64.sqrt()),
            ("=MODE(3,2,3,2)", 3.0),
            ("=MEDIAN(1,TRUE,\"3\")", 1.0),
            ("=1e2+1E-2", 100.01),
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(i, 2, formula.into()).unwrap();
            approximately(&book.display_grid()[i][2], expected);
        }
        for (i, (formula, expected)) in [
            ("=VAR.S(1)", "#DIV/0!"),
            ("=MEDIAN()", "#VALUE!"),
            ("=GEOMEAN(0,2)", "#NUM!"),
            ("=PERCENTILE.EXC(A1:A2,0.1)", "#NUM!"),
            ("=RANK(99,A1:A2)", "#N/A"),
            ("=CORREL(A1:A2,B1:B3)", "#N/A"),
            ("=SLOPE(A1:A2,B1:B1)", "#N/A"),
            ("=PERCENTRANK(A1:A2,0)", "#N/A"),
            ("=STDEV.P(1e308,1e308)", "0"),
            ("=STDEV.S(1e308,-1e308)", "#NUM!"),
            ("=1e+", "#ERROR!"),
            ("=1E99999", "#NUM!"),
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(i, 3, formula.into()).unwrap();
            assert_eq!(book.display_grid()[i][3], expected, "{formula}");
        }
    }
    #[test]
    fn external_xlsx_and_ods_recalculate_statistics_after_input_edits() {
        let mut book = Workbook::new(20, 8).unwrap();
        for (i, v) in ["1", "2", "text", "TRUE", ""].into_iter().enumerate() {
            book.set_cell(i, 0, v.into()).unwrap();
            book.set_cell(i, 1, (2 * (i + 1)).to_string()).unwrap();
        }
        for (i, formula) in [
            "=STDEV.S(A1:A5)",
            "=AVERAGEA(A1:A5)",
            "=COUNTBLANK(A1:A5)",
            "=CORREL(A1:A5,B1:B5)",
            "=NORM.DIST(A1,0,1,TRUE)",
            "=BETA.INV(0.5,2,2)",
            "=T.DIST.2T(A1,10)",
            "=VAR.S(A1:A5)",
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(i, 2, formula.into()).unwrap();
        }
        let before = book.display_grid();
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" {
                book.xlsx_bytes()
            } else {
                book.ods_bytes()
            }
            .unwrap();
            let mut input = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for i in 0..input.len() {
                let part = input.by_index(i).unwrap();
                if part.name() != "xls42/project.json" {
                    output.raw_copy_file(part).unwrap();
                }
            }
            let mut external =
                Workbook::from_office_bytes(&output.finish().unwrap().into_inner(), kind).unwrap();
            assert_eq!(external.display_grid(), before);
            external.set_cell(0, 0, "4".into()).unwrap();
            let changed = external.display_grid();
            approximately(&changed[0][2], 2.0f64.sqrt());
            approximately(&changed[1][2], 1.75);
            approximately(&changed[3][2], -1.0);
            approximately(&changed[4][2], 0.9999683287581669);
            approximately(&changed[7][2], 2.0);
            assert_ne!(changed[6][2], before[6][2]);
        }
    }
    #[test]
    fn repeated_large_reference_arguments_are_bounded_before_accumulation() {
        let mut book = Workbook::new(2, 2).unwrap();
        book.set_cell(0, 0, "x".repeat(2 * 1024 * 1024)).unwrap();
        book.set_cell(0, 1, "=MEDIAN(A1,A1,A1,A1,A1,1)".into())
            .unwrap();
        assert_eq!(book.display_grid()[0][1], "#LIMIT!");
    }
}
