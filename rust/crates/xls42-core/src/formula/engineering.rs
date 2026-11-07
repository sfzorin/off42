//! Scalar engineering functions; bounds match the suite's macOS implementation.
//! Reference: https://support.microsoft.com/en-us/excel/engineering-functions-reference
use super::{finite, Value};
type Result<T> = std::result::Result<T, &'static str>;
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "DEC2BIN"
            | "DEC2OCT"
            | "DEC2HEX"
            | "BIN2DEC"
            | "BIN2OCT"
            | "BIN2HEX"
            | "OCT2DEC"
            | "OCT2BIN"
            | "OCT2HEX"
            | "HEX2DEC"
            | "HEX2BIN"
            | "HEX2OCT"
            | "BASE"
            | "DECIMAL"
            | "BITAND"
            | "BITOR"
            | "BITXOR"
            | "BITLSHIFT"
            | "BITRSHIFT"
            | "DELTA"
            | "GESTEP"
            | "CONVERT"
    )
}
fn numeric(value: &Value) -> Result<f64> {
    let number = match value {
        Value::Number(v) => *v,
        Value::Boolean(v) => f64::from(*v),
        Value::Text(v) => v.trim().parse().map_err(|_| "#VALUE!")?,
        Value::Array(_) => return Err("#VALUE!"),
        Value::Error(e) => return Err(e),
    };
    if number.is_finite() {
        Ok(number)
    } else {
        Err("#NUM!")
    }
}
fn string(value: &Value) -> String {
    value.clone().display()
}
fn radix_text(mut value: u64, radix: u32, places: usize) -> String {
    let digits = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    let mut buffer = vec![];
    loop {
        buffer.push(digits[(value % u64::from(radix)) as usize]);
        value /= u64::from(radix);
        if value == 0 {
            break;
        }
    }
    buffer.resize(buffer.len().max(places), b'0');
    buffer.reverse();
    String::from_utf8(buffer).unwrap()
}
fn from_base(kind: &str) -> (u32, u32) {
    match kind {
        "BIN" => (2, 10),
        "OCT" => (8, 30),
        _ => (16, 40),
    }
}
fn decode(value: &Value, radix: u32, bits: u32) -> Result<i64> {
    let text = string(value);
    let text = text.trim();
    if text.len() > 10 || !text.bytes().all(|b| char::from(b).is_digit(radix)) {
        return Err("#NUM!");
    }
    let raw = if text.is_empty() {
        0
    } else {
        u64::from_str_radix(text, radix).map_err(|_| "#NUM!")?
    };
    if text.len() == 10 && raw >= 1 << (bits - 1) {
        Ok(raw as i64 - (1i64 << bits))
    } else {
        Ok(raw as i64)
    }
}
fn encode(value: i64, radix: u32, bits: u32, places: Option<&Value>) -> Result<Value> {
    let limit = 1i64 << (bits - 1);
    if value < -limit || value >= limit {
        return Err("#NUM!");
    }
    if value < 0 {
        return Ok(Value::Text(radix_text(
            ((1i64 << bits) + value) as u64,
            radix,
            10,
        )));
    }
    let minimum = radix_text(value as u64, radix, 0).len();
    let places = if let Some(places) = places {
        let n = numeric(places)?.trunc();
        if n < minimum as f64 || n > 10. {
            return Err("#NUM!");
        }
        n as usize
    } else {
        0
    };
    Ok(Value::Text(radix_text(value as u64, radix, places)))
}
pub(super) fn call(name: &str, args: &[Value]) -> Value {
    calculate(name, args).unwrap_or_else(Value::Error)
}
fn calculate(name: &str, args: &[Value]) -> Result<Value> {
    if name == "CONVERT" {
        let [number, from, to] = args else {
            return Err("#VALUE!");
        };
        return super::units::convert(numeric(number)?, &string(from), &string(to))
            .map(finite)
            .ok_or("#N/A");
    }
    if matches!(name, "BASE" | "DECIMAL") {
        if args.len() < 2 || args.len() > if name == "BASE" { 3 } else { 2 } {
            return Err("#VALUE!");
        }
        let radix = numeric(&args[1])?.trunc();
        if !(2.0..=36.0).contains(&radix) {
            return Err("#NUM!");
        }
        if name == "DECIMAL" {
            let text = string(&args[0]);
            let text = text.trim();
            if text.len() > 255 {
                return Err("#NUM!");
            }
            let text = text.strip_prefix('+').unwrap_or(text);
            let mut value = 0.;
            for character in text.chars() {
                let digit = character.to_digit(radix as u32).ok_or("#NUM!")?;
                value = value * radix + f64::from(digit);
            }
            return Ok(finite(value));
        }
        let value = numeric(&args[0])?.trunc();
        let places = args.get(2).map(numeric).transpose()?.unwrap_or(0.).trunc();
        if !(0.0..9_007_199_254_740_992.0).contains(&value) || !(0.0..=255.0).contains(&places) {
            return Err("#NUM!");
        }
        return Ok(Value::Text(radix_text(
            value as u64,
            radix as u32,
            places as usize,
        )));
    }
    if name.contains('2') {
        let (source, target) = name.split_once('2').ok_or("#VALUE!")?;
        if args.is_empty() || args.len() > if target == "DEC" { 1 } else { 2 } {
            return Err("#VALUE!");
        }
        let value = if source == "DEC" {
            let value = numeric(&args[0])?.trunc();
            if !(-549_755_813_888.0..549_755_813_888.0).contains(&value) {
                return Err("#NUM!");
            }
            value as i64
        } else {
            let (radix, bits) = from_base(source);
            decode(&args[0], radix, bits)?
        };
        if target == "DEC" {
            return Ok(Value::Number(value as f64));
        }
        let (radix, bits) = from_base(target);
        return encode(value, radix, bits, args.get(1));
    }
    if matches!(name, "DELTA" | "GESTEP") {
        if args.is_empty() || args.len() > 2 {
            return Err("#VALUE!");
        }
        let a = numeric(&args[0])?;
        let b = args.get(1).map(numeric).transpose()?.unwrap_or(0.);
        return Ok(Value::Number(f64::from(if name == "DELTA" {
            a == b
        } else {
            a >= b
        })));
    }
    let [left, right] = args else {
        return Err("#VALUE!");
    };
    let a = numeric(left)?;
    let b = numeric(right)?;
    let limit = 1u64 << 48;
    if a < 0. || a >= limit as f64 || a.trunc() != a {
        return Err("#NUM!");
    }
    let a = a as u64;
    let value = if matches!(name, "BITLSHIFT" | "BITRSHIFT") {
        let shift = b.trunc();
        if shift.abs() > 53. {
            return Err("#NUM!");
        }
        let shift = if name == "BITLSHIFT" {
            shift as i32
        } else {
            -(shift as i32)
        };
        let value = if shift >= 0 {
            u128::from(a) << shift
        } else {
            u128::from(a) >> -shift
        };
        if value >= u128::from(limit) {
            return Err("#NUM!");
        }
        value as u64
    } else {
        if b < 0. || b >= limit as f64 || b.trunc() != b {
            return Err("#NUM!");
        }
        let b = b as u64;
        match name {
            "BITAND" => a & b,
            "BITOR" => a | b,
            "BITXOR" => a ^ b,
            _ => return Err("#VALUE!"),
        }
    };
    Ok(Value::Number(value as f64))
}
#[cfg(test)]
mod tests {
    use crate::Workbook;
    #[test]
    fn workbook_engineering_functions_match_reference_examples_and_bounds() {
        let cases = [
            ("=DEC2BIN(9,4)", "1001"),
            ("=DEC2BIN(-100)", "1110011100"),
            ("=DEC2OCT(-1)", "7777777777"),
            ("=DEC2HEX(-54)", "FFFFFFFFCA"),
            ("=DEC2HEX(100,4)", "0064"),
            ("=DEC2HEX(64,1)", "#NUM!"),
            ("=DEC2BIN(512)", "#NUM!"),
            ("=DEC2HEX(549755813888)", "#NUM!"),
            ("=DEC2HEX(255.9)", "FF"),
            ("=DEC2HEX(0,0)", "#NUM!"),
            ("=BIN2DEC(1111111111)", "-1"),
            ("=BIN2OCT(1111111111)", "7777777777"),
            ("=BIN2HEX(11111011,4)", "00FB"),
            ("=OCT2DEC(7777777777)", "-1"),
            ("=OCT2BIN(10,4)", "1000"),
            ("=OCT2HEX(17)", "F"),
            ("=HEX2DEC(\"ffffffffff\")", "-1"),
            ("=HEX2BIN(\"FFFFFFFFFF\")", "1111111111"),
            ("=HEX2OCT(\"1FF\")", "777"),
            ("=HEX2BIN(\"200\")", "#NUM!"),
            ("=HEX2DEC(\"NO\")", "#NUM!"),
            ("=BIN2DEC(102)", "#NUM!"),
            ("=BASE(31,16,4)", "001F"),
            ("=BASE(9007199254740992,2)", "#NUM!"),
            ("=DECIMAL(\"zap\",36)", "45745"),
            ("=DECIMAL(111,2)", "7"),
            ("=BITAND(13,25)", "9"),
            ("=BITOR(13,25)", "29"),
            ("=BITXOR(13,25)", "20"),
            ("=BITLSHIFT(4,2)", "16"),
            ("=BITLSHIFT(16,-2)", "4"),
            ("=BITRSHIFT(4,-2)", "16"),
            ("=BITRSHIFT(281474976710655,48)", "0"),
            ("=BITLSHIFT(0,53)", "0"),
            ("=BITLSHIFT(281474976710655,1)", "#NUM!"),
            ("=BITAND(0.5,1)", "#NUM!"),
            ("=BITAND(-1,1)", "#NUM!"),
            ("=BITRSHIFT(1,54)", "#NUM!"),
            ("=DELTA(1,1)", "1"),
            ("=DELTA(1)", "0"),
            ("=GESTEP(-1)", "0"),
            ("=GESTEP(1,1)", "1"),
            ("=CONVERT(1,\"Mibyte\",\"bit\")", "8388608"),
            ("=CONVERT(1,\"m\",\"g\")", "#N/A"),
            ("=CONVERT(1,\"MiW\",\"W\")", "#N/A"),
            ("=DEC2HEX(1/0)", "#DIV/0!"),
            ("=IFERROR(HEX2BIN(\"200\"),\"too large\")", "too large"),
            ("=_xlfn.DEC2HEX(15)", "F"),
            ("=SUM(DEC2HEX(15)=\"F\",3)", "4"),
        ];
        let mut book = Workbook::new(cases.len(), 2).unwrap();
        for (row, (formula, _)) in cases.iter().enumerate() {
            book.set_cell(row, 0, (*formula).into()).unwrap();
        }
        let displayed = book.display_grid();
        for (row, (formula, expected)) in cases.iter().enumerate() {
            assert_eq!(displayed[row][0], *expected, "{formula}");
        }
    }
    #[test]
    fn scalar_boolean_and_numeric_text_coercion_do_not_change_range_aggregation() {
        let mut book = Workbook::new(10, 2).unwrap();
        book.set_cell(0, 0, "TRUE".into()).unwrap();
        book.set_cell(1, 0, "=TRUE()".into()).unwrap();
        let cases = [
            ("=SUM(A1:A2)", "0"),
            ("=SUM(TRUE,3)", "4"),
            ("=SUM(\"3\",3)", "6"),
            ("=COUNT(\"abc\",\"3\",TRUE)", "2"),
            ("=COUNTA(\"\",\"x\",FALSE)", "3"),
            ("=SUM(\"abc\")", "#VALUE!"),
            ("=COUNT(A1:A2)", "0"),
            ("=COUNTA(A1:A2)", "2"),
            ("=SIN(\"0\")", "0"),
        ];
        for (row, (formula, _)) in cases.iter().enumerate() {
            book.set_cell(row, 1, (*formula).into()).unwrap();
        }
        let displayed = book.display_grid();
        for (row, (formula, expected)) in cases.iter().enumerate() {
            assert_eq!(displayed[row][1], *expected, "{formula}");
        }
    }
    #[test]
    fn functions_with_digits_keep_references_and_saved_formulas_intact() {
        let mut book = Workbook::new(20, 8).unwrap();
        book.set_cell(0, 0, "255".into()).unwrap();
        book.set_cell(0, 1, "=DEC2HEX($A$1,4)".into()).unwrap();
        book.set_cell(1, 1, "=HEX2DEC(B1)".into()).unwrap();
        book.set_cell(2, 1, "=A1+1".into()).unwrap();
        book.set_cell(3, 1, "=SUM(A1,1)".into()).unwrap();
        book.set_cell(4, 1, "=_xlfn._xlws.BITAND(A1,15)".into())
            .unwrap();
        let values = book.display_grid();
        assert_eq!(values[0][1], "00FF");
        assert_eq!(values[1][1], "255");
        assert_eq!(values[2][1], "256");
        assert_eq!(values[3][1], "256");
        assert_eq!(values[4][1], "15");
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" {
                book.xlsx_bytes().unwrap()
            } else {
                book.ods_bytes().unwrap()
            };
            let reopened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(reopened.cell(0, 1), Some("=DEC2HEX($A$1,4)"));
            assert_eq!(reopened.display_grid(), values);
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&bytes)).unwrap();
            let mut external = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for index in 0..archive.len() {
                let entry = archive.by_index(index).unwrap();
                if entry.name() != "xls42/project.json" {
                    external.raw_copy_file(entry).unwrap();
                }
            }
            let external = external.finish().unwrap().into_inner();
            let mut independent = Workbook::from_office_bytes(&external, kind).unwrap();
            assert_eq!(
                independent.display_grid(),
                values,
                "{kind}: external formula import"
            );
            independent.set_cell(0, 0, "16".into()).unwrap();
            let recalculated = independent.display_grid();
            for (row, expected) in ["0010", "16", "17", "17", "0"].into_iter().enumerate() {
                assert_eq!(
                    recalculated[row][1], expected,
                    "{kind}: recomputation must not use saved cached results"
                );
            }
        }
    }
}
