//! RTF list and override tables for decimal and bullet lists.
use super::*;
use std::collections::BTreeMap;
pub(super) fn tables(blocks: &[Block]) -> Result<String, Error> {
    let mut lists = BTreeMap::<u64, BTreeMap<u8, bool>>::new();
    for p in blocks.iter().flat_map(|b| match b {
        Block::Paragraph(p) => vec![p],
        Block::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| r.iter().flat_map(|c| c.paragraphs.iter()))
            .collect(),
        _ => vec![],
    }) {
        if let Some(l) = &p.style.list {
            if l.id == 0 || l.id > i32::MAX as u64 || l.level > 8 {
                return Err(Error::Invalid("invalid RTF list ID".into()));
            }
            if lists
                .entry(l.id)
                .or_default()
                .insert(l.level, l.numbered)
                .is_some_and(|v| v != l.numbered)
            {
                return Err(Error::Unsupported("conflicting RTF list level".into()));
            }
        }
    }
    if lists.is_empty() {
        return Ok(String::new());
    }
    let mut table = String::from("{\\*\\listtable");
    let mut overrides = String::from("{\\*\\listoverridetable");
    for (id, levels) in lists {
        let fallback = levels.values().next().copied().unwrap_or(true);
        table.push_str(&format!("{{\\list\\listtemplateid{id}"));
        for level in 0..9 {
            let numbered = levels.get(&level).copied().unwrap_or(fallback);
            if numbered {
                table.push_str(&format!("{{\\listlevel\\levelnfc0\\levelstartat1{{\\leveltext\\'02\\'{level:02x}.;}}{{\\levelnumbers\\'01;}}}}"));
            } else {
                table.push_str("{\\listlevel\\levelnfc23\\levelstartat1{\\leveltext\\'01\\u8226 ?;}{\\levelnumbers;}}");
            }
        }
        table.push_str(&format!("\\listid{id}}}"));
        overrides.push_str(&format!(
            "{{\\listoverride\\listid{id}\\listoverridecount0\\ls{id}}}"
        ));
    }
    table.push('}');
    overrides.push('}');
    table.push_str(&overrides);
    Ok(table)
}
/// Extract one balanced RTF group, accounting for escaped braces.
pub(super) fn group_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => depth += 1,
            b'}' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::Invalid("list group".into()))?;
                if depth == 0 {
                    return Ok(i + 1);
                }
            }
            b'\\' if i + 1 < bytes.len() && matches!(bytes[i + 1], b'{' | b'}' | b'\\') => i += 1,
            _ => {}
        }
        i += 1
    }
    Err(Error::Invalid("unterminated list table".into()))
}
fn number(data: &[u8], key: &[u8]) -> Option<i64> {
    let i = data.windows(key.len()).position(|x| x == key)? + key.len();
    let mut end = i;
    if data.get(end) == Some(&b'-') {
        end += 1
    }
    while data.get(end).is_some_and(u8::is_ascii_digit) {
        end += 1
    }
    std::str::from_utf8(&data[i..end]).ok()?.parse().ok()
}
fn groups<'a>(data: &'a [u8], prefix: &[u8]) -> Result<Vec<&'a [u8]>, Error> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + prefix.len() <= data.len() {
        if &data[i..i + prefix.len()] == prefix {
            let end = group_end(data, i)?;
            out.push(&data[i..end]);
            i = end
        } else {
            i += 1
        }
    }
    Ok(out)
}
pub(super) fn definitions(data: &[u8]) -> Result<BTreeMap<u64, Vec<bool>>, Error> {
    let mut out = BTreeMap::new();
    for list in groups(data, b"{\\list\\")? {
        let id = u64::try_from(
            number(list, b"\\listid").ok_or_else(|| Error::Invalid("RTF list id".into()))?,
        )
        .map_err(|_| Error::Invalid("RTF list id".into()))?;
        let mut levels = Vec::new();
        for level in groups(list, b"{\\listlevel\\")? {
            let n = number(level, b"\\levelnfc")
                .ok_or_else(|| Error::Invalid("RTF level format".into()))?;
            if !matches!(n, 0 | 23) {
                return Err(Error::Unsupported("RTF list numbering style".into()));
            }
            levels.push(n == 0)
        }
        if levels.is_empty() {
            return Err(Error::Invalid("RTF list levels".into()));
        }
        out.insert(id, levels);
    }
    Ok(out)
}
pub(super) fn overrides(data: &[u8]) -> Result<BTreeMap<u64, u64>, Error> {
    let mut out = BTreeMap::new();
    for item in groups(data, b"{\\listoverride\\")? {
        if number(item, b"\\listoverridecount").unwrap_or(0) != 0 {
            return Err(Error::Unsupported("RTF list overrides".into()));
        }
        let id = u64::try_from(
            number(item, b"\\listid").ok_or_else(|| Error::Invalid("RTF list id".into()))?,
        )
        .map_err(|_| Error::Invalid("RTF list id".into()))?;
        let ls = u64::try_from(
            number(item, b"\\ls").ok_or_else(|| Error::Invalid("RTF list index".into()))?,
        )
        .map_err(|_| Error::Invalid("RTF list index".into()))?;
        out.insert(ls, id);
    }
    Ok(out)
}
