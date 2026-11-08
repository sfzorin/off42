//! Surgical element updates keep unknown attributes and child objects byte-for-byte.
use super::*;
pub(super) fn opening(raw: &str) -> Result<usize, Error> {
    let mut quote = None;
    for (i, c) in raw.char_indices() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if matches!(c, '\'' | '"') {
            quote = Some(c);
        } else if c == '>' {
            return Ok(i);
        }
    }
    Err(Error::Invalid("Unterminated Office element"))
}
pub(super) fn edit(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    attrs: &[(&str, &str, Option<String>)],
    remove: &[(&str, &str)],
    insert: &str,
) -> Result<String, Error> {
    let range = node.range();
    let raw = &source[range.clone()];
    let end = opening(raw)?;
    let closed = raw.as_bytes().get(end.wrapping_sub(1)) == Some(&b'/');
    let at = if closed { end - 1 } else { end };
    let mut changes = Vec::new();
    let mut add = String::new();
    let mut namespaces = BTreeMap::<String, String>::new();
    for (ns, name, value) in attrs {
        let existing = node
            .attributes()
            .find(|a| a.name() == *name && a.namespace().unwrap_or("") == *ns);
        if existing.is_none() && value.is_none() {
            continue;
        }
        let qualified = if let Some(a) = existing {
            source[a.range()]
                .split('=')
                .next()
                .unwrap()
                .trim()
                .to_string()
        } else if ns.is_empty() {
            (*name).into()
        } else {
            let p = if let Some(prefix) = node.lookup_prefix(ns) {
                prefix.to_owned()
            } else if let Some(prefix) = namespaces.get(*ns) {
                prefix.clone()
            } else {
                let mut i = 0usize;
                let prefix = loop {
                    let p = format!("off42ns{i}");
                    if node.lookup_namespace_uri(Some(&p)).is_none()
                        && !namespaces.values().any(|v| v == &p)
                    {
                        break p;
                    }
                    i += 1;
                };
                add.push_str(&format!(" xmlns:{prefix}=\"{}\"", escape_xml(ns)));
                namespaces.insert((*ns).into(), prefix.clone());
                prefix
            };
            format!("{p}:{name}")
        };
        if let Some(a) = existing {
            changes.push((
                a.range().start - range.start,
                a.range().end - range.start,
                value
                    .as_ref()
                    .map(|v| {
                        format!(
                            "{qualified}=\"{}\"",
                            escape_xml(v)
                                .replace('\n', "&#10;")
                                .replace('\r', "&#13;")
                                .replace('\t', "&#9;")
                        )
                    })
                    .unwrap_or_default(),
            ));
        } else if let Some(value) = value {
            add.push_str(&format!(
                " {qualified}=\"{}\"",
                escape_xml(value)
                    .replace('\n', "&#10;")
                    .replace('\r', "&#13;")
                    .replace('\t', "&#9;")
            ));
        }
    }
    for child in node
        .children()
        .filter(|n| remove.iter().any(|(ns, name)| n.has_tag_name((*ns, *name))))
    {
        changes.push((
            child.range().start - range.start,
            child.range().end - range.start,
            String::new(),
        ));
    }
    if closed {
        let p = prefix(raw);
        changes.push((
            at,
            end + 1,
            format!("{add}>{insert}</{p}{}>", node.tag_name().name()),
        ));
    } else {
        changes.push((at, at, add));
        if !insert.is_empty() {
            changes.push((end + 1, end + 1, insert.into()));
        }
    }
    Ok(apply_edits(raw.into(), changes))
}
pub(super) fn repeat(
    source: &str,
    node: roxmltree::Node<'_, '_>,
    name: &str,
    count: usize,
) -> Result<String, Error> {
    edit(
        source,
        node,
        &[(
            T,
            name,
            if count > 1 {
                Some(count.to_string())
            } else {
                None
            },
        )],
        &[],
        "",
    )
}
