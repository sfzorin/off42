//! Unicode delimiter splitting with bounded work, shape and padding replication.
use super::*;
fn delimiters(x: &Args<'_>, index: usize) -> Result<Vec<String>> {
    if !x.has(index) {
        return Ok(Vec::new());
    }
    x.grid(index)?
        .values
        .into_iter()
        .map(|v| match v {
            Value::Error(e) => Err(e),
            v => Ok(v.display()),
        })
        .collect()
}
fn split(
    text: &str,
    delimiters: &[String],
    ignore: bool,
    ci: bool,
    steps: &mut usize,
) -> Result<Vec<String>> {
    let chars: Vec<_> = text.chars().collect();
    let delims: Vec<Vec<char>> = delimiters
        .iter()
        .filter(|d| !d.is_empty())
        .map(|d| d.chars().collect())
        .collect();
    if delims.is_empty() {
        return Ok(vec![text.into()]);
    }
    let mut parts = Vec::new();
    let (mut start, mut pos) = (0, 0);
    while pos < chars.len() {
        let mut best = 0;
        for d in &delims {
            charge(steps, d.len())?;
            if pos + d.len() <= chars.len()
                && d.iter().zip(&chars[pos..pos + d.len()]).all(|(a, b)| {
                    if ci {
                        a.to_lowercase().eq(b.to_lowercase())
                    } else {
                        a == b
                    }
                })
            {
                best = best.max(d.len());
            }
        }
        if best == 0 {
            pos += 1;
            continue;
        }
        if !ignore || start != pos {
            parts.push(chars[start..pos].iter().collect());
        }
        if parts.len() >= MAX_CELLS {
            return Err("#LIMIT!");
        }
        pos += best;
        start = pos;
    }
    if !ignore || start != chars.len() {
        parts.push(chars[start..].iter().collect());
    }
    Ok(parts)
}
pub(super) fn calculate(x: &Args<'_>, steps: &mut usize) -> Result<Value> {
    x.count(2, 6)?;
    let text = match x.scalar(0)? {
        Value::Error(e) => return Err(e),
        v => v.display(),
    };
    let cols = delimiters(x, 1)?;
    let rows = delimiters(x, 2)?;
    if cols.iter().chain(&rows).all(String::is_empty) {
        return Err("#VALUE!");
    }
    let ignore = x.boolean(3, false)?;
    let mode = x.integer(4, Some(0))?;
    if ![0, 1].contains(&mode) {
        return Err("#VALUE!");
    }
    let pad = if x.has(5) {
        x.scalar(5)?
    } else {
        Value::Error("#N/A")
    };
    let lines = split(&text, &rows, ignore, mode == 1, steps)?;
    if lines.is_empty() {
        return Err("#CALC!");
    }
    let parts: Vec<Vec<String>> = lines
        .iter()
        .map(|r| split(r, &cols, ignore, mode == 1, steps))
        .collect::<Result<_>>()?;
    let width = parts.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let count = size(parts.len(), width)?;
    charge(steps, count)?;
    let mut bytes = 0;
    for row in &parts {
        for s in row {
            text_budget(&mut bytes, s.len())?;
        }
        text_budget(
            &mut bytes,
            text_length(&pad)
                .checked_mul(width - row.len())
                .ok_or("#LIMIT!")?,
        )?;
    }
    let height = parts.len();
    let mut values = Vec::with_capacity(count);
    for row in parts {
        let len = row.len();
        values.extend(row.into_iter().map(Value::Text));
        values.extend(std::iter::repeat_n(pad.clone(), width - len));
    }
    Ok(Grid::new(height, width, values)?.value())
}
#[cfg(test)]
mod tests {
    use crate::Workbook;
    #[test]
    fn split_multiple_unicode_delimiters_and_typed_padding() {
        let mut b = Workbook::new(12, 8).unwrap();
        b.set_cell(
            0,
            0,
            "=TEXTSPLIT(\"αXβxγ;δ\",{\"x\",\"XX\"},\";\",FALSE,1,99)".into(),
        )
        .unwrap();
        let grid = b.display_grid();
        assert_eq!(&grid[0][..3], &["α", "β", "γ"]);
        assert_eq!(&grid[1][..3], &["δ", "99", "99"]);
        for (formula, expected) in [
            ("=COLUMNS(TEXTSPLIT(\"a,,b\",\",\",,TRUE))", "2"),
            ("=TEXTSPLIT(\"x\",\"\")", "#VALUE!"),
            ("=TEXTSPLIT(\"x\",\",\",,,2)", "#VALUE!"),
            ("=TEXTSPLIT(\",\",\",\",,TRUE)", "#N/A"),
        ] {
            b.set_cell(4, 0, formula.into()).unwrap();
            assert_eq!(crate::formula::display_cell(&b, 4, 0), expected, "{formula}");
        }
    }
}
