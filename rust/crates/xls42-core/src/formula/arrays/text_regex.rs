//! Bounded Unicode regular expressions. Unsupported look-around/backreferences return #VALUE!.
use super::*;
use regex::{Captures, RegexBuilder};
fn text(x: &Args<'_>, i: usize) -> Result<String> {
    match x.scalar(i)? {
        Value::Error(e) => Err(e),
        v => Ok(v.display()),
    }
}
fn append(out: &mut String, value: &str) -> Result<()> {
    if out.len().saturating_add(value.len()) > crate::MAX_TEXT {
        return Err("#LIMIT!");
    }
    out.push_str(value);
    Ok(())
}
fn replacement(caps: &Captures<'_>, template: &str) -> Result<String> {
    let mut out = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(c) = chars.next() {
                append(&mut out, &c.to_string())?;
            } else {
                append(&mut out, "\\")?;
            }
        } else if c == '$' && chars.peek().is_some_and(char::is_ascii_digit) {
            let mut number = 0usize;
            while chars.peek().is_some_and(char::is_ascii_digit) {
                number = number
                    .saturating_mul(10)
                    .saturating_add(chars.next().unwrap().to_digit(10).unwrap() as usize);
            }
            if let Some(value) = caps.get(number) {
                append(&mut out, value.as_str())?;
            }
        } else {
            append(&mut out, &c.to_string())?;
        }
    }
    Ok(out)
}
pub(super) fn calculate(name: &str, x: &Args<'_>, steps: &mut usize) -> Result<Value> {
    x.count(
        if name == "REGEXREPLACE" { 3 } else { 2 },
        match name {
            "REGEXTEST" => 3,
            "REGEXREPLACE" => 5,
            _ => 4,
        },
    )?;
    let input = text(x, 0)?;
    let pattern = text(x, 1)?;
    if pattern.is_empty() {
        return Err("#VALUE!");
    }
    let ci = match name {
        "REGEXTEST" => x.integer(2, Some(0))? == 1,
        "REGEXEXTRACT" => x.integer(3, Some(0))? == 1,
        "REGEXREPLACE" => x.integer(4, Some(0))? == 1,
        _ => false,
    };
    charge(
        steps,
        pattern.len().max(1).saturating_mul(input.len().max(1)),
    )?;
    let re = RegexBuilder::new(&pattern)
        .case_insensitive(ci)
        .size_limit(8 * 1024 * 1024)
        .dfa_size_limit(2 * 1024 * 1024)
        .nest_limit(128)
        .build()
        .map_err(|_| "#VALUE!")?;
    if name == "REGEXTEST" {
        return Ok(Value::Boolean(re.is_match(&input)));
    }
    let mut matches = Vec::new();
    let mut start = 0;
    while start <= input.len() {
        charge(steps, input.len().saturating_sub(start).max(1))?;
        let Some(cap) = re.captures_at(&input, start) else {
            break;
        };
        let found = cap.get(0).unwrap();
        let end = found.end();
        let empty = found.start() == end;
        matches.push(cap);
        if matches.len() > MAX_CELLS {
            return Err("#LIMIT!");
        }
        if empty {
            if end == input.len() {
                break;
            }
            start = end + input[end..].chars().next().unwrap().len_utf8();
        } else {
            start = end;
        }
    }
    if name == "REGEXEXTRACT" {
        let Some(first) = matches.first() else {
            return Err("#N/A");
        };
        let mode = x.integer(2, Some(0))?;
        let (rows, cols, values) = match mode {
            0 => (1, 1, vec![Value::Text(first[0].into())]),
            1 => (
                matches.len(),
                1,
                matches.iter().map(|c| Value::Text(c[0].into())).collect(),
            ),
            2 => {
                if first.len() < 2 {
                    return Err("#VALUE!");
                }
                (
                    1,
                    first.len() - 1,
                    (1..first.len())
                        .map(|i| Value::Text(first.get(i).map_or("", |m| m.as_str()).into()))
                        .collect(),
                )
            }
            _ => return Err("#VALUE!"),
        };
        return Ok(Grid::new(rows, cols, values)?.value());
    }
    let mut global = name == "REGEXREPLACE";
    let mut occurrence = if global { 0 } else { 1 };
    if name == "REGEX" && x.has(3) {
        match x.scalar(3)? {
            Value::Text(flags) => global = flags.contains('g'),
            _ => occurrence = x.integer(3, Some(1))?,
        }
    } else if name == "REGEXREPLACE" {
        occurrence = x.integer(3, Some(0))?;
        global = occurrence == 0;
    }
    let index = if occurrence > 0 {
        usize::try_from(occurrence - 1).ok()
    } else if name == "REGEXREPLACE" && occurrence < 0 {
        matches
            .len()
            .checked_sub(occurrence.unsigned_abs() as usize)
    } else {
        None
    };
    if name == "REGEX" && !x.has(2) {
        return index
            .and_then(|i| matches.get(i))
            .map(|c| Value::Text(c[0].into()))
            .ok_or("#N/A");
    }
    let template = text(x, 2)?;
    let mut out = String::new();
    let mut from = 0;
    for (i, cap) in matches.iter().enumerate() {
        if !global && occurrence != 0 && Some(i) != index {
            continue;
        }
        let found = cap.get(0).unwrap();
        append(&mut out, &input[from..found.start()])?;
        append(&mut out, &replacement(cap, &template)?)?;
        from = found.end();
    }
    append(&mut out, &input[from..])?;
    Ok(Value::Text(out))
}
#[cfg(test)]
mod tests {
    use crate::{formula, Workbook};
    #[test]
    fn unicode_captures_replacements_occurrences_and_empty_matches() {
        let mut b = Workbook::new(12, 6).unwrap();
        for (f, e) in [
            (r#"=REGEX("abc123def","[0-9]+")"#, "123"),
            (r##"=REGEX("a1b2c3","[0-9]","#","g")"##, "a#b#c#"),
            (r#"=REGEX("a1b2c3","[0-9]",,2)"#, "2"),
            (r#"=REGEX("abc","z")"#, "#N/A"),
            (r#"=REGEXTEST("Жук","ж",1)"#, "TRUE"),
            (r#"=REGEXREPLACE("a1b2","([a-z])([0-9])","$2$1")"#, "1a2b"),
            (r#"=REGEXREPLACE("a1b2c3","[0-9]","X",-2)"#, "a1bXc3"),
            (r#"=REGEXREPLACE("αβ","","!")"#, "#VALUE!"),
            (r#"=REGEXEXTRACT("abc","z")"#, "#N/A"),
            (r#"=REGEX("a","[")"#, "#VALUE!"),
            (r#"=REGEX("aa","(a)\1")"#, "#VALUE!"),
        ] {
            b.set_cell(8, 0, f.into()).unwrap();
            assert_eq!(formula::display_cell(&b, 8, 0), e, "{f}");
        }
        b.set_cell(0, 0, r#"=REGEXEXTRACT("a12b34","[0-9]+",1)"#.into())
            .unwrap();
        b.set_cell(0, 2, r#"=REGEXEXTRACT("Ж12","(Ж)([0-9]+)",2)"#.into())
            .unwrap();
        let g = b.display_grid();
        assert_eq!(&g[0][..4], &["12", "", "Ж", "12"]);
        assert_eq!(g[1][0], "34");
    }
}
