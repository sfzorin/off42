//! Scalar text functions and row-major text range aggregation.
use super::Value;
type Result<T> = std::result::Result<T, &'static str>;
pub(super) enum Arg {
    Missing,
    Scalar(Value),
    Range(Vec<Value>, Vec<bool>),
}
impl Arg {
    pub(super) fn scalar(&self) -> Result<Value> {
        match self {
            Self::Missing => Ok(Value::Text(String::new())),
            Self::Scalar(Value::Error(e)) => Err(e),
            Self::Scalar(value) => Ok(value.clone()),
            Self::Range(values, _) if values.len() == 1 => match &values[0] {
                Value::Error(e) => Err(e),
                value => Ok(value.clone()),
            },
            Self::Range(_, _) => Err("#VALUE!"),
        }
    }
}
pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "PROPER"
            | "CLEAN"
            | "NUMBERVALUE"
            | "TEXTBEFORE"
            | "TEXTAFTER"
            | "SUBSTITUTE"
            | "REPLACE"
            | "REPT"
            | "FIND"
            | "SEARCH"
            | "CHAR"
            | "UNICHAR"
            | "CODE"
            | "UNICODE"
            | "T"
            | "N"
            | "TEXTJOIN"
            | "CONCAT"
            | "CONCATENATE"
    )
}
fn number(value: Value) -> Result<f64> {
    match value {
        Value::Number(n) => Ok(n),
        Value::Boolean(b) => Ok(f64::from(b)),
        Value::Text(t) => t
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| n.is_finite())
            .ok_or("#VALUE!"),
        Value::Error(e) => Err(e),
    }
}
struct Args<'a>(&'a [Arg]);
impl Args<'_> {
    fn value(&self, index: usize) -> Result<Value> {
        self.0.get(index).ok_or("#VALUE!")?.scalar()
    }
    fn text(&self, index: usize) -> Result<String> {
        Ok(self.value(index)?.display())
    }
    fn integer(&self, index: usize, default: i64) -> Result<i64> {
        match self.0.get(index) {
            None | Some(Arg::Missing) => Ok(default),
            Some(arg) => {
                let n = number(arg.scalar()?)?.trunc();
                if n.abs() > (1u64 << 53) as f64 {
                    Err("#VALUE!")
                } else {
                    Ok(n as i64)
                }
            }
        }
    }
    fn count(&self, low: usize, high: usize) -> Result<()> {
        if (low..=high).contains(&self.0.len()) {
            Ok(())
        } else {
            Err("#VALUE!")
        }
    }
}
fn append(output: &mut String, value: &str) -> Result<()> {
    if output.len().saturating_add(value.len()) > crate::MAX_TEXT {
        return Err("#LIMIT!");
    }
    output.push_str(value);
    Ok(())
}
fn join(args: &[Arg], delimiter: &str, ignore_empty: bool) -> Result<Value> {
    let mut output = String::new();
    let mut first = true;
    for arg in args {
        let values = match arg {
            Arg::Range(values, _) => values.clone(),
            _ => vec![arg.scalar()?],
        };
        for value in values {
            if let Value::Error(e) = value {
                return Err(e);
            }
            let value = value.display();
            if ignore_empty && value.is_empty() {
                continue;
            }
            if !first {
                append(&mut output, delimiter)?;
            }
            append(&mut output, &value)?;
            first = false;
        }
    }
    Ok(Value::Text(output))
}
fn number_value(args: &Args<'_>) -> Result<Value> {
    args.count(1, 3)?;
    let separator = |index, default| match args.0.get(index) {
        None | Some(Arg::Missing) => Ok(default),
        _ => args.text(index)?.chars().next().ok_or("#VALUE!"),
    };
    // The workbook's existing formula/number convention uses '.' and ','.
    // Explicit separators provide locale-independent conversion.
    let decimal = separator(1, '.')?;
    let group = separator(2, ',')?;
    if decimal == group
        || decimal.is_ascii_digit()
        || group.is_ascii_digit()
        || matches!(decimal, '+' | '-' | '%' | 'e' | 'E')
        || matches!(group, '+' | '-' | '%' | 'e' | 'E')
    {
        return Err("#VALUE!");
    }
    let mut text: String = args
        .text(0)?
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let mut percent = 0;
    while text.ends_with('%') {
        text.pop();
        percent += 1;
    }
    if text.is_empty() {
        return Ok(Value::Number(0.));
    }
    let mut normalized = String::new();
    let mut seen_decimal = false;
    for c in text.chars() {
        if c == decimal {
            if seen_decimal {
                return Err("#VALUE!");
            }
            seen_decimal = true;
            normalized.push('.');
        } else if c == group {
            if seen_decimal {
                return Err("#VALUE!");
            }
        } else if c.is_ascii_digit() || matches!(c, '+' | '-' | 'e' | 'E') {
            normalized.push(c);
        } else {
            return Err("#VALUE!");
        }
    }
    let number = normalized.parse::<f64>().map_err(|_| "#VALUE!")?;
    let number = number / 100f64.powi(percent);
    if number.is_finite() {
        Ok(Value::Number(number))
    } else {
        Err("#NUM!")
    }
}
#[derive(Clone)]
enum Token {
    Star,
    Any,
    Literal(String),
}
/// Earliest start of a prefix match. O(pattern × text), with an explicit work cap.
fn search(pattern: &str, text: &str, start: usize) -> Result<usize> {
    let mut chars = pattern.chars();
    let mut tokens = vec![];
    while let Some(c) = chars.next() {
        tokens.push(match c {
            '*' => Token::Star,
            '?' => Token::Any,
            '~' => Token::Literal(chars.next().unwrap_or('~').to_lowercase().collect()),
            c => Token::Literal(c.to_lowercase().collect()),
        });
    }
    let text: Vec<String> = text.chars().map(|c| c.to_lowercase().collect()).collect();
    if tokens.len().saturating_mul(text.len().max(1)) > 1_000_000 {
        return Err("#LIMIT!");
    }
    let mut previous: Vec<Option<usize>> = (0..=text.len())
        .map(|i| (i >= start).then_some(i))
        .collect();
    for token in tokens {
        let mut next = vec![None; text.len() + 1];
        if matches!(token, Token::Star) {
            next[0] = previous[0];
        }
        for j in 1..=text.len() {
            next[j] = match &token {
                Token::Star => previous[j].into_iter().chain(next[j - 1]).min(),
                Token::Any => previous[j - 1],
                Token::Literal(c) if *c == text[j - 1] => previous[j - 1],
                _ => None,
            };
        }
        previous = next;
    }
    previous.into_iter().flatten().min().ok_or("#VALUE!")
}
fn before_after(name: &str, args: &Args<'_>) -> Result<Value> {
    args.count(2, 6)?;
    let text = args.text(0)?;
    let delimiter = args.text(1)?;
    let instance = args.integer(2, 1)?;
    let ignore_case = args.integer(3, 0)?;
    let match_end = args.integer(4, 0)?;
    if instance == 0 || !matches!(ignore_case, 0 | 1) || !matches!(match_end, 0 | 1) {
        return Err("#VALUE!");
    }
    let chars: Vec<char> = text.chars().collect();
    if instance.unsigned_abs() > chars.len().max(1) as u64 {
        return Err("#VALUE!");
    }
    if delimiter.is_empty() {
        return Ok(Value::Text(if (name == "TEXTBEFORE") == (instance > 0) {
            String::new()
        } else {
            text
        }));
    }
    let delimiter: Vec<char> = delimiter.chars().collect();
    if chars.len().saturating_mul(delimiter.len()) > 1_000_000 {
        return Err("#LIMIT!");
    }
    let mut hits = vec![];
    let mut i = 0;
    while i + delimiter.len() <= chars.len() {
        let equal = chars[i..i + delimiter.len()]
            .iter()
            .zip(&delimiter)
            .all(|(a, b)| {
                if ignore_case == 1 {
                    a.to_lowercase().eq(b.to_lowercase())
                } else {
                    a == b
                }
            });
        if equal {
            hits.push((i, i + delimiter.len()));
            i += delimiter.len();
        } else {
            i += 1;
        }
    }
    if match_end == 1 {
        if instance > 0 {
            hits.push((chars.len(), chars.len()));
        } else {
            hits.insert(0, (0, 0));
        }
    }
    let index = if instance > 0 {
        instance - 1
    } else {
        hits.len() as i64 + instance
    };
    let Some(&(left, right)) = usize::try_from(index).ok().and_then(|i| hits.get(i)) else {
        return if args.0.len() == 6 {
            args.value(5)
        } else {
            Err("#N/A")
        };
    };
    Ok(Value::Text(if name == "TEXTBEFORE" {
        chars[..left].iter().collect()
    } else {
        chars[right..].iter().collect()
    }))
}

pub(super) fn call(name: &str, raw: &[Arg]) -> Value {
    let args = Args(raw);
    let result: Result<Value> = (|| {
        if matches!(name, "CONCAT" | "CONCATENATE") {
            args.count(1, 1024)?;
            return join(raw, "", false);
        }
        if name == "TEXTJOIN" {
            args.count(3, 254)?;
            let delimiter = args.text(0)?;
            let ignore = match args.value(1)? {
                Value::Boolean(v) => v,
                Value::Text(t) if t.eq_ignore_ascii_case("true") => true,
                Value::Text(t) if t.eq_ignore_ascii_case("false") => false,
                value => number(value)? != 0.,
            };
            return join(&raw[2..], &delimiter, ignore);
        }
        if name == "NUMBERVALUE" {
            return number_value(&args);
        }
        if matches!(name, "TEXTBEFORE" | "TEXTAFTER") {
            return before_after(name, &args);
        }
        match name {
            "T" | "N" => {
                args.count(1, 1)?;
                Ok(match (name, args.value(0)?) {
                    ("T", Value::Text(t)) => Value::Text(t),
                    ("T", _) => Value::Text(String::new()),
                    ("N", Value::Number(n)) => Value::Number(n),
                    ("N", Value::Boolean(b)) => Value::Number(f64::from(b)),
                    _ => Value::Number(0.),
                })
            }
            "PROPER" | "CLEAN" => {
                args.count(1, 1)?;
                let text = args.text(0)?;
                let result = if name == "CLEAN" {
                    text.chars().filter(|c| *c as u32 >= 32).collect()
                } else {
                    let mut previous_letter = false;
                    let mut out = String::new();
                    for c in text.chars() {
                        if previous_letter {
                            out.extend(c.to_lowercase());
                        } else {
                            out.extend(c.to_uppercase());
                        }
                        previous_letter = c.is_alphabetic();
                    }
                    out
                };
                Ok(Value::Text(result))
            }
            "CHAR" | "UNICHAR" => {
                args.count(1, 1)?;
                let code = args.integer(0, 0)?;
                if code <= 0 || code > if name == "CHAR" { 255 } else { 0x10FFFF } {
                    return Err("#VALUE!");
                }
                // off42's shared reference uses Unicode/Latin-1, not the OS ANSI code page.
                let c = char::from_u32(code as u32).ok_or("#N/A")?;
                Ok(Value::Text(c.to_string()))
            }
            "CODE" | "UNICODE" => {
                args.count(1, 1)?;
                let text = args.text(0)?;
                Ok(Value::Number(
                    text.chars().next().ok_or("#VALUE!")? as u32 as f64
                ))
            }
            "REPT" => {
                args.count(2, 2)?;
                let text = args.text(0)?;
                let count = args.integer(1, 0)?;
                if count < 0
                    || (!text.is_empty() && count as u64 > 32767 / text.chars().count() as u64)
                {
                    return Err("#VALUE!");
                }
                if text.len().saturating_mul(count as usize) > crate::MAX_TEXT {
                    return Err("#LIMIT!");
                }
                Ok(Value::Text(if text.is_empty() {
                    text
                } else {
                    text.repeat(count as usize)
                }))
            }
            "REPLACE" => {
                args.count(4, 4)?;
                let text: Vec<char> = args.text(0)?.chars().collect();
                let start = args.integer(1, 0)?;
                let count = args.integer(2, 0)?;
                let new = args.text(3)?;
                if start < 1 || count < 0 {
                    return Err("#VALUE!");
                }
                let left = ((start - 1) as usize).min(text.len());
                let right = left.saturating_add(count as usize).min(text.len());
                let mut out: String = text[..left].iter().collect();
                append(&mut out, &new)?;
                append(&mut out, &text[right..].iter().collect::<String>())?;
                Ok(Value::Text(out))
            }
            "SUBSTITUTE" => {
                args.count(3, 4)?;
                let text = args.text(0)?;
                let old = args.text(1)?;
                let new = args.text(2)?;
                let occurrence = if raw.len() == 4 {
                    let n = args.integer(3, 0)?;
                    if n <= 0 {
                        return Err("#VALUE!");
                    }
                    Some(n as usize)
                } else {
                    None
                };
                if old.is_empty() {
                    return Ok(Value::Text(text));
                }
                let mut out = String::new();
                let mut position = 0;
                for (index, (start, matched)) in text.match_indices(&old).enumerate() {
                    append(&mut out, &text[position..start])?;
                    append(
                        &mut out,
                        if occurrence.is_none_or(|n| n == index + 1) {
                            &new
                        } else {
                            matched
                        },
                    )?;
                    position = start + matched.len();
                }
                append(&mut out, &text[position..])?;
                Ok(Value::Text(out))
            }
            "FIND" | "SEARCH" => {
                args.count(2, 3)?;
                let needle = args.text(0)?;
                let text = args.text(1)?;
                let start = args.integer(2, 1)?;
                let count = text.chars().count();
                if start < 1 || start as usize > count + 1 {
                    return Err("#VALUE!");
                }
                let start = start as usize - 1;
                let found = if name == "SEARCH" {
                    search(&needle, &text, start)?
                } else {
                    let byte = text
                        .char_indices()
                        .nth(start)
                        .map_or(text.len(), |(i, _)| i);
                    let found = text[byte..].find(&needle).ok_or("#VALUE!")?;
                    start + text[byte..byte + found].chars().count()
                };
                Ok(Value::Number((found + 1) as f64))
            }
            _ => Err("#NAME?"),
        }
    })();
    match result {
        Ok(Value::Text(t)) if t.len() > crate::MAX_TEXT => Value::Error("#LIMIT!"),
        Ok(value) => value,
        Err(error) => Value::Error(error),
    }
}

#[cfg(test)]
mod tests {
    use crate::Workbook;
    #[test]
    fn text_functions_match_reference_examples_and_bounds() {
        let cases = [
            (r#"=TEXTJOIN("-",FALSE,"a","","b")"#, "a--b"),
            (r#"=TEXTBEFORE("john.smith@x.com","@")"#, "john.smith"),
            (r#"=TEXTAFTER("a-b-c","-",2)"#, "c"),
            (r#"=TEXTAFTER("a-b-c","-",-1)"#, "c"),
            (r#"=TEXTBEFORE("a-b-c","-",-1)"#, "a-b"),
            (r#"=TEXTBEFORE("abc","z")"#, "#N/A"),
            (r#"=TEXTBEFORE("abc","z",,,,"none")"#, "none"),
            (r#"=_xlfn.TEXTAFTER("aXbxc","x",1,1)"#, "bxc"),
            (r#"=TEXTBEFORE("abc","z",1,0,1)"#, "abc"),
            (r#"=TEXTAFTER("abc","z",-1,0,1)"#, "abc"),
            (r#"=TEXTAFTER("abc","",-1)"#, ""),
            (r#"=TEXTBEFORE("a","",2)"#, "#VALUE!"),
            (r#"=TEXTBEFORE("abc","b",0)"#, "#VALUE!"),
            (r#"=TEXTBEFORE("abc","b",1,2)"#, "#VALUE!"),
            (r#"=SUBSTITUTE("aaa","a","b",2)"#, "aba"),
            (r#"=SUBSTITUTE("aaaa","aa","b")"#, "bb"),
            (r#"=SUBSTITUTE("aaa","a","b",0)"#, "#VALUE!"),
            (r#"=REPLACE("abcdef",2,3,"X")"#, "aXef"),
            (r#"=REPLACE("abc",99,3,"X")"#, "abcX"),
            (r#"=REPLACE("abc",0,3,"X")"#, "#VALUE!"),
            (r#"=REPT("ab",3.9)"#, "ababab"),
            (r#"=REPT("ab",20000)"#, "#VALUE!"),
            (
                r#"=PROPER("hello WORLD o'neil 2nd-place")"#,
                "Hello World O'Neil 2Nd-Place",
            ),
            (r#"=CLEAN("a"&CHAR(10)&"b")"#, "ab"),
            (r#"=NUMBERVALUE("1.234,5",",",".")"#, "1234.5"),
            (r#"=NUMBERVALUE("12%")"#, "0.12"),
            (r#"=NUMBERVALUE("9%%")"#, "0.0009"),
            (r#"=NUMBERVALUE(" 3 000 ")"#, "3000"),
            (r#"=NUMBERVALUE("1,234.5,6")"#, "#VALUE!"),
            (r#"=NUMBERVALUE("1.2.3")"#, "#VALUE!"),
            (r#"=NUMBERVALUE("NaN")"#, "#VALUE!"),
            (r#"=UNICODE("Ж")"#, "1046"),
            (r#"=UNICHAR(8364)"#, "€"),
            (r#"=UNICHAR(128512)"#, "😀"),
            (r#"=UNICHAR(0)"#, "#VALUE!"),
            (r#"=UNICHAR(55296)"#, "#N/A"),
            (r#"=CHAR(256)"#, "#VALUE!"),
            (r#"=FIND("b","abcb",3)"#, "4"),
            (r#"=SEARCH("B","abcb")"#, "2"),
            (r#"=SEARCH("?Л*А","xxПЛАТА")"#, "3"),
            (r#"=SEARCH("~*","a*b")"#, "2"),
            (r#"=SEARCH("x?z","xAz")"#, "1"),
            (r#"=FIND("A","a")"#, "#VALUE!"),
            (r#"=CODE("A")"#, "65"),
            (r#"=CHAR(65)"#, "A"),
            (r#"=T(5)"#, ""),
            (r#"=N("x")"#, "0"),
            (r#"=N(TRUE)"#, "1"),
            (r#"=N(1/0)"#, "#DIV/0!"),
            (r#"=T(1/0)"#, "#DIV/0!"),
        ];
        let mut workbook = Workbook::new(cases.len(), 2).unwrap();
        for (row, (formula, _)) in cases.iter().enumerate() {
            workbook.set_cell(row, 0, formula.to_string()).unwrap();
        }
        let grid = workbook.display_grid();
        for (row, (formula, expected)) in cases.iter().enumerate() {
            assert_eq!(&grid[row][0], expected, "{formula}");
        }
    }
    #[test]
    fn range_text_keeps_blanks_types_qualified_names_and_recalculates_after_edit_and_reopen() {
        let mut book = Workbook::new(10, 8).unwrap();
        book.set_cell(0, 0, "board".into()).unwrap();
        book.set_cell(1, 0, "".into()).unwrap();
        book.set_cell(2, 0, "0".into()).unwrap();
        book.set_cell(3, 0, "TRUE".into()).unwrap();
        book.set_cell(0, 1, r#"=TEXTJOIN("-",FALSE,A1:A4)"#.into())
            .unwrap();
        book.set_cell(1, 1, r#"=TEXTJOIN("-",TRUE,A1:A4)"#.into())
            .unwrap();
        book.set_cell(2, 1, r#"=CONCAT('Sheet1'!A1:A4)"#.into())
            .unwrap();
        book.set_cell(3, 1, r#"=TEXTJOIN("-",FALSE,A2,A3)"#.into())
            .unwrap();
        let grid = book.display_grid();
        assert_eq!(grid[0][1], "board--0-TRUE");
        assert_eq!(grid[1][1], "board-0-TRUE");
        assert_eq!(grid[2][1], "board0TRUE");
        assert_eq!(grid[3][1], "-0");
        for format in ["xlsx", "ods"] {
            let bytes = if format == "xlsx" {
                book.xlsx_bytes()
            } else {
                book.ods_bytes()
            }
            .unwrap();
            let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut external = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for index in 0..archive.len() {
                let entry = archive.by_index(index).unwrap();
                if entry.name() != "xls42/project.json" {
                    external.raw_copy_file(entry).unwrap();
                }
            }
            let bytes = external.finish().unwrap().into_inner();
            let mut reopened = Workbook::from_office_bytes(&bytes, format).unwrap();
            reopened.set_cell(0, 0, "changed".into()).unwrap();
            assert_eq!(reopened.display_grid()[0][1], "changed--0-TRUE");
            assert!(reopened.undo());
            assert_eq!(reopened.display_grid()[0][1], "board--0-TRUE");
            assert!(reopened.redo());
            assert_eq!(reopened.display_grid()[1][1], "changed-0-TRUE");
        }
    }
}
