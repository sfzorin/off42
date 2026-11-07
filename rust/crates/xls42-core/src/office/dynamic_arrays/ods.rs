use super::*;
pub(super) fn spans(text: &str) -> Result<BTreeMap<String, Vec<Span>>, Error> {
    let doc = xml(text)?;
    let mut result = BTreeMap::new();
    for table in doc.descendants().filter(|n| {
        n.has_tag_name((T, "table")) && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let name = table
            .attribute((T, "name"))
            .ok_or(Error::Invalid("Missing ODS sheet name"))?;
        let mut ranges = Vec::new();
        let mut r = 0usize;
        for row in table.descendants().filter(|n| {
            n.has_tag_name((T, "table-row"))
                && n.ancestors().skip(1).find(|a| a.has_tag_name((T, "table"))) == Some(table)
        }) {
            let rr = row
                .attribute((T, "number-rows-repeated"))
                .unwrap_or("1")
                .parse::<usize>()
                .map_err(|_| Error::Invalid("Invalid ODS row repeat"))?;
            let mut c = 0usize;
            for cell in row.children().filter(|n| {
                n.has_tag_name((T, "table-cell")) || n.has_tag_name((T, "covered-table-cell"))
            }) {
                let cc = cell
                    .attribute((T, "number-columns-repeated"))
                    .unwrap_or("1")
                    .parse::<usize>()
                    .map_err(|_| Error::Invalid("Invalid ODS column repeat"))?;
                if cell.attribute((T, "number-matrix-rows-spanned")).is_some()
                    || cell
                        .attribute((T, "number-matrix-columns-spanned"))
                        .is_some()
                {
                    if rr != 1 || cc != 1 {
                        return Err(Error::Invalid(
                            "Repeated ODS matrix formulas cannot yet be imported safely",
                        ));
                    }
                    let h = cell
                        .attribute((T, "number-matrix-rows-spanned"))
                        .unwrap_or("1")
                        .parse::<usize>()
                        .map_err(|_| Error::Invalid("Invalid ODS matrix height"))?;
                    let w = cell
                        .attribute((T, "number-matrix-columns-spanned"))
                        .unwrap_or("1")
                        .parse::<usize>()
                        .map_err(|_| Error::Invalid("Invalid ODS matrix width"))?;
                    let bottom = r
                        .checked_add(h)
                        .and_then(|v| v.checked_sub(1))
                        .filter(|v| *v < MAX_ROWS)
                        .ok_or(Error::Limit)?;
                    let right = c
                        .checked_add(w)
                        .and_then(|v| v.checked_sub(1))
                        .filter(|v| *v < MAX_COLS)
                        .ok_or(Error::Limit)?;
                    let dynamic = cell
                        .attribute((T, "formula"))
                        .and_then(ods_formula_to_a1)
                        .is_some_and(|f| formula::is_dynamic_formula(&f));
                    ranges.push(Span {
                        anchor: (r, c),
                        range: Range {
                            top: r,
                            left: c,
                            bottom,
                            right,
                        },
                        dynamic,
                        cm: None,
                    });
                }
                c = c.checked_add(cc).ok_or(Error::Limit)?;
            }
            r = r.checked_add(rr).ok_or(Error::Limit)?;
        }
        result.insert(name.into(), ranges);
    }
    Ok(result)
}
fn count(node: roxmltree::Node<'_, '_>, name: &str) -> Result<usize, Error> {
    node.attribute((T, name))
        .unwrap_or("1")
        .parse::<usize>()
        .ok()
        .filter(|n| *n > 0 && *n <= 1_048_576)
        .ok_or(Error::Limit)
}
fn cell(source: &str, node: roxmltree::Node<'_, '_>, update: &Update) -> Result<String, Error> {
    let mut attrs = Vec::new();
    let mut remove = Vec::new();
    let mut body = String::new();
    if let Some(style) = &update.style {
        attrs.push((T, "style-name", style.clone()));
    }
    if let Some(array) = update.array {
        attrs.push((
            T,
            "number-matrix-rows-spanned",
            array
                .filter(|r| r.bottom > r.top)
                .map(|r| (r.bottom - r.top + 1).to_string()),
        ));
        attrs.push((
            T,
            "number-matrix-columns-spanned",
            array
                .filter(|r| r.right > r.left)
                .map(|r| (r.right - r.left + 1).to_string()),
        ));
    }
    if let Some(value) = &update.cache {
        for name in [
            "value-type",
            "value",
            "string-value",
            "boolean-value",
            "date-value",
            "time-value",
            "currency",
        ] {
            attrs.push((O, name, None));
        }
        remove.push(("urn:oasis:names:tc:opendocument:xmlns:text:1.0", "p"));
        if let Some(value) = value {
            let (kind, key, raw) = match value {
                formula::Value::Number(n) => ("float", "value", n.to_string()),
                formula::Value::Boolean(b) => ("boolean", "boolean-value", b.to_string()),
                formula::Value::Text(s) => ("string", "string-value", s.clone()),
                formula::Value::Error(e) => ("string", "string-value", e.to_string()),
                formula::Value::Array(_) => return Err(Error::Invalid("Nested ODS array")),
            };
            attrs.retain(|(_, name, _)| !matches!(*name, "value-type") && *name != key);
            attrs.push((O, "value-type", Some(kind.into())));
            attrs.push((O, key, Some(raw.clone())));
            body = format!(
                "<text:p xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\">{}</text:p>",
                escape_xml(&raw)
                    .replace('\r', "&#13;")
                    .replace('\n', "<text:line-break/>")
                    .replace('\t', "<text:tab/>")
            );
        }
    }
    attrs.push((T, "number-columns-repeated", None));
    xml_nodes::edit(source, node, &attrs, &remove, &body)
}
fn new_cell(update: Option<&Update>) -> Result<String, Error> {
    let raw = format!("<table:table-cell xmlns:table=\"{T}\" xmlns:office=\"{O}\"/>");
    if let Some(update) = update {
        let doc = xml(&raw)?;
        cell(&raw, doc.root_element(), update)
    } else {
        Ok(raw)
    }
}
fn edited_row(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    changes: &BTreeMap<usize, &Update>,
) -> Result<String, Error> {
    let mut edits = Vec::new();
    let mut col = 0usize;
    for child in node
        .children()
        .filter(|n| n.has_tag_name((T, "table-cell")) || n.has_tag_name((T, "covered-table-cell")))
    {
        let repeat = count(child, "number-columns-repeated")?;
        let end = col.checked_add(repeat).ok_or(Error::Limit)?;
        let wanted: Vec<_> = changes.range(col..end).collect();
        if !wanted.is_empty() {
            let mut output = String::new();
            let mut from = col;
            for (&c, update) in wanted {
                if c > from {
                    output.push_str(&xml_nodes::repeat(
                        source,
                        child,
                        "number-columns-repeated",
                        c - from,
                    )?);
                }
                output.push_str(&cell(source, child, update)?);
                from = c + 1;
            }
            if end > from {
                output.push_str(&xml_nodes::repeat(
                    source,
                    child,
                    "number-columns-repeated",
                    end - from,
                )?);
            }
            edits.push((
                child.range().start - node.range().start,
                child.range().end - node.range().start,
                output,
            ));
        }
        col = end;
    }
    let mut missing = String::new();
    for (&c, update) in changes.range(col..) {
        while col < c {
            missing.push_str(&new_cell(None)?);
            col += 1;
        }
        missing.push_str(&new_cell(Some(update))?);
        col += 1;
    }
    let raw = &source[node.range()];
    if !missing.is_empty() {
        let end = xml_nodes::opening(raw)?;
        if raw.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/') {
            let p = prefix(raw);
            edits.push((end - 1, end + 1, format!(">{missing}</{p}table-row>")));
        } else {
            let at = raw.rfind("</").ok_or(Error::Invalid("Invalid ODS row"))?;
            edits.push((at, at, missing));
        }
    }
    // The original row can repeat. Every edited row is materialized once; untouched runs retain repetition.
    if let Some(attr) = node.attribute_node((T, "number-rows-repeated")) {
        edits.push((
            attr.range().start - node.range().start,
            attr.range().end - node.range().start,
            String::new(),
        ));
    }
    Ok(apply_edits(raw.into(), edits))
}
pub(super) fn patch(
    source: &str,
    changes: &BTreeMap<String, BTreeMap<(usize, usize), Update>>,
) -> Result<String, Error> {
    let doc = xml(source)?;
    let mut edits = Vec::new();
    for table in doc.descendants().filter(|n| {
        n.has_tag_name((T, "table")) && n.parent().is_some_and(|p| p.has_tag_name("spreadsheet"))
    }) {
        let Some(update) = table.attribute((T, "name")).and_then(|n| changes.get(n)) else {
            continue;
        };
        let mut by_row: BTreeMap<usize, BTreeMap<usize, &Update>> = BTreeMap::new();
        for (&(r, c), u) in update {
            by_row.entry(r).or_default().insert(c, u);
        }
        let mut row = 0usize;
        let mut last_end = None;
        for node in table.descendants().filter(|n| {
            n.has_tag_name((T, "table-row"))
                && n.ancestors().skip(1).find(|a| a.has_tag_name((T, "table"))) == Some(table)
        }) {
            let repeated = count(node, "number-rows-repeated")?;
            let end = row.checked_add(repeated).ok_or(Error::Limit)?;
            let wanted: Vec<_> = by_row.range(row..end).collect();
            if !wanted.is_empty() {
                let mut output = String::new();
                let mut from = row;
                for (&r, u) in wanted {
                    if r > from {
                        output.push_str(&xml_nodes::repeat(
                            source,
                            node,
                            "number-rows-repeated",
                            r - from,
                        )?);
                    }
                    output.push_str(&edited_row(source, node, u)?);
                    from = r + 1;
                }
                if end > from {
                    output.push_str(&xml_nodes::repeat(
                        source,
                        node,
                        "number-rows-repeated",
                        end - from,
                    )?);
                }
                edits.push((node.range().start, node.range().end, output));
            }
            last_end = Some(node.range().end);
            row = end;
        }
        let mut missing = String::new();
        for (&r, u) in by_row.range(row..) {
            while row < r {
                missing.push_str(&format!("<table:table-row xmlns:table=\"{T}\"/>"));
                row += 1;
            }
            let mut cells = String::new();
            let mut c = 0;
            for (&col, update) in u {
                while c < col {
                    cells.push_str(&new_cell(None)?);
                    c += 1;
                }
                cells.push_str(&new_cell(Some(update))?);
                c += 1;
            }
            missing.push_str(&format!(
                "<table:table-row xmlns:table=\"{T}\">{cells}</table:table-row>"
            ));
            row += 1;
        }
        if !missing.is_empty() {
            let at = last_end.unwrap_or_else(|| {
                table.range().start + source[table.range()].rfind("</").unwrap_or(0)
            });
            edits.push((at, at, missing));
        }
    }
    let output = apply_edits(source.into(), edits);
    xml(&output)?;
    Ok(output)
}
