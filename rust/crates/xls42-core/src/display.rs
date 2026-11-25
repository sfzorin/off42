//! Conservative common-format projection; unsupported custom codes retain raw display.
use crate::{CellContent, Workbook};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
pub(crate) fn apply(book: &Workbook, grid: &mut [Vec<String>]) {
    for (r, row) in grid.iter_mut().enumerate() {
        for (c, text) in row.iter_mut().enumerate() {
            let Some(style) = book.style(r, c) else {
                continue;
            };
            let code = style.number_format.trim();
            let Some(content) = book.content(r, c) else {
                continue;
            };
            if let CellContent::Date(_) = content {
                if let Some(date) = parse_date(text) {
                    let rendered = match code.to_ascii_lowercase().as_str() {
                        "m/d/yy" => Some(format!(
                            "{}/{}/{:02}",
                            date.month(),
                            date.day(),
                            date.year() % 100
                        )),
                        "mm-dd-yy" => Some(format!(
                            "{:02}-{:02}-{:02}",
                            date.month(),
                            date.day(),
                            date.year() % 100
                        )),
                        "yyyy-mm-dd" => Some(date.format("%Y-%m-%d").to_string()),
                        "d-mmm-yy" => Some(date.format("%-d-%b-%y").to_string()),
                        "d-mmm" => Some(date.format("%-d-%b").to_string()),
                        "mmm-yy" => Some(date.format("%b-%y").to_string()),
                        "" | "general" => Some(if text.contains('T') {
                            text.replace('T', " ")
                        } else {
                            date.format("%Y-%m-%d").to_string()
                        }),
                        _ => None,
                    };
                    if let Some(v) = rendered {
                        *text = v
                    }
                }
                continue;
            }
            if !matches!(content, CellContent::Number(_) | CellContent::Formula(_)) {
                continue;
            }
            let Ok(value) = text.parse::<f64>() else {
                continue;
            };
            if !value.is_finite() {
                continue;
            }
            if !book.history.current().date_1904 && value.floor() == 60.0 {
                let rendered = match code.to_ascii_lowercase().as_str() {
                    "yyyy-mm-dd" => Some("1900-02-29"),
                    "m/d/yy" => Some("2/29/00"),
                    "mm-dd-yy" => Some("02-29-00"),
                    "d-mmm-yy" => Some("29-Feb-00"),
                    _ => None,
                };
                if let Some(value) = rendered {
                    *text = value.into();
                    continue;
                }
            }
            if let Some(date) = workbook_serial_to_date(book, value) {
                let rendered = match code.to_ascii_lowercase().as_str() {
                    "yyyy-mm-dd" => Some(date.format("%Y-%m-%d").to_string()),
                    "m/d/yy" => Some(format!(
                        "{}/{}/{:02}",
                        date.month(),
                        date.day(),
                        date.year() % 100
                    )),
                    "mm-dd-yy" => Some(format!(
                        "{:02}-{:02}-{:02}",
                        date.month(),
                        date.day(),
                        date.year() % 100
                    )),
                    "d-mmm-yy" => Some(date.format("%-d-%b-%y").to_string()),
                    _ => None,
                };
                if let Some(v) = rendered {
                    *text = v;
                    continue;
                }
            }
            let (pattern, percent) = match code.strip_suffix('%') {
                Some(p) => (p, true),
                None => (code, false),
            };
            let decimals = match pattern {
                "0" | "#,##0" => Some(0),
                "0.0" | "#,##0.0" => Some(1),
                "0.00" | "#,##0.00" => Some(2),
                "0.000" | "#,##0.000" => Some(3),
                _ => None,
            };
            if let Some(n) = decimals {
                let scaled = if percent { value * 100.0 } else { value };
                let mut rendered = format!("{:.*}", n, scaled);
                if pattern.starts_with("#,##") {
                    let dot = rendered.find('.').unwrap_or(rendered.len());
                    let start = usize::from(rendered.starts_with('-'));
                    let mut i = dot;
                    while i > start + 3 {
                        i -= 3;
                        rendered.insert(i, ',');
                    }
                }
                if percent {
                    rendered.push('%')
                }
                *text = rendered;
            }
        }
    }
}
pub(crate) fn parse_date(s: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").ok().or_else(|| {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")
            .ok()
            .map(|d| d.date())
    })
}
pub(crate) fn workbook_serial_to_date(book: &Workbook, serial: f64) -> Option<NaiveDate> {
    serial_to_date(if book.history.current().date_1904 {
        serial + 1462.0
    } else {
        serial
    })
}
pub(crate) fn workbook_date_to_serial(book: &Workbook, date: NaiveDate) -> Option<f64> {
    let value = date_to_serial(date)?
        - if book.history.current().date_1904 {
            1462.0
        } else {
            0.0
        };
    if value < 0.0 {
        None
    } else {
        Some(value)
    }
}
pub(crate) fn workbook_parse_serial(book: &Workbook, input: &str) -> Option<f64> {
    let value = parse_serial(input)?
        - if book.history.current().date_1904 {
            1462.0
        } else {
            0.0
        };
    if value < 0.0 {
        None
    } else {
        Some(value)
    }
}
pub(crate) fn serial_to_date(serial: f64) -> Option<NaiveDate> {
    if !serial.is_finite() || serial < 0.0 || serial > 2_958_465.0 || serial.floor() == 60.0 {
        return None;
    }
    let n = serial.floor() as i64;
    let days = if n < 60 { n } else { n - 1 };
    NaiveDate::from_ymd_opt(1899, 12, 31)?.checked_add_signed(chrono::Duration::days(days))
}
pub(crate) fn date_to_serial(date: NaiveDate) -> Option<f64> {
    let base = NaiveDate::from_ymd_opt(1899, 12, 31)?;
    let mut n = date.signed_duration_since(base).num_days();
    if n < 0 {
        return None;
    }
    if n >= 60 {
        n += 1
    }
    Some(n as f64)
}

pub(crate) fn parse_serial(s: &str) -> Option<f64> {
    use chrono::Timelike;
    if let Ok(dt) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f") {
        let day = date_to_serial(dt.date())?;
        let time = dt.time();
        return Some(
            day + (time.num_seconds_from_midnight() as f64
                + time.nanosecond() as f64 / 1_000_000_000.0)
                / 86_400.0,
        );
    }
    parse_date(s).and_then(date_to_serial)
}
