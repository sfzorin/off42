use super::*;
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Number(u64),
    Text(String),
    Boolean(bool),
    Error(&'static str),
    Blank,
}
fn key(m: &Grid, r: usize, c: usize) -> Key {
    let i = r * m.cols + c;
    if m.blanks[i] {
        return Key::Blank;
    }
    match &m.values[i] {
        Value::Number(n) => Key::Number(if *n == 0.0 { 0 } else { n.to_bits() }),
        Value::Text(s) => Key::Text(s.to_lowercase()),
        Value::Boolean(b) => Key::Boolean(*b),
        Value::Error(e) => Key::Error(e),
        Value::Array(_) => unreachable!(),
    }
}
fn order(a: &Key, b: &Key) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let rank = |k: &Key| match k {
        Key::Number(_) => 0,
        Key::Text(_) => 1,
        Key::Boolean(_) => 2,
        Key::Error(_) => 3,
        Key::Blank => 4,
    };
    match (a, b) {
        (Key::Number(a), Key::Number(b)) => f64::from_bits(*a).total_cmp(&f64::from_bits(*b)),
        (Key::Text(a), Key::Text(b)) => a.cmp(b),
        (Key::Boolean(a), Key::Boolean(b)) => a.cmp(b),
        (Key::Error(a), Key::Error(b)) => a.cmp(b),
        (Key::Blank, Key::Blank) => Ordering::Equal,
        _ => rank(a).cmp(&rank(b)),
    }
}
pub(super) fn call(name: &str, x: &Args<'_>, m: &Grid, steps: &mut usize) -> Option<Result<Value>> {
    if !matches!(name, "SORT" | "SORTBY" | "UNIQUE") {
        return None;
    }
    Some((|| {
        if name == "UNIQUE" {
            x.count(1, 3)?;
            let by_col = x.boolean(1, false)?;
            let once = x.boolean(2, false)?;
            let count = if by_col { m.cols } else { m.rows };
            let mut positions = std::collections::HashMap::<Vec<Key>, usize>::new();
            let mut groups = Vec::<(usize, usize)>::new();
            for i in 0..count {
                let keys: Vec<_> = if by_col {
                    (0..m.rows).map(|r| key(m, r, i)).collect()
                } else {
                    (0..m.cols).map(|c| key(m, i, c)).collect()
                };
                charge(steps, keys.len())?;
                if let Some(group) = positions.get(&keys) {
                    groups[*group].1 += 1;
                } else {
                    positions.insert(keys, groups.len());
                    groups.push((i, 1));
                }
            }
            let selected: Vec<_> = groups
                .into_iter()
                .filter(|(_, count)| !once || *count == 1)
                .map(|(i, _)| i)
                .collect();
            return Ok(if by_col {
                m.select(&(0..m.rows).collect::<Vec<_>>(), &selected)?
            } else {
                m.select(&selected, &(0..m.cols).collect::<Vec<_>>())?
            }
            .value());
        }
        let mut keys = Vec::<(Vec<Key>, i64)>::new();
        let by_col;
        if name == "SORT" {
            x.count(1, 4)?;
            by_col = x.boolean(3, false)?;
            let k = x.integer(1, Some(1))?;
            let direction = x.integer(2, Some(1))?;
            if ![-1, 1].contains(&direction)
                || k < 1
                || k > if by_col { m.rows } else { m.cols } as i64
            {
                return Err("#VALUE!");
            }
            keys.push((
                (0..if by_col { m.cols } else { m.rows })
                    .map(|i| {
                        if by_col {
                            key(m, k as usize - 1, i)
                        } else {
                            key(m, i, k as usize - 1)
                        }
                    })
                    .collect(),
                direction,
            ));
        } else {
            x.count(2, 255)?;
            let first = x.grid(1)?;
            by_col = first.rows == 1 && first.cols == m.cols && m.rows != first.values.len();
            let n = if by_col { m.cols } else { m.rows };
            let mut i = 1;
            while i < x.0.len() {
                let grid = x.grid(i)?;
                let direction = x.integer(i + 1, Some(1))?;
                if ![-1, 1].contains(&direction)
                    || grid.values.len() != n
                    || grid.rows != 1 && grid.cols != 1
                {
                    return Err("#VALUE!");
                }
                keys.push((
                    (0..grid.values.len())
                        .map(|j| key(&grid, j / grid.cols, j % grid.cols))
                        .collect(),
                    direction,
                ));
                i += 2;
            }
        }
        let n = if by_col { m.cols } else { m.rows };
        let comparisons = n
            .saturating_mul((usize::BITS - n.max(1).leading_zeros()) as usize)
            .saturating_mul(keys.len());
        charge(steps, comparisons)?;
        let mut indices: Vec<_> = (0..n).collect();
        indices.sort_by(|a, b| {
            for (keys, direction) in &keys {
                let ordering = order(&keys[*a], &keys[*b]);
                if !ordering.is_eq() {
                    return if matches!(keys[*a], Key::Blank)
                        || matches!(keys[*b], Key::Blank)
                        || *direction > 0
                    {
                        ordering
                    } else {
                        ordering.reverse()
                    };
                }
            }
            a.cmp(b)
        });
        Ok(if by_col {
            m.select(&(0..m.rows).collect::<Vec<_>>(), &indices)?
        } else {
            m.select(&indices, &(0..m.cols).collect::<Vec<_>>())?
        }
        .value())
    })())
}
