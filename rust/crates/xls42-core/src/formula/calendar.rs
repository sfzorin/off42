//! Workbook-calendar functions; serial 60 retains Excel's fictional leap day.
use super::{finite, statistics::number, text::Arg, Value, MAX_STEPS};
use crate::{display, Workbook};
use chrono::{Datelike, NaiveDate};
use std::collections::BTreeSet;
type Result<T> = std::result::Result<T, &'static str>;
pub(super) fn handles(n: &str) -> bool {
    matches!(
        n,
        "EDATE"
            | "EOMONTH"
            | "WEEKDAY"
            | "WEEKNUM"
            | "ISOWEEKNUM"
            | "DATEDIF"
            | "DAYS"
            | "DAYS360"
            | "YEARFRAC"
            | "NETWORKDAYS"
            | "NETWORKDAYS.INTL"
            | "WORKDAY"
            | "WORKDAY.INTL"
            | "TIMEVALUE"
            | "HOUR"
            | "MINUTE"
            | "SECOND"
    )
}
#[derive(Clone, Copy)]
struct Parts {
    year: i32,
    month: u32,
    day: u32,
}
fn parts(s: i64) -> Result<Parts> {
    if s == 0 {
        return Ok(Parts {
            year: 1900,
            month: 1,
            day: 0,
        });
    }
    if s == 60 {
        return Ok(Parts {
            year: 1900,
            month: 2,
            day: 29,
        });
    }
    let d = display::serial_to_date(s as f64).ok_or("#NUM!")?;
    Ok(Parts {
        year: d.year(),
        month: d.month(),
        day: d.day(),
    })
}
fn leap(y: i32) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}
fn month_days(y: i32, m: u32) -> u32 {
    match m {
        2 => {
            if leap(y) || y == 1900 {
                29
            } else {
                28
            }
        }
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
fn serial(y: i32, m: u32, d: u32) -> Result<i64> {
    if d == 0 {
        return serial(y, m, 1).map(|n| n - 1);
    }
    if y == 1900 && m == 2 && d == 29 {
        return Ok(60);
    }
    if !(1900..=9999).contains(&y) {
        return Err("#NUM!");
    }
    display::date_to_serial(NaiveDate::from_ymd_opt(y, m, d).ok_or("#NUM!")?)
        .map(|n| n as i64)
        .ok_or("#NUM!")
}
fn weekday(s: i64) -> usize {
    (s + 5).rem_euclid(7) as usize
}
fn charge(steps: &mut usize, n: usize) -> Result<()> {
    *steps = steps
        .checked_add(n)
        .filter(|n| *n <= MAX_STEPS)
        .ok_or("#LIMIT!")?;
    Ok(())
}
fn weekend(arg: Option<&Arg>) -> Result<[bool; 7]> {
    let mut mask = [false; 7];
    match arg {
        None | Some(Arg::Missing) => {
            mask[5] = true;
            mask[6] = true;
        }
        Some(arg) => match arg.scalar()? {
            Value::Text(s) => {
                if s.len() != 7 || !s.bytes().all(|c| matches!(c, b'0' | b'1')) || s == "1111111" {
                    return Err("#VALUE!");
                }
                for (i, b) in s.bytes().enumerate() {
                    mask[i] = b == b'1';
                }
            }
            _ => {
                let k = number(Some(arg), None)?.trunc();
                if (1.0..=7.0).contains(&k) {
                    mask[(k as usize + 4) % 7] = true;
                    mask[(k as usize + 5) % 7] = true;
                } else if (11.0..=17.0).contains(&k) {
                    mask[(k as i32 - 12).rem_euclid(7) as usize] = true;
                } else {
                    return Err("#NUM!");
                }
            }
        },
    }
    Ok(mask)
}
fn days360(a: Parts, b: Parts, european: bool, yearfrac: bool) -> f64 {
    let (mut d1, mut d2) = (a.day as i32, b.day as i32);
    if european {
        d1 = d1.min(30);
        d2 = d2.min(30);
    } else {
        let feb1 = a.month == 2 && a.day == month_days(a.year, 2);
        let feb2 = b.month == 2 && b.day == month_days(b.year, 2);
        if yearfrac && feb1 && feb2 {
            d2 = 30;
        }
        if feb1 {
            d1 = 30;
        }
        if !yearfrac && d1 == 31 {
            d1 = 30;
        }
        if d2 == 31 && d1 >= 30 {
            d2 = 30;
        }
        if d1 == 31 {
            d1 = 30;
        }
    }
    ((b.year - a.year) * 360 + (b.month as i32 - a.month as i32) * 30 + d2 - d1) as f64
}
fn business_count(
    lo: i64,
    hi: i64,
    mask: &[bool; 7],
    hol: &BTreeSet<i64>,
    steps: &mut usize,
) -> Result<i64> {
    if hi < lo {
        return Ok(0);
    }
    let days = hi - lo + 1;
    let per = mask.iter().filter(|b| !**b).count() as i64;
    let mut result = days / 7 * per;
    for i in 0..days % 7 {
        if !mask[weekday(lo + i)] {
            result += 1;
        }
    }
    for h in hol.range(lo..=hi) {
        charge(steps, 1)?;
        if !mask[weekday(*h)] {
            result -= 1;
        }
    }
    Ok(result)
}
pub(super) fn call(name: &str, args: &[Arg], book: &Workbook, steps: &mut usize) -> Value {
    match calculate(name, args, book, steps) {
        Ok(n) => finite(n),
        Err(e) => Value::Error(e),
    }
}
fn calculate(name: &str, a: &[Arg], book: &Workbook, steps: &mut usize) -> Result<f64> {
    let (min, max) = match name {
        "TIMEVALUE" | "HOUR" | "MINUTE" | "SECOND" | "ISOWEEKNUM" => (1, 1),
        "WEEKNUM" | "WEEKDAY" => (1, 2),
        "DATEDIF" => (3, 3),
        "YEARFRAC" | "DAYS360" | "NETWORKDAYS" | "WORKDAY" => (2, 3),
        "NETWORKDAYS.INTL" | "WORKDAY.INTL" => (2, 4),
        _ => (2, 2),
    };
    if !(min..=max).contains(&a.len()) {
        return Err("#VALUE!");
    }
    let offset = if book.history.current().date_1904 {
        1462
    } else {
        0
    };
    let n = |i: usize| number(a.get(i), None);
    let opt = |i: usize, d: f64| number(a.get(i), Some(d));
    let day = |i: usize| -> Result<i64> {
        let arg = a.get(i).ok_or("#VALUE!")?;
        let v = match arg.scalar()? {
            Value::Text(s) => display::workbook_parse_serial(book, &s).ok_or("#VALUE!")?,
            _ => n(i)?,
        };
        let absolute = v.floor() + offset as f64;
        if v < 0.0 || absolute > 2_958_465.0 {
            return Err("#NUM!");
        }
        Ok(absolute as i64)
    };
    if name == "TIMEVALUE" {
        let s = a[0].scalar()?.display();
        let s = s.trim();
        if let Some(t) = display::parse_serial(&s.replace(' ', "T")) {
            return Ok(t.fract());
        }
        let fields = s.split(':').collect::<Vec<_>>();
        if !(2..=3).contains(&fields.len()) {
            return Err("#VALUE!");
        }
        let v = fields
            .iter()
            .map(|s| s.parse::<f64>().map_err(|_| "#VALUE!"))
            .collect::<Result<Vec<_>>>()?;
        if v.iter().any(|n| !n.is_finite() || *n < 0.0)
            || v[0] >= 24.0
            || v[1] >= 60.0
            || v[0].fract() != 0.0
            || v[1].fract() != 0.0
            || v.get(2).is_some_and(|x| *x >= 60.0)
        {
            return Err("#VALUE!");
        }
        return Ok((v[0] * 3600.0 + v[1] * 60.0 + v.get(2).copied().unwrap_or(0.0)) / 86400.0);
    }
    if matches!(name, "HOUR" | "MINUTE" | "SECOND") {
        let v = n(0)?;
        if !(0.0..3_000_000.0).contains(&v) {
            return Err("#NUM!");
        }
        let secs = (v.fract() * 86400.0).round() as i64 % 86400;
        return Ok(match name {
            "HOUR" => secs / 3600,
            "MINUTE" => secs / 60 % 60,
            _ => secs % 60,
        } as f64);
    }
    let start = day(0)?;
    let p = parts(start)?;
    match name {
        "EDATE" | "EOMONTH" => {
            let months = n(1)?.trunc();
            if months.abs() > 120_000.0 {
                return Err("#NUM!");
            }
            let index = p.year as i64 * 12 + p.month as i64 - 1 + months as i64;
            let (y, m) = (
                index.div_euclid(12) as i32,
                (index.rem_euclid(12) + 1) as u32,
            );
            let d = if name == "EOMONTH" {
                month_days(y, m)
            } else {
                p.day.min(month_days(y, m))
            };
            let s = serial(y, m, d)?;
            if s < offset {
                return Err("#NUM!");
            }
            Ok((s - offset) as f64)
        }
        "WEEKDAY" => {
            let typ = opt(1, 1.0)?.trunc();
            let w = weekday(start) as i64;
            Ok(match typ as i64 {
                1 | 17 => (w + 1) % 7 + 1,
                2 | 11 => w + 1,
                3 => w,
                12..=16 => (w - (typ as i64 - 11)).rem_euclid(7) + 1,
                _ => return Err("#NUM!"),
            } as f64)
        }
        "WEEKNUM" | "ISOWEEKNUM" => {
            let typ = if name == "ISOWEEKNUM" {
                21.0
            } else {
                opt(1, 1.0)?.trunc()
            };
            if typ == 21.0 {
                let thursday = start - weekday(start) as i64 + 3;
                let p = parts(thursday)?;
                return Ok(((thursday - serial(p.year, 1, 1)?) / 7 + 1) as f64);
            }
            let first = match typ as i64 {
                1 | 17 => 6,
                2 | 11 => 0,
                12..=16 => typ as i64 - 11,
                _ => return Err("#NUM!"),
            };
            let jan = serial(p.year, 1, 1)?;
            let off = (weekday(jan) as i64 - first).rem_euclid(7);
            Ok(((start - jan + off) / 7 + 1) as f64)
        }
        "DAYS" => Ok((start - day(1)?) as f64),
        "DATEDIF" => {
            let end = day(1)?;
            if end < start {
                return Err("#NUM!");
            }
            let q = parts(end)?;
            let months =
                (q.year - p.year) * 12 + q.month as i32 - p.month as i32 - i32::from(q.day < p.day);
            match a[2].scalar()?.display().to_ascii_uppercase().as_str() {
                "Y" => Ok((months / 12) as f64),
                "M" => Ok(months as f64),
                "D" => Ok((end - start) as f64),
                "YM" => Ok((months % 12) as f64),
                "YD" => {
                    let y = q.year - i32::from((q.month, q.day) < (p.month, p.day));
                    Ok(
                        (end - serial(y, p.month, p.day.min(month_days(y, p.month)).max(1))?)
                            as f64,
                    )
                }
                "MD" => {
                    let mut d = q.day as i32 - p.day as i32;
                    if d < 0 {
                        d += month_days(
                            q.year - i32::from(q.month == 1),
                            if q.month == 1 { 12 } else { q.month - 1 },
                        ) as i32;
                    }
                    Ok(d as f64)
                }
                _ => Err("#NUM!"),
            }
        }
        "DAYS360" | "YEARFRAC" => {
            let end = day(1)?;
            if name == "DAYS360" {
                return Ok(days360(p, parts(end)?, opt(2, 0.0)? != 0.0, false));
            }
            let basis = opt(2, 0.0)?.trunc();
            if !(0.0..=4.0).contains(&basis) {
                return Err("#NUM!");
            }
            let (lo, hi) = if start <= end {
                (start, end)
            } else {
                (end, start)
            };
            let (a, b) = (parts(lo)?, parts(hi)?);
            Ok(match basis as i32 {
                0 => days360(a, b, false, true) / 360.0,
                2 => (hi - lo) as f64 / 360.0,
                3 => (hi - lo) as f64 / 365.0,
                4 => days360(a, b, true, false) / 360.0,
                _ => {
                    let within = a.year == b.year
                        || b.year == a.year + 1
                            && (a.month > b.month || a.month == b.month && a.day >= b.day);
                    let denominator = if within {
                        let is_leap = if a.year == b.year {
                            leap(a.year)
                        } else {
                            leap(a.year) && lo <= serial(a.year, 2, 29)?
                                || leap(b.year) && hi >= serial(b.year, 2, 29)?
                        };
                        if is_leap {
                            366.0
                        } else {
                            365.0
                        }
                    } else {
                        charge(steps, (b.year - a.year + 1) as usize)?;
                        (a.year..=b.year)
                            .map(|y| if leap(y) { 366.0 } else { 365.0 })
                            .sum::<f64>()
                            / (b.year - a.year + 1) as f64
                    };
                    (hi - lo) as f64 / denominator
                }
            })
        }
        "NETWORKDAYS" | "NETWORKDAYS.INTL" | "WORKDAY" | "WORKDAY.INTL" => {
            let intl = name.ends_with("INTL");
            let mask = weekend(if intl { a.get(2) } else { None })?;
            let mut hol = BTreeSet::new();
            if let Some(arg) = a.get(if intl { 3 } else { 2 }) {
                match arg {
                    Arg::Missing => {}
                    Arg::Range(v, b) => {
                        for (v, b) in v.iter().zip(b) {
                            charge(steps, 1)?;
                            if *b {
                                continue;
                            }
                            match v {
                                Value::Number(d) => {
                                    let d = d.floor() + offset as f64;
                                    if d < offset as f64 || d > 2_958_465.0 {
                                        return Err("#NUM!");
                                    }
                                    hol.insert(d as i64);
                                }
                                Value::Error(e) => return Err(e),
                                _ => {}
                            }
                        }
                    }
                    _ => {
                        let d = number(Some(arg), None)?.floor() + offset as f64;
                        if d < offset as f64 || d > 2_958_465.0 {
                            return Err("#NUM!");
                        }
                        hol.insert(d as i64);
                    }
                }
            }
            if name.starts_with("NETWORKDAYS") {
                let end = day(1)?;
                return if end >= start {
                    Ok(business_count(start, end, &mask, &hol, steps)? as f64)
                } else {
                    Ok(-business_count(end, start, &mask, &hol, steps)? as f64)
                };
            }
            let count = n(1)?.trunc();
            if count.abs() >= 3_000_000.0 {
                return Err("#NUM!");
            }
            if count == 0.0 {
                return Ok((start - offset) as f64);
            }
            let forward = count > 0.0;
            let target = count.abs() as i64;
            let limit = if forward {
                2_958_465 - start
            } else {
                start - offset
            };
            let mut count_at = |distance: i64| {
                if forward {
                    business_count(start + 1, start + distance, &mask, &hol, steps)
                } else {
                    business_count(start - distance, start - 1, &mask, &hol, steps)
                }
            };
            if count_at(limit)? < target {
                return Err("#NUM!");
            }
            let (mut lo, mut hi) = (1, limit);
            while lo < hi {
                let mid = lo + (hi - lo) / 2;
                if count_at(mid)? >= target {
                    hi = mid;
                } else {
                    lo = mid + 1;
                }
            }
            Ok((start + if forward { lo } else { -lo } - offset) as f64)
        }
        _ => Err("#NAME?"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workbook_calendars_fictional_day_weekends_and_errors() {
        let mut b = Workbook::new(40, 5).unwrap();
        for (i, (f, e)) in [
            ("=EDATE(DATE(1900,1,31),1)", "60"),
            ("=EOMONTH(60,0)", "60"),
            ("=WEEKDAY(60,2)", "3"),
            (
                "=NETWORKDAYS.INTL(DATE(2024,1,1),DATE(2024,1,7),\"0000000\")",
                "7",
            ),
            (
                "=WORKDAY.INTL(DATE(2024,1,1),1000000,\"0000000\")",
                "1045292",
            ),
            ("=WORKDAY.INTL(DATE(2024,1,1),-3,\"0000000\")", "45289"),
            ("=NETWORKDAYS.INTL(1,2,\"1111111\")", "#VALUE!"),
            ("=WORKDAY.INTL(1,2,8)", "#NUM!"),
            ("=WORKDAY(2958465,1)", "#NUM!"),
            ("=WEEKDAY(1,4)", "#NUM!"),
            ("=YEARFRAC(1,2,5)", "#NUM!"),
            ("=TIMEVALUE(\"25:00\")", "#VALUE!"),
            ("=SECOND(1.5)", "0"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 0, (*f).into()).unwrap();
            assert_eq!(&b.display_grid()[i][0], e, "{f}");
        }
        let mut state=b.history.current().clone();state.date_1904=true;b.history.apply(state);
        for (i, (f, e)) in [
            ("=EDATE(0,1)", "31"),
            ("=EOMONTH(0,1)", "59"),
            ("=DAYS(DATE(1904,2,1),0)", "31"),
            ("=WORKDAY(0,1)", "3"),
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(i, 1, (*f).into()).unwrap();
            assert_eq!(&b.display_grid()[i][1], e, "{f}");
        }
    }
}
