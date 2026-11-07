//! Fixed decimal and Roman text conversions. Number separators follow the current formula convention.
use super::*;
pub(super) fn calculate(name: &str, x: &Args<'_>) -> Result<Value> {
    if name == "ARABIC" {
        x.count(1, 1)?;
        let text = x.text(0)?.trim().to_ascii_uppercase();
        let negative = text.starts_with('-');
        let text = text.strip_prefix('-').unwrap_or(&text);
        let (mut total, mut previous) = (0_i64, 0);
        for c in text.chars().rev() {
            let v = match c {
                'I' => 1,
                'V' => 5,
                'X' => 10,
                'L' => 50,
                'C' => 100,
                'D' => 500,
                'M' => 1000,
                _ => return Err("#VALUE!"),
            };
            total += if v < previous {
                -v
            } else {
                previous = v;
                v
            };
        }
        return Ok(Value::Number(if negative { -total } else { total } as f64));
    }
    if name == "ROMAN" {
        x.count(1, 2)?;
        let mut n = x.integer(0, 0)?;
        if !(0..=3999).contains(&n) {
            return Err("#VALUE!");
        }
        // macOS currently uses classic form for all form arguments.
        let mut out = String::new();
        for (v, letters) in [
            (1000, "M"),
            (900, "CM"),
            (500, "D"),
            (400, "CD"),
            (100, "C"),
            (90, "XC"),
            (50, "L"),
            (40, "XL"),
            (10, "X"),
            (9, "IX"),
            (5, "V"),
            (4, "IV"),
            (1, "I"),
        ] {
            while n >= v {
                out.push_str(letters);
                n -= v;
            }
        }
        return Ok(Value::Text(out));
    }
    x.count(1, if name == "FIXED" { 3 } else { 2 })?;
    let number = number(x.value(0)?)?;
    let decimals = x.integer(1, 2)?;
    if decimals > 127 {
        return Err("#VALUE!");
    }
    let factor = 10f64.powi(decimals.max(-309) as i32);
    let rounded = if factor == 0.0 {
        0.0
    } else {
        let scaled = number * factor;
        if scaled.is_finite() {
            (if (scaled * 1e9).is_finite() {
                (scaled * 1e9).round() / 1e9
            } else {
                scaled
            })
            .round()
                / factor
        } else {
            number
        }
    };
    if !rounded.is_finite() {
        return Err("#NUM!");
    }
    let body = format!("{:.*}", decimals.max(0) as usize, rounded.abs());
    let grouping = if name == "FIXED" {
        match x.0.get(2) {
            None | Some(Arg::Missing) => true,
            Some(v) => !super::super::information::truth(v.scalar()?)?,
        }
    } else {
        true
    };
    let (whole, fraction) = body.split_once('.').unwrap_or((&body, ""));
    let mut out = String::new();
    for (i, c) in whole.chars().enumerate() {
        if grouping && i > 0 && (whole.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    if decimals > 0 {
        out.push('.');
        out.push_str(fraction);
    }
    if name == "DOLLAR" {
        out.insert(0, '$');
        if rounded < 0.0 {
            out = format!("({out})");
        }
    } else if rounded < 0.0 {
        out.insert(0, '-');
    }
    Ok(Value::Text(out))
}
#[cfg(test)]
mod tests {
    use crate::{formula, Workbook};
    #[test]
    fn fixed_roman_and_arabic_bounds() {
        let mut b = Workbook::new(1, 1).unwrap();
        for (f, e) in [
            ("=FIXED(1234.567,-2,TRUE)", "1200"),
            ("=FIXED(1234.567,1)", "1,234.6"),
            ("=FIXED(-0.125,2,TRUE)", "-0.13"),
            ("=FIXED(1.005,2,TRUE)", "1.01"),
            ("=FIXED(2,-1000,TRUE)", "0"),
            ("=FIXED(1,128)", "#VALUE!"),
            ("=DOLLAR(-1234.5,1)", "($1,234.5)"),
            ("=ROMAN(1999)", "MCMXCIX"),
            ("=ROMAN(4000)", "#VALUE!"),
            ("=ARABIC(\"-iv\")", "-4"),
            ("=ARABIC(\"\")", "0"),
            ("=ARABIC(\"oops\")", "#VALUE!"),
        ] {
            b.set_cell(0, 0, f.into()).unwrap();
            assert_eq!(formula::display_cell(&b, 0, 0), e, "{f}");
        }
    }
}
