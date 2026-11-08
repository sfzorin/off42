//! Excel/LibreOffice's sheet-scoped hidden Solver names; never replace unrelated names.
use super::*;
use office::{apply_edits, escape_xml as esc};
use std::io::{Cursor, Write};
const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const TABLE: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const LOEXT: &str = "urn:org:documentfoundation:names:experimental:office:xmlns:loext:1.0";
#[derive(Clone)]
enum Item {
    Ranges(Vec<Range>),
    Value(String),
}
fn singleton((r, c): (usize, usize)) -> Range {
    Range {
        top: r,
        left: c,
        bottom: r,
        right: c,
    }
}
fn a1(r: Range) -> String {
    let a = format!("${}${}", office::col_name(r.left), r.top + 1);
    if count(r) == 1 {
        a
    } else {
        format!("{}:${}${}", a, office::col_name(r.right), r.bottom + 1)
    }
}
fn items(m: &SolverModel) -> BTreeMap<String, Item> {
    let mut out = BTreeMap::new();
    out.insert(
        "solver_opt".into(),
        Item::Ranges(vec![singleton(m.objective)]),
    );
    out.insert(
        "solver_adj".into(),
        Item::Ranges(m.variables.iter().copied().map(singleton).collect()),
    );
    let (typ, value) = match m.goal {
        SolverGoal::Maximum => ("1", 0.),
        SolverGoal::Minimum => ("2", 0.),
        SolverGoal::Value(v) => ("3", v),
    };
    for (key, value) in [
        ("solver_typ", typ.into()),
        ("solver_val", value.to_string()),
        ("solver_num", m.constraints.len().to_string()),
        ("solver_neg", if m.non_negative { "1" } else { "2" }.into()),
        ("solver_eng", if m.linear { "2" } else { "1" }.into()),
        ("solver_lin", if m.linear { "1" } else { "2" }.into()),
    ] {
        out.insert(key.into(), Item::Value(value));
    }
    for (i, c) in m.constraints.iter().enumerate() {
        let n = i + 1;
        out.insert(format!("solver_lhs{n}"), Item::Ranges(vec![c.lhs]));
        let rel = match c.op {
            SolverOp::Le => 1,
            SolverOp::Eq => 2,
            SolverOp::Ge => 3,
            SolverOp::Integer => 4,
            SolverOp::Binary => 5,
        };
        out.insert(format!("solver_rel{n}"), Item::Value(rel.to_string()));
        let rhs = match c.op {
            SolverOp::Integer => Item::Value("\"integer\"".into()),
            SolverOp::Binary => Item::Value("\"binary\"".into()),
            _ => {
                if let Some(r) = range(&c.rhs, MAX_ROWS, MAX_COLS) {
                    Item::Ranges(vec![r])
                } else {
                    Item::Value(c.rhs.trim().replace(',', "."))
                }
            }
        };
        out.insert(format!("solver_rhs{n}"), rhs);
    }
    for (key, value) in [
        ("solver_cvg", "0.0001"),
        ("solver_drv", "1"),
        ("solver_est", "1"),
        ("solver_itr", "2147483647"),
        ("solver_mip", "2147483647"),
        ("solver_mni", "30"),
        ("solver_mrt", "0.075"),
        ("solver_msl", "2"),
        ("solver_nod", "2147483647"),
        ("solver_nwt", "1"),
        ("solver_pre", "0.000001"),
        ("solver_rbv", "1"),
        ("solver_rlx", "2"),
        ("solver_rsd", "0"),
        ("solver_scl", "1"),
        ("solver_sho", "2"),
        ("solver_ssz", "100"),
        ("solver_tim", "2147483647"),
        ("solver_tol", "0.01"),
        ("solver_ver", "3"),
    ] {
        out.insert(key.into(), Item::Value(value.into()));
    }
    for (key, value) in &m.extra {
        if !out.contains_key(key)
            || !matches!(
                key.as_str(),
                "solver_opt"
                    | "solver_adj"
                    | "solver_typ"
                    | "solver_val"
                    | "solver_num"
                    | "solver_neg"
                    | "solver_eng"
                    | "solver_lin"
            )
        {
            if !key.starts_with("solver_lhs")
                && !key.starts_with("solver_rel")
                && !key.starts_with("solver_rhs")
            {
                out.insert(key.clone(), Item::Value(value.clone()));
            }
        }
    }
    let engine = m
        .extra
        .get("solver_lo_eng")
        .map(|v| v.trim_matches('"'))
        .filter(|v| is_linear(v) == m.linear)
        .unwrap_or(if m.linear {
            "com.sun.star.comp.Calc.CoinMPSolver"
        } else {
            "com.sun.star.comp.Calc.SwarmSolver"
        });
    out.insert("solver_lo_eng".into(), Item::Value(format!("\"{engine}\"")));
    out
}
fn is_linear(value: &str) -> bool {
    value.contains("CoinMP") || value.contains("Lpsolve") || value.contains("LpSolve")
}
fn split(value: &str, ods: bool) -> Option<Vec<&str>> {
    if value.len() > 32768 {
        return None;
    }
    let mut out = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut chars = value.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\'' {
            if quoted && chars.peek().is_some_and(|(_, c)| *c == '\'') {
                chars.next();
            } else {
                quoted = !quoted
            }
        } else if !quoted && (c == ',' || ods && c.is_ascii_whitespace()) {
            let p = value[start..i].trim();
            if !p.is_empty() {
                out.push(p)
            }
            start = i + c.len_utf8();
        }
    }
    if quoted {
        return None;
    }
    let p = value[start..].trim();
    if !p.is_empty() {
        out.push(p)
    }
    Some(out)
}
fn refs(value: &str, sheet: &str, ods: bool) -> Option<Vec<Range>> {
    fn endpoint<'a>(value: &'a str, sheet: &str, ods: bool) -> Option<&'a str> {
        let separator = if ods { '.' } else { '!' };
        if let Some(at) = value.rfind(separator) {
            let prefix = value[..at].trim_start_matches('$');
            let name = if prefix.starts_with('\'') && prefix.ends_with('\'') {
                prefix[1..prefix.len() - 1].replace("''", "'")
            } else {
                prefix.into()
            };
            if !name.is_empty() && !name.eq_ignore_ascii_case(sheet) {
                return None;
            }
            Some(&value[at + 1..])
        } else {
            Some(value)
        }
    }
    split(value.trim().trim_start_matches('='), ods)?
        .into_iter()
        .map(|part| {
            let part = part.trim_matches(|c| c == '[' || c == ']');
            let mut p = part.split(':');
            let a = endpoint(p.next()?, sheet, ods)?;
            let b = p
                .next()
                .map(|v| endpoint(v, sheet, ods))
                .unwrap_or(Some(a))?;
            if p.next().is_some() {
                return None;
            }
            range(&format!("{a}:{b}"), MAX_ROWS, MAX_COLS)
        })
        .collect()
}
fn parse(names: BTreeMap<String, String>, sheet: &str, ods: bool) -> Option<SolverModel> {
    let get = |key: &str| {
        names
            .get(key)
            .map(|v| v.trim().trim_start_matches("of:").trim_start_matches('='))
    };
    let objective = refs(get("solver_opt")?, sheet, ods)?;
    if objective.len() != 1 || count(objective[0]) != 1 {
        return None;
    }
    let variable_ranges = refs(get("solver_adj")?, sheet, ods)?;
    if variable_ranges.iter().map(|r| count(*r)).sum::<usize>() > 32 {
        return None;
    }
    let variables = variable_ranges
        .into_iter()
        .flat_map(|r| {
            (r.top..=r.bottom).flat_map(move |row| (r.left..=r.right).map(move |col| (row, col)))
        })
        .collect();
    let goal = match get("solver_typ").unwrap_or("1") {
        "1" => SolverGoal::Maximum,
        "2" => SolverGoal::Minimum,
        "3" => SolverGoal::Value(get("solver_val")?.parse().ok()?),
        _ => return None,
    };
    let n = get("solver_num")
        .map(str::parse::<usize>)
        .transpose()
        .ok()?
        .unwrap_or_else(|| names.keys().filter(|k| k.starts_with("solver_lhs")).count());
    if n > 128 {
        return None;
    }
    let mut constraints = Vec::new();
    for i in 1..=n {
        let lhs = refs(get(&format!("solver_lhs{i}"))?, sheet, ods)?;
        if lhs.len() != 1 {
            return None;
        }
        let op = match get(&format!("solver_rel{i}"))? {
            "1" => SolverOp::Le,
            "2" => SolverOp::Eq,
            "3" => SolverOp::Ge,
            "4" => SolverOp::Integer,
            "5" => SolverOp::Binary,
            _ => return None,
        };
        let raw = get(&format!("solver_rhs{i}")).unwrap_or("0");
        let rhs = if matches!(op, SolverOp::Integer | SolverOp::Binary) {
            String::new()
        } else if raw.parse::<f64>().is_ok_and(f64::is_finite) {
            raw.into()
        } else {
            let rs = refs(raw, sheet, ods)?;
            if rs.len() != 1 {
                return None;
            }
            a1(rs[0])
        };
        constraints.push(SolverConstraint {
            lhs: lhs[0],
            op,
            rhs,
        });
    }
    let non_negative = !matches!(get("solver_neg"), Some("0" | "2" | "false"));
    let linear = if let Some(eng) = get("solver_eng") {
        eng == "2"
    } else if let Some(eng) = get("solver_lo_eng") {
        is_linear(eng)
    } else {
        get("solver_lin").unwrap_or("1") == "1"
    };
    let extra = names
        .iter()
        .filter(|(k, _)| {
            !matches!(
                k.as_str(),
                "solver_opt"
                    | "solver_adj"
                    | "solver_typ"
                    | "solver_val"
                    | "solver_num"
                    | "solver_neg"
                    | "solver_eng"
                    | "solver_lin"
            ) && !k.starts_with("solver_lhs")
                && !k.starts_with("solver_rel")
                && !k.starts_with("solver_rhs")
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let m = SolverModel {
        objective: (objective[0].top, objective[0].left),
        goal,
        variables,
        constraints,
        non_negative,
        linear,
        extra,
    };
    valid_model(&m, MAX_ROWS, MAX_COLS).then_some(m)
}
fn read_part(bytes: &[u8], name: &str) -> Result<String, Error> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid workbook package"))?;
    let part = zip
        .by_name(name)
        .map_err(|_| Error::Invalid("Missing workbook XML"))?;
    let mut text = String::new();
    part.take(16 * 1024 * 1024 + 1).read_to_string(&mut text)?;
    if text.len() > 16 * 1024 * 1024 {
        return Err(Error::Limit);
    }
    Ok(text)
}
fn solver_name(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("solver_")
}
pub(crate) fn import(bytes: &[u8], kind: &str, state: &mut State) -> Result<(), Error> {
    if !matches!(kind, "xlsx" | "ods") {
        return Ok(());
    }
    let text = read_part(
        bytes,
        if kind == "xlsx" {
            "xl/workbook.xml"
        } else {
            "content.xml"
        },
    )?;
    let doc = office::xml(&text)?;
    let tables: Vec<_> = doc
        .descendants()
        .filter(|n| n.has_tag_name((TABLE, "table")))
        .collect();
    for (i, sheet) in state.sheets.iter_mut().enumerate() {
        let mut names = BTreeMap::new();
        let mut invalid = false;
        let nodes: Vec<_> = if kind == "xlsx" {
            doc.descendants()
                .filter(|n| {
                    n.has_tag_name("definedName")
                        && n.attribute("localSheetId")
                            .and_then(|v| v.parse::<usize>().ok())
                            == Some(i)
                })
                .collect()
        } else {
            tables
                .get(i)
                .map(|n| {
                    n.children()
                        .filter(|n| n.has_tag_name((TABLE, "named-expressions")))
                        .flat_map(|n| n.children().filter(|n| n.is_element()))
                        .collect()
                })
                .unwrap_or_default()
        };
        for n in nodes {
            let name = if kind == "xlsx" {
                n.attribute("name")
            } else {
                n.attribute((TABLE, "name"))
            }
            .unwrap_or("");
            if !solver_name(name) {
                continue;
            }
            let value = if kind == "xlsx" {
                n.text()
            } else {
                n.attribute((TABLE, "cell-range-address"))
                    .or_else(|| n.attribute((TABLE, "expression")))
            }
            .unwrap_or("");
            if names.len() >= 512
                || value.len() > 32768
                || names
                    .insert(name.to_ascii_lowercase(), value.to_owned())
                    .is_some()
            {
                invalid = true;
                break;
            }
        }
        if !invalid {
            if let Some(m) = parse(names, &sheet.name, kind == "ods") {
                let mut ends = vec![m.objective];
                ends.extend(m.variables.iter().copied());
                for c in &m.constraints {
                    ends.push((c.lhs.bottom, c.lhs.right));
                    if let Some(r) = range(&c.rhs, MAX_ROWS, MAX_COLS) {
                        ends.push((r.bottom, r.right));
                    }
                }
                for (r, c) in ends {
                    sheet.rows = sheet.rows.max(r + 1);
                    sheet.cols = sheet.cols.max(c + 1);
                }
                sheet.solver = Some(m);
            }
        }
    }
    Ok(())
}
fn names_xml(m: Option<&SolverModel>, name: &str, index: usize, ods: bool) -> String {
    let quoted = format!("'{}'", name.replace('\'', "''"));
    let mut out = String::new();
    let Some(m) = m else { return out };
    for (key, item) in items(m) {
        if ods {
            let base = esc(&format!("{quoted}.$A$1"));
            match item{
        Item::Ranges(rs)=>{let value=rs.into_iter().map(|r|{let a=format!("{quoted}.${}${}",office::col_name(r.left),r.top+1);if count(r)==1{a}else{format!("{a}:{quoted}.${}${}",office::col_name(r.right),r.bottom+1)}}).collect::<Vec<_>>().join(" ");out+=&format!("<table:named-range xmlns:table=\"{TABLE}\" table:name=\"{}\" table:base-cell-address=\"{base}\" table:cell-range-address=\"{}\" table:range-usable-as=\"hidden\"/>",esc(&key),esc(&value));},
        Item::Value(v)=>out+=&format!("<table:named-expression xmlns:table=\"{TABLE}\" xmlns:loext=\"{LOEXT}\" table:name=\"{}\" table:base-cell-address=\"{base}\" table:expression=\"{}\" loext:hidden=\"true\"/>",esc(&key),esc(&v)),
    }
        } else {
            let value = match item {
                Item::Ranges(rs) => rs
                    .into_iter()
                    .map(|r| format!("{quoted}!{}", a1(r)))
                    .collect::<Vec<_>>()
                    .join(","),
                Item::Value(v) => v,
            };
            out += &format!(
                "<definedName xmlns=\"{MAIN}\" name=\"{}\" localSheetId=\"{index}\" hidden=\"1\">{}</definedName>",
                esc(&key),
                esc(&value)
            );
        }
    }
    out
}
fn append(
    text: &str,
    node: roxmltree::Node<'_, '_>,
    insert: String,
    edits: &mut Vec<(usize, usize, String)>,
) -> Result<(), Error> {
    if insert.is_empty() {
        return Ok(());
    }
    let raw = &text[node.range()];
    if let Some(i) = raw.rfind("</") {
        let at = node.range().start + i;
        edits.push((at, at, insert));
    } else if raw.ends_with("/>") {
        let at = node.range().end - 2;
        let name = raw[1..]
            .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
            .next()
            .ok_or(Error::Invalid("Invalid XML element"))?;
        edits.push((at, at + 2, format!(">{insert}</{name}>")));
    } else {
        return Err(Error::Invalid("Invalid XML container"));
    }
    Ok(())
}
pub(crate) fn patch(
    bytes: &[u8],
    kind: &str,
    old: Option<&State>,
    state: &State,
) -> Result<Vec<u8>, Error> {
    if !matches!(kind, "xlsx" | "ods") {
        return Ok(bytes.to_vec());
    }
    let changed: Vec<_> = state
        .sheets
        .iter()
        .map(|s| {
            let before = old.and_then(|o| o.sheets.iter().find(|b| b.id == s.id));
            before
                .map(|b| b.solver != s.solver || b.name != s.name && s.solver.is_some())
                .unwrap_or(s.solver.is_some())
        })
        .collect();
    if !changed.iter().any(|v| *v) {
        return Ok(bytes.to_vec());
    }
    let path = if kind == "xlsx" {
        "xl/workbook.xml"
    } else {
        "content.xml"
    };
    let text = read_part(bytes, path)?;
    let doc = office::xml(&text)?;
    let mut edits = Vec::new();
    if kind == "xlsx" {
        let root = doc.root_element();
        let defs = root.children().find(|n| n.has_tag_name("definedNames"));
        if let Some(defs) = defs {
            for n in defs.children().filter(|n| n.has_tag_name("definedName")) {
                if solver_name(n.attribute("name").unwrap_or(""))
                    && n.attribute("localSheetId")
                        .and_then(|v| v.parse::<usize>().ok())
                        .is_some_and(|i| changed.get(i) == Some(&true))
                {
                    edits.push((n.range().start, n.range().end, String::new()));
                }
            }
        }
        let add = state
            .sheets
            .iter()
            .enumerate()
            .filter(|(i, _)| changed[*i])
            .map(|(i, s)| names_xml(s.solver.as_ref(), &s.name, i, false))
            .collect::<String>();
        if let Some(defs) = defs {
            append(&text, defs, add, &mut edits)?
        } else {
            let add = format!("<definedNames xmlns=\"{MAIN}\">{add}</definedNames>");
            let next = root.children().find(|n| {
                n.is_element()
                    && matches!(
                        n.tag_name().name(),
                        "calcPr"
                            | "oleSize"
                            | "customWorkbookViews"
                            | "pivotCaches"
                            | "smartTagPr"
                            | "smartTagTypes"
                            | "webPublishing"
                            | "fileRecoveryPr"
                            | "webPublishObjects"
                            | "extLst"
                    )
            });
            if let Some(n) = next {
                edits.push((n.range().start, n.range().start, add));
            } else {
                append(&text, root, add, &mut edits)?
            }
        }
    } else {
        for (i, table) in doc
            .descendants()
            .filter(|n| n.has_tag_name((TABLE, "table")))
            .enumerate()
        {
            if changed.get(i) != Some(&true) {
                continue;
            }
            let sheet = &state.sheets[i];
            let defs = table
                .children()
                .find(|n| n.has_tag_name((TABLE, "named-expressions")));
            if let Some(defs) = defs {
                for n in defs.children().filter(|n| n.is_element()) {
                    if solver_name(n.attribute((TABLE, "name")).unwrap_or("")) {
                        edits.push((n.range().start, n.range().end, String::new()));
                    }
                }
            }
            let add = names_xml(sheet.solver.as_ref(), &sheet.name, i, true);
            if let Some(defs) = defs {
                append(&text, defs, add, &mut edits)?
            } else {
                append(
                    &text,
                    table,
                    format!("<table:named-expressions xmlns:table=\"{TABLE}\">{add}</table:named-expressions>"),
                    &mut edits,
                )?
            }
        }
    }
    let output = apply_edits(text, edits);
    office::xml(&output)?;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid workbook package"))?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let part = archive
            .by_index(i)
            .map_err(|_| Error::Invalid("Invalid package part"))?;
        if part.name() == path {
            zip.start_file(
                path,
                zip::write::SimpleFileOptions::default().compression_method(part.compression()),
            )
            .map_err(|_| Error::Invalid("Cannot write Solver model"))?;
            zip.write_all(output.as_bytes())?;
        } else {
            zip.raw_copy_file(part)
                .map_err(|_| Error::Invalid("Cannot preserve package part"))?;
        }
    }
    Ok(zip
        .finish()
        .map_err(|_| Error::Invalid("Cannot finish workbook package"))?
        .into_inner())
}
pub(crate) fn owned_ods_names(node: roxmltree::Node<'_, '_>, state: &State) -> bool {
    node.has_tag_name((TABLE, "named-expressions"))
        && node
            .parent()
            .filter(|n| n.has_tag_name((TABLE, "table")))
            .and_then(|n| n.attribute((TABLE, "name")))
            .is_some_and(|name| {
                state
                    .sheets
                    .iter()
                    .any(|s| s.name == name && s.solver.is_some())
            })
        && node.children().filter(|n| n.is_element()).all(|n| {
            matches!(n.tag_name().name(), "named-range" | "named-expression")
                && n.tag_name().namespace() == Some(TABLE)
                && solver_name(n.attribute((TABLE, "name")).unwrap_or(""))
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bad_foreign_or_oversized_models_never_become_partial_models() {
        let mut names = BTreeMap::from([
            ("solver_opt".into(), "'S'!$C$1".into()),
            ("solver_adj".into(), "'S'!$A$1:$B$1".into()),
            ("solver_num".into(), "1".into()),
            ("solver_lhs1".into(), "'S'!$A$1".into()),
            ("solver_rel1".into(), "6".into()),
            ("solver_rhs1".into(), "0".into()),
        ]);
        assert!(parse(names.clone(), "S", false).is_none());
        names.insert("solver_rel1".into(), "1".into());
        assert!(parse(names.clone(), "S", false).is_some());
        names.insert("solver_lhs1".into(), "'Other'!$A$1".into());
        assert!(parse(names.clone(), "S", false).is_none());
        names.insert("solver_lhs1".into(), "'S'!$A$1".into());
        names.insert("solver_adj".into(), "'S'!$A$1:$B$1000".into());
        assert!(parse(names, "S", false).is_none());
    }
}
