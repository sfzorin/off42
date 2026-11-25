//! Conservative common-format projection; unsupported custom codes retain raw display.
use crate::{
    CellContent, Workbook,
    formula::Value,
};
use chrono::{Datelike, NaiveDate, NaiveDateTime};
pub(super) fn format_cell(
    book: &Workbook,
    sheet: crate::SheetId,
    r: usize,
    c: usize,
    calculated: Option<&Value>,
    text: &mut String,
) {
    let Some(source) = book.history.current().sheets.iter().find(|s| s.id == sheet) else {
        return;
    };
    let cell = source.cells.get(&(r, c));
    let code = cell
        .and_then(|c| book.history.current().styles.get(c.style))
        .map_or("", |s| s.number_format.trim());
    let content = cell.map(|c| &c.text);
    if calculated.is_some_and(|v| !matches!(v, Value::Number(_))) {
        return;
    }
    if let Some(CellContent::Date(_)) = content {
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
        return;
    }
    if calculated.is_none()
        && !matches!(
            content,
            Some(CellContent::Number(_) | CellContent::Formula(_))
        )
    {
        return;
    }
    let Ok(value) = text.parse::<f64>() else {
        return;
    };
    if !value.is_finite() {
        return;
    }
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        *text = general(value);
        return;
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
            return;
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
            return;
        }
    }
    if let Some(rendered) = number_pattern(value, code) {
        *text = rendered;
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

/// Match the macOS General projection without feeding rounded display text back into calculations.
pub(super) fn general(value: f64) -> String {
    if !value.is_finite() {
        return "#NUM!".into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let magnitude = value.abs();
    if !(1e-9..1e11).contains(&magnitude) {
        let source = format!("{value:.5E}");
        let (mantissa, exponent) = source.split_once('E').unwrap();
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent = exponent.parse::<i32>().unwrap();
        format!("{mantissa}E{exponent:+03}")
    } else {
        let decimals = (10.0 - magnitude.log10().floor()).clamp(0.0, 10.0) as usize;
        let result = format!("{:.*}", decimals, value);
        if result.contains('.') {
            result.trim_end_matches('0').trim_end_matches('.').into()
        } else {
            result
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CellStyle, Range};
    #[test]
    fn general_matches_actual_swift_formatter_at_boundaries_and_wide_exponents() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/macos-number-display-cases.json"))
                .unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert!(cases.len() > 100);
        for case in cases {
            let input = case["input"].as_str().unwrap();
            let value = input.parse::<f64>().unwrap();
            assert_eq!(
                general(value),
                case["expected"].as_str().unwrap(),
                "{input}"
            );
        }
        for case in fixture["patterns"].as_array().unwrap() {
            let input = case["input"].as_str().unwrap();
            let pattern = case["pattern"].as_str().unwrap();
            let actual = number_pattern(input.parse::<f64>().unwrap(), pattern);
            assert_eq!(
                actual.as_deref(),
                case["expected"].as_str(),
                "{input}: {pattern}"
            );
        }
        assert!(number_pattern(1.0, &format!("0.{}", "0".repeat(128))).is_none());
    }
    #[test]
    fn text_formula_uses_the_same_optional_decimals_and_scientific_rounding_as_cells() {
        let mut book = Workbook::new(2, 4).unwrap();
        for (column, formula, expected) in [
            (0, "=TEXT(0.1234,\"0.0##%\")", "12.34%"),
            (1, "=TEXT(999.999,\"0.00E+00\")", "1.00E+03"),
            (2, "=TEXT(1E-323,\"0.00E-00\")", "9.88E-324"),
            (3, "=TEXT(-0.0001,\"0.##\")", "0"),
        ] {
            book.set_cell(0, column, formula.into()).unwrap();
            assert_eq!(book.display_grid()[0][column], expected);
        }
    }
    #[test]
    fn display_and_clipboard_round_without_changing_calculations_or_office_values() {
        let mut b = Workbook::new(8, 12).unwrap();
        for (r, c, f) in [
            (0, 0, "=1/3"),
            (0, 1, "=A1*3"),
            (0, 2, "=TEXT(7,\"0000\")"),
            (0, 3, "=TEXTSPLIT(\"0007,=1+2\",\",\")"),
            (0, 6, "=SEQUENCE(2)"),
            (0, 8, "3.141592653589793"),
        ] {
            b.set_cell(r, c, f.into()).unwrap();
        }
        for (r, c, code) in [(0, 2, "0.00"), (0, 3, "0%"), (0, 4, "0.00"), (1, 6, "0.00")] {
            b.set_style(
                Range {
                    top: r,
                    left: c,
                    bottom: r,
                    right: c,
                },
                CellStyle {
                    number_format: code.into(),
                    ..CellStyle::default()
                },
            )
            .unwrap();
        }
        let displayed = b.display_grid();
        assert_eq!(
            &displayed[0][..5],
            &["0.3333333333", "1", "0007", "0007", "=1+2"]
        );
        assert_eq!(displayed[1][6], "2.00");
        assert_eq!(displayed[0][8], "3.1415926536");
        assert_eq!(b.cell(0, 8), Some("3.141592653589793"));
        assert_eq!(b.calculated_grid()[0][0], "0.3333333333333333");
        assert_eq!(b.calculated_grid()[1][6], "2");
        let (text, payload) = b.copy_clipboard_mapped(&[0], &[0, 1, 2]).unwrap();
        assert_eq!(text, "0.3333333333\t1\t0007");
        let payload: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(payload["cells"][0]["content"]["value"]["raw"], "=1/3");
        // Native ODS custom-number-format export is still guarded; verify its raw numeric cache on a plain workbook.
        let mut ods_book = Workbook::new(2, 2).unwrap();
        ods_book.set_cell(0, 0, "=1/3".into()).unwrap();
        for (kind, bytes, path, ns, tag, attr) in [
            (
                "xlsx",
                b.xlsx_bytes().unwrap(),
                "xl/worksheets/sheet1.xml",
                "http://schemas.openxmlformats.org/spreadsheetml/2006/main",
                "c",
                "r",
            ),
            (
                "ods",
                ods_book.ods_bytes().unwrap(),
                "content.xml",
                "urn:oasis:names:tc:opendocument:xmlns:table:1.0",
                "table-cell",
                "formula",
            ),
        ] {
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut source = String::new();
            use std::io::Read;
            archive
                .by_name(path)
                .unwrap()
                .read_to_string(&mut source)
                .unwrap();
            let doc = roxmltree::Document::parse(&source).unwrap();
            let cell = doc
                .descendants()
                .find(|n| {
                    n.has_tag_name((ns, tag))
                        && (if kind == "xlsx" {
                            n.attribute(attr)
                        } else {
                            n.attribute((ns, attr))
                        }) == Some(if kind == "xlsx" { "A1" } else { "of:=1/3" })
                })
                .unwrap();
            let value = if kind == "xlsx" {
                cell.children()
                    .find(|n| n.has_tag_name((ns, "v")))
                    .unwrap()
                    .text()
                    .unwrap()
            } else {
                cell.attribute(("urn:oasis:names:tc:opendocument:xmlns:office:1.0", "value"))
                    .unwrap()
            };
            assert_eq!(
                value.parse::<f64>().unwrap().to_bits(),
                (1.0_f64 / 3.0).to_bits(),
                "{kind} full precision"
            );
        }
        b.set_cell(0, 0, "=2/3".into()).unwrap();
        assert_eq!(b.display_grid()[0][0], "0.6666666667");
        assert_eq!(b.calculated_grid()[0][1], "2");
        b.undo();
        assert_eq!(b.display_grid(), displayed);
        b.redo();
        assert_eq!(b.display_grid()[0][0], "0.6666666667");
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Exponent {
    pub digits: usize,
    pub show_plus: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct NumberPattern {
    pub integer: usize,
    pub decimals: usize,
    pub minimum_decimals: usize,
    pub grouping: bool,
    pub percent: bool,
    pub exponent: Option<Exponent>,
}

impl NumberPattern {
    pub fn parse(code: &str) -> Option<Self> {
        let (pattern, percent) = code.strip_suffix('%').map_or((code, false), |p| (p, true));
        let (pattern, exponent) = match pattern.split_once(['E', 'e']) {
            Some((mantissa, suffix)) => {
                let (show_plus, zeros) = match suffix.as_bytes().first()? {
                    b'+' => (true, &suffix[1..]),
                    b'-' => (false, &suffix[1..]),
                    _ => return None,
                };
                if zeros.is_empty() || !zeros.bytes().all(|c| c == b'0') {
                    return None;
                }
                (
                    mantissa,
                    Some(Exponent {
                        digits: zeros.len(),
                        show_plus,
                    }),
                )
            }
            None => (pattern, None),
        };
        let (integer, fraction) = pattern.split_once('.').unwrap_or((pattern, ""));
        let (grouping, integer) = integer
            .strip_prefix("#,##")
            .map_or((false, integer), |digits| (true, digits));
        let mandatory = fraction.trim_end_matches('#');
        if !integer.bytes().all(|c| c == b'0') || !mandatory.bytes().all(|c| c == b'0') {
            return None;
        }
        let parsed = Self {
            integer: integer.len(),
            decimals: fraction.len(),
            minimum_decimals: mandatory.len(),
            grouping,
            percent,
            exponent,
        };
        parsed.is_valid().then_some(parsed)
    }

    pub fn is_valid(self) -> bool {
        (1..=255).contains(&self.integer)
            && self.minimum_decimals <= self.decimals
            && self.decimals <= 127
            && self.exponent.is_none_or(|exp| {
                self.integer == 1
                    && !self.grouping
                    && !self.percent
                    && (1..=12).contains(&exp.digits)
            })
    }

    pub fn code(self) -> String {
        let mut code = if self.grouping {
            "#,##".to_owned()
        } else {
            String::new()
        };
        code.push_str(&"0".repeat(self.integer));
        if self.decimals > 0 {
            code.push('.');
            code.push_str(&"0".repeat(self.minimum_decimals));
            code.push_str(&"#".repeat(self.decimals - self.minimum_decimals));
        }
        if let Some(exponent) = self.exponent {
            code.push_str(if exponent.show_plus { "E+" } else { "E-" });
            code.push_str(&"0".repeat(exponent.digits));
        }
        if self.percent {
            code.push('%');
        }
        code
    }

    pub fn render(self, value: f64) -> String {
        let scaled = if self.percent { value * 100.0 } else { value };
        if !scaled.is_finite() {
            return "#NUM!".into();
        }
        let (rendered, suffix) = if let Some(exponent) = self.exponent {
            let scientific = format!("{:.*E}", self.decimals, scaled.abs());
            let (mantissa, power) = scientific.split_once('E').expect("scientific formatting");
            let power = power.parse::<i32>().expect("finite f64 exponent");
            let sign = if power < 0 {
                "-"
            } else if exponent.show_plus {
                "+"
            } else {
                ""
            };
            let magnitude = power.unsigned_abs();
            let width = exponent.digits;
            (mantissa.to_owned(), format!("E{sign}{magnitude:0width$}"))
        } else {
            (
                format!("{:.*}", self.decimals, scaled.abs()),
                if self.percent {
                    "%".into()
                } else {
                    String::new()
                },
            )
        };
        let (whole, fraction) = rendered.split_once('.').unwrap_or((&rendered, ""));
        let length = fraction
            .trim_end_matches('0')
            .len()
            .max(self.minimum_decimals);
        let fraction = &fraction[..length];
        let mut whole = "0".repeat(self.integer.saturating_sub(whole.len())) + whole;
        if self.grouping {
            let mut i = whole.len();
            while i > 3 {
                i -= 3;
                whole.insert(i, ',');
            }
        }
        let sign = if scaled < 0.0 && rendered.bytes().any(|digit| matches!(digit, b'1'..=b'9')) {
            "-"
        } else {
            ""
        };
        let point = if fraction.is_empty() { "" } else { "." };
        format!("{sign}{whole}{point}{fraction}{suffix}")
    }
}

/// Bounded numeric patterns shared by cell display, Office output and TEXT.
pub(super) fn number_pattern(value: f64, code: &str) -> Option<String> {
    Some(NumberPattern::parse(code)?.render(value))
}
#[cfg(test)]
mod filter_tests {
    use super::*;
    #[test]
    fn filters_summaries_and_saved_rows_use_the_same_presentation() {
        let mut b = Workbook::new(6, 4).unwrap();
        for (r, c, v) in [
            (0, 0, "Value"),
            (1, 0, "=1/3"),
            (2, 0, "=2/3"),
            (0, 1, "=SUBTOTAL(9,A2:A3)"),
        ] {
            b.set_cell(r, c, v.into()).unwrap();
        }
        let filter = crate::SheetFilter {
            range: crate::Range {
                top: 0,
                left: 0,
                bottom: 2,
                right: 0,
            },
            column: 0,
            contains: "0.3333333333".into(),
        };
        b.set_filter(Some(filter.clone())).unwrap();
        let excluded =
            crate::formula::filter_hidden_rows_at(&b, chrono::Local::now().naive_local(), &filter);
        assert_eq!(excluded, std::collections::BTreeSet::from([2]));
        assert_eq!(b.display_grid()[0][1], "0.3333333333");
        assert_eq!(b.calculated_grid()[0][1], "0.3333333333333333");
        let bytes = b.xlsx_bytes().unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        use std::io::Read;
        archive
            .by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        let doc = roxmltree::Document::parse(&text).unwrap();
        for (row, hidden) in [("2", false), ("3", true)] {
            let node = doc
                .descendants()
                .find(|n| n.has_tag_name("row") && n.attribute("r") == Some(row))
                .unwrap();
            assert_eq!(node.attribute("hidden") == Some("1"), hidden);
        }
        b.set_cell(1, 0, "=1E-100".into()).unwrap();
        b.set_filter(Some(crate::SheetFilter {
            contains: "1E-100".into(),
            ..filter
        }))
        .unwrap();
        let mut view = b.sheet_view().clone();
        view.hidden_rows.insert(1);
        b.set_sheet_view(view).unwrap();
        let saved = b.xlsx_bytes().unwrap();
        let mut input = zip::ZipArchive::new(std::io::Cursor::new(saved)).unwrap();
        let mut output = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for index in 0..input.len() {
            let entry = input.by_index(index).unwrap();
            if entry.name() != "xls42/project.json" {
                output.raw_copy_file(entry).unwrap();
            }
        }
        let plain = output.finish().unwrap().into_inner();
        let reopened = Workbook::from_office_bytes(&plain, "xlsx").unwrap();
        assert_eq!(
            reopened.sheet_view().hidden_rows,
            std::collections::BTreeSet::from([1])
        );
        assert_eq!(reopened.display_grid()[0][1], "1E-100");
    }
}
