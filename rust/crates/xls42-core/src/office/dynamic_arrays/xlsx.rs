use super::*;
fn cell(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    update: &Update,
    cm: usize,
) -> Result<String, Error> {
    let raw = &source[node.range()];
    let p = prefix(raw);
    let mut attrs = Vec::new();
    let mut remove = Vec::new();
    let mut content = String::new();
    if let Some(array) = update.array {
        attrs.push(("", "cm", array.map(|_| update.cm.unwrap_or(cm).to_string())));
        if let Some(formula) = node.children().find(|n| n.has_tag_name((S, "f"))) {
            let span = array.map(|r| {
                format!(
                    "{}{}:{}{}",
                    col_name(r.left),
                    r.top + 1,
                    col_name(r.right),
                    r.bottom + 1
                )
            });
            let f = xml_nodes::edit(
                source,
                formula,
                &[("", "t", array.map(|_| "array".into())), ("", "ref", span)],
                &[],
                "",
            )?;
            let opening = xml_nodes::opening(&f)?;
            let closing = f
                .rfind("</")
                .ok_or(Error::Invalid("Invalid dynamic formula output"))?;
            content.push_str(&f[..opening + 1]);
            content.push_str(&escape_xml(&standard_formula(formula.text().unwrap_or(""))));
            content.push_str(&f[closing..]);
            remove.push((S, "f"));
        } else if array.is_some() {
            return Err(Error::Invalid("Missing dynamic formula"));
        }
    }
    if let Some(value) = &update.cache {
        remove.extend([(S, "v"), (S, "is")]);
        let (kind, cached) = match value {
            Some(formula::Value::Number(n)) => (None, Some(n.to_string())),
            Some(formula::Value::Boolean(b)) => (Some("b"), Some(u8::from(*b).to_string())),
            Some(formula::Value::Text(s)) => (Some("str"), Some(formula_cache::cached_text_xml(s))),
            Some(formula::Value::Error(e)) => (
                Some("e"),
                Some(escape_xml(match *e {
                    "#CYCLE!" => "#REF!",
                    "#ERROR!" => "#NAME?",
                    "#LIMIT!" => "#NUM!",
                    e => e,
                })),
            ),
            None => (None, None),
            Some(formula::Value::Array(_)) => return Err(Error::Invalid("Nested spill cache")),
        };
        attrs.push(("", "t", kind.map(str::to_string)));
        if let Some(value) = cached {
            content.push_str(&format!("<{p}v>{value}</{p}v>"));
        }
    }
    xml_nodes::edit(source, node, &attrs, &remove, &content)
}
fn new_cell(p: &str, r: usize, c: usize, update: &Update) -> Result<String, Error> {
    let Some(Some(value)) = &update.cache else {
        return Ok(String::new());
    };
    if update.array.is_some() {
        return Err(Error::Invalid("Missing array anchor cell"));
    }
    let (kind, body) = match value {
        formula::Value::Number(n) => ("", n.to_string()),
        formula::Value::Boolean(b) => (" t=\"b\"", u8::from(*b).to_string()),
        formula::Value::Text(s) => (" t=\"str\"", formula_cache::cached_text_xml(s)),
        formula::Value::Error(e) => (" t=\"e\"", escape_xml(e)),
        formula::Value::Array(_) => return Err(Error::Invalid("Nested spill cache")),
    };
    Ok(format!(
        "<{p}c r=\"{}{}\"{kind}><{p}v>{body}</{p}v></{p}c>",
        col_name(c),
        r + 1
    ))
}
fn row(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    changes: &BTreeMap<usize, &Update>,
    cm: usize,
) -> Result<String, Error> {
    let raw = &source[node.range()];
    let p = prefix(raw);
    let cells: Vec<_> = node
        .children()
        .filter(|n| n.has_tag_name((S, "c")))
        .collect();
    let mut edits = Vec::new();
    let mut additions = BTreeMap::<usize, String>::new();
    let end = xml_nodes::opening(raw)?;
    for (&col, update) in changes {
        if let Some(n) = cells
            .iter()
            .find(|n| n.attribute("r").and_then(a1).is_some_and(|(_, c)| c == col))
        {
            edits.push((
                n.range().start - node.range().start,
                n.range().end - node.range().start,
                cell(source, *n, update, cm)?,
            ));
        } else {
            let r = node
                .attribute("r")
                .and_then(|v| v.parse::<usize>().ok())
                .and_then(|v| v.checked_sub(1))
                .ok_or(Error::Invalid("Invalid cache row"))?;
            let value = new_cell(p, r, col, update)?;
            let at = cells
                .iter()
                .find(|n| n.attribute("r").and_then(a1).is_some_and(|(_, c)| c > col))
                .map(|n| n.range().start - node.range().start)
                .unwrap_or_else(|| raw.rfind("</").unwrap_or(end.saturating_sub(1)));
            additions.entry(at).or_default().push_str(&value);
        }
    }
    let closed = raw.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/');
    if closed {
        let values = additions.into_values().collect::<String>();
        return Ok(format!("{}>{values}</{p}row>", &raw[..end - 1]));
    }
    for (at, value) in additions {
        edits.push((at, at, value));
    }
    Ok(apply_edits(raw.into(), edits))
}
pub(super) fn patch(
    source: &str,
    changes: &BTreeMap<(usize, usize), Update>,
    cm: usize,
) -> Result<String, Error> {
    let doc = xml(source)?;
    let data = doc
        .descendants()
        .find(|n| n.has_tag_name((S, "sheetData")))
        .ok_or(Error::Invalid("Missing worksheet data"))?;
    let rows: Vec<_> = data
        .children()
        .filter(|n| n.has_tag_name((S, "row")))
        .collect();
    let mut edits = Vec::new();
    let mut additions = BTreeMap::<usize, String>::new();
    let p = prefix(&source[data.range()]);
    let mut by_row: BTreeMap<usize, BTreeMap<usize, &Update>> = BTreeMap::new();
    for (&(r, c), update) in changes {
        by_row.entry(r).or_default().insert(c, update);
    }
    for (r, changes) in by_row {
        if let Some(n) = rows
            .iter()
            .find(|n| n.attribute("r").and_then(|v| v.parse::<usize>().ok()) == Some(r + 1))
        {
            edits.push((
                n.range().start,
                n.range().end,
                row(source, *n, &changes, cm)?,
            ));
        } else {
            let mut value = String::new();
            for (c, update) in changes {
                value.push_str(&new_cell(p, r, c, update)?);
            }
            if value.is_empty() {
                continue;
            }
            let new = format!("<{p}row r=\"{}\">{value}</{p}row>", r + 1);
            let at = rows
                .iter()
                .find(|n| {
                    n.attribute("r")
                        .and_then(|v| v.parse::<usize>().ok())
                        .is_some_and(|rr| rr > r + 1)
                })
                .map(|n| n.range().start)
                .unwrap_or_else(|| {
                    data.range().start + source[data.range()].rfind("</").unwrap_or(0)
                });
            additions.entry(at).or_default().push_str(&new);
        }
    }
    for (at, value) in additions {
        edits.push((at, at, value));
    }
    if let Some(dim) = doc.descendants().find(|n| n.has_tag_name((S, "dimension"))) {
        let mut range = dim.attribute("ref").and_then(range).unwrap_or(Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        });
        for (&(r, c), u) in changes {
            if matches!(u.cache, Some(Some(_))) {
                range.top = range.top.min(r);
                range.left = range.left.min(c);
                range.bottom = range.bottom.max(r);
                range.right = range.right.max(c);
            }
        }
        let value = format!(
            "{}{}:{}{}",
            col_name(range.left),
            range.top + 1,
            col_name(range.right),
            range.bottom + 1
        );
        edits.push((
            dim.range().start,
            dim.range().end,
            xml_nodes::edit(source, dim, &[("", "ref", Some(value))], &[], "")?,
        ));
    }
    let output = apply_edits(source.into(), edits);
    xml(&output)?;
    Ok(output)
}
fn standard_formula(raw: &str) -> String {
    let b = raw.as_bytes();
    let (mut i, mut quoted, mut sheet) = (0, false, false);
    let mut out = String::new();
    while i < b.len() {
        if b[i] == b'"' && !sheet {
            if quoted && b.get(i + 1) == Some(&b'"') {
                out.push_str("\"\"");
                i += 2;
                continue;
            }
            quoted = !quoted;
        } else if b[i] == b'\'' && !quoted {
            if sheet && b.get(i + 1) == Some(&b'\'') {
                out.push_str("''");
                i += 2;
                continue;
            }
            sheet = !sheet;
        }
        if !quoted && !sheet && (b[i].is_ascii_alphabetic() || b[i] == b'_') {
            let start = i;
            while b
                .get(i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.'))
            {
                i += 1;
            }
            let token = &raw[start..i];
            let name = token.to_ascii_uppercase();
            let mut next = i;
            while b.get(next).is_some_and(u8::is_ascii_whitespace) {
                next += 1;
            }
            if b.get(next) == Some(&b'(')
                && matches!(
                    name.as_str(),
                    "FILTER"
                        | "SORT"
                        | "SORTBY"
                        | "UNIQUE"
                        | "SEQUENCE"
                        | "RANDARRAY"
                        | "TEXTSPLIT"
                        | "MODE.MULT"
                        | "MUNIT"
                        | "XLOOKUP"
                        | "VSTACK"
                        | "HSTACK"
                        | "TOCOL"
                        | "TOROW"
                        | "CHOOSECOLS"
                        | "CHOOSEROWS"
                        | "TAKE"
                        | "DROP"
                        | "EXPAND"
                        | "WRAPROWS"
                        | "WRAPCOLS"
                        | "LET"
                        | "REGEXEXTRACT"
                )
            {
                out.push_str(if matches!(name.as_str(), "FILTER" | "SORT") {
                    "_xlfn._xlws."
                } else {
                    "_xlfn."
                });
                out.push_str(token);
            } else {
                out.push_str(token);
            }
            continue;
        }
        let c = raw[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}
