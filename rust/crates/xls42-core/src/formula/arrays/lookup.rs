use super::super::{lookup_equal, lookup_order, wildcard};
use super::*;
fn find(
    needle: &Value,
    hay: &Grid,
    mode: i64,
    search: i64,
    steps: &mut usize,
) -> Result<Option<usize>> {
    if let Value::Error(e) = needle {
        return Err(e);
    }
    if ![-1, 0, 1, 2].contains(&mode) || ![-2, -1, 1, 2].contains(&search) {
        return Err("#VALUE!");
    }
    let mut best: Option<(usize, Value)> = None;
    for offset in 0..hay.values.len() {
        let i = if search < 0 {
            hay.values.len() - 1 - offset
        } else {
            offset
        };
        charge(steps, 1)?;
        if hay.blanks[i] {
            continue;
        }
        let value = &hay.values[i];
        if mode == 2 {
            if let (Value::Text(a), Value::Text(b)) = (needle, value) {
                charge(steps, a.len().saturating_mul(b.len()))?;
            }
        }
        if lookup_equal(needle, value)
            || mode == 2 && matches!((needle,value),(Value::Text(a),Value::Text(b))if wildcard(a,b))
        {
            return Ok(Some(i));
        }
        if matches!(mode, -1 | 1)
            && lookup_order(value, needle)
                .is_some_and(|o| if mode < 0 { o.is_lt() } else { o.is_gt() })
            && best.as_ref().is_none_or(|(_, b)| {
                lookup_order(value, b).is_some_and(|o| if mode < 0 { o.is_gt() } else { o.is_lt() })
            })
        {
            best = Some((i, value.clone()));
        }
    }
    Ok(best.map(|(i, _)| i))
}
pub(super) fn call(name: &str, x: &Args<'_>, steps: &mut usize) -> Option<Result<Value>> {
    if !matches!(name, "HLOOKUP" | "XMATCH" | "LOOKUP") {
        return None;
    }
    Some((|| {
        if name == "XMATCH" {
            x.count(2, 4)?;
            let needles = x.grid(0)?;
            let hay = x.grid(1)?;
            if hay.rows > 1 && hay.cols > 1 {
                return Err("#VALUE!");
            }
            let mode = x.integer(2, Some(0))?;
            let search = x.integer(3, Some(1))?;
            let mut values = Vec::new();
            for needle in &needles.values {
                values.push(match find(needle, &hay, mode, search, steps) {
                    Ok(Some(i)) => finite((i + 1) as f64),
                    Ok(None) => Value::Error("#N/A"),
                    Err(e) => Value::Error(e),
                });
            }
            return Ok(if values.len() == 1 {
                values.remove(0)
            } else {
                Grid::new(needles.rows, needles.cols, values)?.value()
            });
        }
        if name == "HLOOKUP" {
            x.count(3, 4)?;
            let needle = x.scalar(0)?;
            let table = x.grid(1)?;
            let row = x.integer(2, None)?;
            if row < 1 {
                return Err("#VALUE!");
            }
            if row > table.rows as i64 {
                return Err("#REF!");
            }
            let hay = table.select(&[0], &(0..table.cols).collect::<Vec<_>>())?;
            let approximate = x.boolean(3, true)?;
            let Some(i) = find(&needle, &hay, if approximate { -1 } else { 2 }, 1, steps)? else {
                return Err("#N/A");
            };
            return Ok(table.at(row as usize - 1, i));
        }
        x.count(2, 3)?;
        let needle = x.scalar(0)?;
        if let Value::Error(e) = needle {
            return Err(e);
        }
        let matrix = x.grid(1)?;
        let (hay, result) = if x.has(2) {
            (matrix.values, x.grid(2)?.values)
        } else if matrix.cols > matrix.rows {
            (
                (0..matrix.cols).map(|c| matrix.at(0, c)).collect(),
                (0..matrix.cols)
                    .map(|c| matrix.at(matrix.rows - 1, c))
                    .collect(),
            )
        } else {
            (
                (0..matrix.rows).map(|r| matrix.at(r, 0)).collect(),
                (0..matrix.rows)
                    .map(|r| matrix.at(r, matrix.cols - 1))
                    .collect(),
            )
        };
        let mut best: Option<usize> = None;
        for (i, v) in hay.iter().enumerate() {
            charge(steps, 1)?;
            if lookup_order(v, &needle).is_some_and(|o| !o.is_gt())
                && best.is_none_or(|b| lookup_order(v, &hay[b]).is_some_and(|o| !o.is_lt()))
            {
                best = Some(i);
            }
        }
        best.and_then(|i| result.get(i).cloned()).ok_or("#N/A")
    })())
}
