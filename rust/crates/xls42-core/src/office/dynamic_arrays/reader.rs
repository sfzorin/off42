//! Calamine's legacy error enum cannot parse modern Excel errors. Sanitize a disposable reader copy only.
use super::*;
pub(super) type Errors = BTreeMap<(String, usize, usize), String>;
pub(in crate::office) fn working_copy<'a>(
    bytes: &'a [u8],
    kind: &str,
) -> Result<(std::borrow::Cow<'a, [u8]>, Errors), Error> {
    if kind != "xlsx" {
        return Ok((std::borrow::Cow::Borrowed(bytes), Errors::new()));
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid XLSX reader package"))?;
    let mut errors = Errors::new();
    let mut entries = BTreeMap::new();
    for (name, path) in targets(&mut archive)? {
        let text = zip_text(&mut archive, &path)?.ok_or(Error::Invalid("Missing worksheet"))?;
        let doc = xml(&text)?;
        let mut edits = Vec::new();
        for cell in doc
            .descendants()
            .filter(|n| n.has_tag_name((S, "c")) && n.attribute("t") == Some("e"))
        {
            let Some(value) = cell.children().find(|n| n.has_tag_name((S, "v"))) else {
                continue;
            };
            let error = value.text().unwrap_or("");
            if matches!(
                error,
                "#GETTING_DATA"
                    | "#SPILL!"
                    | "#CALC!"
                    | "#FIELD!"
                    | "#BLOCKED!"
                    | "#CONNECT!"
                    | "#UNKNOWN!"
                    | "#BUSY!"
                    | "#PYTHON!"
            ) {
                let (r, c) = cell
                    .attribute("r")
                    .and_then(a1)
                    .ok_or(Error::Invalid("Invalid error cell"))?;
                errors.insert((name.clone(), r, c), error.into());
                let p = prefix(&text[value.range()]);
                edits.push((
                    value.range().start,
                    value.range().end,
                    format!("<{p}v>#N/A</{p}v>"),
                ));
            }
        }
        if !edits.is_empty() {
            entries.insert(path, apply_edits(text.clone(), edits).into_bytes());
        }
    }
    if entries.is_empty() {
        Ok((std::borrow::Cow::Borrowed(bytes), errors))
    } else {
        Ok((
            std::borrow::Cow::Owned(rewrite_zip(bytes, entries)?),
            errors,
        ))
    }
}
