//! Recognize standard dynamic-array caches without confusing them with authoritative user inputs.
use super::*;
mod metadata;
mod reader;
mod xlsx;
mod ods;
mod xml_nodes;
pub(super) use reader::working_copy;
const S: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
const T: &str = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
#[derive(Clone, Copy, Debug)]
struct Span {
    anchor: (usize, usize),
    range: Range,
    dynamic: bool,
    cm: Option<usize>,
}
pub(super) fn range(reference: &str) -> Option<Range> {
    let (a, b) = reference.split_once(':').unwrap_or((reference, reference));
    let (top, left) = a1(a)?;
    let (bottom, right) = a1(b)?;
    Some(Range {
        top,
        left,
        bottom,
        right,
    })
    .filter(|r| r.top <= r.bottom && r.left <= r.right && r.bottom < MAX_ROWS && r.right < MAX_COLS)
}
fn targets(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
) -> Result<BTreeMap<String, String>, Error> {
    let book = zip_text(archive, "xl/workbook.xml")?.ok_or(Error::Invalid("Missing workbook"))?;
    let rels = zip_text(archive, "xl/_rels/workbook.xml.rels")?
        .ok_or(Error::Invalid("Missing workbook relationships"))?;
    let (book, rels) = (xml(&book)?, xml(&rels)?);
    let mut links = BTreeMap::new();
    for r in rels.descendants().filter(|n| {
        n.has_tag_name("Relationship")
            && n.attribute("Type")
                .is_some_and(|t| t.ends_with("/worksheet"))
            && n.attribute("TargetMode") != Some("External")
    }) {
        if let (Some(id), Some(target)) = (r.attribute("Id"), r.attribute("Target")) {
            let path = if target.starts_with('/') {
                target.trim_start_matches('/').into()
            } else {
                format!("xl/{target}")
            };
            if path.split('/').any(|p| p == "..") {
                return Err(Error::Invalid("Invalid worksheet path"));
            }
            links.insert(id, path);
        }
    }
    let mut out = BTreeMap::new();
    for sheet in book.descendants().filter(|n| n.has_tag_name((S, "sheet"))) {
        let name = sheet
            .attribute("name")
            .ok_or(Error::Invalid("Missing sheet name"))?;
        let id = sheet
            .attributes()
            .find(|a| a.name() == "id" && a.namespace().is_some())
            .ok_or(Error::Invalid("Missing worksheet relationship"))?
            .value();
        let path = links
            .get(id)
            .ok_or(Error::Invalid("Invalid worksheet relationship"))?;
        out.insert(name.into(), path.clone());
    }
    Ok(out)
}
fn xlsx_spans(text: &str, indices: &std::collections::BTreeSet<usize>) -> Result<Vec<Span>, Error> {
    let doc = xml(text)?;
    let mut spans = Vec::new();
    for cell in doc.descendants().filter(|n| n.has_tag_name((S, "c"))) {
        let Some(f) = cell.children().find(|n| {
            n.has_tag_name((S, "f")) && matches!(n.attribute("t"), Some("array" | "dataTable"))
        }) else {
            continue;
        };
        let anchor = cell
            .attribute("r")
            .and_then(a1)
            .ok_or(Error::Invalid("Invalid array anchor"))?;
        let range = f
            .attribute("ref")
            .and_then(range)
            .ok_or(Error::Invalid("Invalid array range"))?;
        if anchor != (range.top, range.left) {
            return Err(Error::Invalid("Array range does not start at anchor"));
        }
        let cm = cell.attribute("cm").and_then(|v| v.parse().ok());
        let dynamic = f.attribute("t") == Some("array")
            && cm.is_some_and(|v| indices.contains(&v))
            && formula::is_dynamic_formula(&xlsx_formula_to_a1(f.text().unwrap_or("")));
        spans.push(Span {
            anchor,
            range,
            dynamic,
            cm,
        });
    }
    Ok(spans)
}
fn clear_children(sheet: &mut Sheet, spans: &[Span]) -> Result<(), Error> {
    if !spans.iter().any(|s| s.dynamic) {
        return Ok(());
    }
    let mut occupied = BTreeMap::new();
    let mut work = 0usize;
    for span in spans {
        let area = (span.range.bottom - span.range.top + 1)
            .checked_mul(span.range.right - span.range.left + 1)
            .ok_or(Error::Limit)?;
        work = work
            .checked_add(area)
            .filter(|v| *v <= 1_000_000)
            .ok_or(Error::Limit)?;
        for r in span.range.top..=span.range.bottom {
            for c in span.range.left..=span.range.right {
                if occupied
                    .insert((r, c), span.dynamic)
                    .is_some_and(|previous| previous || span.dynamic)
                {
                    return Err(Error::Invalid("Overlapping Office arrays"));
                }
            }
        }
    }
    for span in spans.iter().filter(|s| s.dynamic) {
        if span.range.bottom >= sheet.rows || span.range.right >= sheet.cols {
            return Err(Error::Invalid("Array range exceeds worksheet"));
        }
        for r in span.range.top..=span.range.bottom {
            for c in span.range.left..=span.range.right {
                if (r, c) == span.anchor {
                    continue;
                }
                if let Some(cell) = sheet.cells.get_mut(&(r, c)) {
                    if matches!(cell.text, CellContent::Formula(_)) {
                        return Err(Error::Invalid("Formula inside calculated array cache"));
                    }
                    cell.text = CellContent::default();
                }
            }
        }
        sheet.protected_formula_ranges.retain(|r| *r != span.range);
    }
    Ok(())
}
pub(super) fn import(bytes: &[u8], kind: &str, state: &mut State) -> Result<(), Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid Office arrays"))?;
    if kind == "xlsx" {
        let indices = zip_text(&mut archive, "xl/metadata.xml")?
            .map(|t| metadata::indices(&t))
            .transpose()?
            .unwrap_or_default();
        for (name, path) in targets(&mut archive)? {
            let text =
                zip_text(&mut archive, &path)?.ok_or(Error::Invalid("Missing array worksheet"))?;
            let spans = xlsx_spans(&text, &indices)?;
            let sheet = state
                .sheets
                .iter_mut()
                .find(|s| s.name == name)
                .ok_or(Error::Invalid("Unknown array worksheet"))?;
            clear_children(sheet, &spans)?;
        }
    } else if kind == "ods" {
        let text =
            zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("Missing ODS content"))?;
        for (name, spans) in ods::spans(&text)? {
            let sheet = state
                .sheets
                .iter_mut()
                .find(|s| s.name == name)
                .ok_or(Error::Invalid("Unknown ODS array worksheet"))?;
            sheet
                .protected_formula_ranges
                .extend(spans.iter().filter(|s| !s.dynamic).map(|s| s.range));
            clear_children(sheet, &spans)?;
        }
    }
    Ok(())
}
fn rewrite_zip(bytes: &[u8], mut entries: BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, Error> {
    if entries.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut source = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("Invalid Office package"))?;
    digest(&mut source)?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..source.len() {
        let item = source
            .by_index(i)
            .map_err(|_| Error::Invalid("Invalid Office entry"))?;
        if let Some(value) = entries.remove(item.name()) {
            writer
                .start_file(
                    item.name(),
                    zip::write::SimpleFileOptions::default().compression_method(item.compression()),
                )
                .map_err(|_| Error::Invalid("Cannot write Office entry"))?;
            writer.write_all(&value)?;
        } else {
            writer
                .raw_copy_file(item)
                .map_err(|_| Error::Invalid("Cannot preserve Office entry"))?;
        }
    }
    for (name, value) in entries {
        writer
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(|_| Error::Invalid("Cannot add Office entry"))?;
        writer.write_all(&value)?;
    }
    let out = writer
        .finish()
        .map_err(|_| Error::Invalid("Cannot finish Office package"))?
        .into_inner();
    if out.len() > LIMIT {
        return Err(Error::Limit);
    }
    let mut check = zip::ZipArchive::new(Cursor::new(out.as_slice()))
        .map_err(|_| Error::Invalid("Invalid array output"))?;
    digest(&mut check)?;
    Ok(out)
}

#[derive(Default)]
struct Update {
    input: Option<Option<CellContent>>,
    cm: Option<usize>,
    style: Option<Option<String>>,
    cache: Option<Option<formula::Value>>,
    array: Option<Option<Range>>,
}
pub(super) fn patch_ods_inputs(bytes:&[u8],changes:&[Vec<((usize,usize),Option<Cell>)>],state:&State)->Result<Vec<u8>,Error>{
    let mut archive=zip::ZipArchive::new(Cursor::new(bytes)).map_err(|_|Error::Invalid("invalid ODS package"))?;
    let text=zip_text(&mut archive,"content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
    let changes=state.sheets.iter().zip(changes).map(|(sheet,cells)|{
        let cells=cells.iter().map(|(p,cell)|(*p,Update{input:Some(cell.as_ref().map(|c|c.text.clone())),..Default::default()})).collect();
        (sheet.name.clone(),cells)
    }).collect();
    let output=ods::patch(&text,&changes)?;
    rewrite_zip(bytes,BTreeMap::from([("content.xml".into(),output.into_bytes())]))
}

fn updates(
    sheet: &Sheet,
    spans: &[Span],
    calculation: &formula::Calculation,
) -> BTreeMap<(usize, usize), Update> {
    let mut updates = BTreeMap::new();
    for span in spans.iter().filter(|s| s.dynamic) {
        let update = updates.entry(span.anchor).or_insert_with(Update::default);
        update.array = Some(None);
        update.cm = span.cm;
        for r in span.range.top..=span.range.bottom {
            for c in span.range.left..=span.range.right {
                if (r, c) != span.anchor
                    && sheet
                        .cells
                        .get(&(r, c))
                        .is_none_or(|c| c.text.raw().is_empty())
                {
                    updates.entry((r, c)).or_insert_with(Update::default).cache = Some(None);
                }
            }
        }
    }
    for (&(r, c), cell) in &sheet.cells {
        if !matches!(cell.text, CellContent::Formula(_))
            || !formula::is_dynamic_formula(cell.text.raw())
            || sheet
                .protected_formula_ranges
                .iter()
                .any(|s| r >= s.top && r <= s.bottom && c >= s.left && c <= s.right)
        {
            continue;
        }
        let range = calculation.spills.get(&(r, c)).copied().unwrap_or(Range {
            top: r,
            left: c,
            bottom: r,
            right: c,
        });
        let anchor = updates.entry((r, c)).or_insert_with(Update::default);
        anchor.array = Some(Some(range));
        anchor.cache = Some(calculation.values.get(&(r, c)).cloned());
    }
    for (range_anchor, range) in &calculation.spills {
        for r in range.top..=range.bottom {
            for c in range.left..=range.right {
                if (r, c) != *range_anchor {
                    updates.entry((r, c)).or_insert_with(Update::default).cache =
                        Some(calculation.values.get(&(r, c)).cloned());
                }
            }
        }
    }
    updates
}
pub(super) fn refresh(
    bytes: Vec<u8>,
    book: &Workbook,
    kind: &str,
    calculations: &BTreeMap<String, formula::Calculation>,
) -> Result<Vec<u8>, Error> {
    let Some(original) = book.external_source.as_deref() else {
        return Ok(bytes);
    };
    let mut input = zip::ZipArchive::new(Cursor::new(original))
        .map_err(|_| Error::Invalid("Invalid array source"))?;
    let mut output = zip::ZipArchive::new(Cursor::new(bytes.as_slice()))
        .map_err(|_| Error::Invalid("Invalid array output"))?;
    let mut entries = BTreeMap::new();
    if kind == "xlsx" {
        let old_metadata = zip_text(&mut input, "xl/metadata.xml")?.unwrap_or_default();
        let old_indices = if old_metadata.is_empty() {
            Default::default()
        } else {
            metadata::indices(&old_metadata)?
        };
        let original_targets = targets(&mut input)?;
        let current_targets = targets(&mut output)?;
        let mut pending = Vec::new();
        let mut needs_metadata = false;
        for sheet in &book.history.current().sheets {
            let old_name = book
                .external_baseline
                .as_ref()
                .and_then(|s| s.sheets.iter().find(|s| s.id == sheet.id))
                .map(|s| s.name.as_str());
            let old_text = old_name
                .and_then(|n| original_targets.get(n))
                .map(|p| zip_text(&mut input, p))
                .transpose()?
                .flatten();
            let old_spans = old_text
                .as_deref()
                .map(|s| xlsx_spans(s, &old_indices))
                .transpose()?
                .unwrap_or_default();
            let calc = calculations
                .get(&sheet.name)
                .ok_or(Error::Invalid("Missing array calculation"))?;
            let changes = updates(sheet, &old_spans, calc);
            if changes.is_empty() {
                continue;
            }
            needs_metadata |= changes.values().any(|u| matches!(u.array, Some(Some(_))));
            let path = current_targets
                .get(&sheet.name)
                .ok_or(Error::Invalid("Missing array sheet target"))?;
            let text =
                zip_text(&mut output, path)?.ok_or(Error::Invalid("Missing array output sheet"))?;
            pending.push((path.clone(), text, changes));
        }
        let cm = if needs_metadata {
            metadata::ensure(&mut output, &mut entries)?
        } else {
            0
        };
        for (path, text, changes) in pending {
            entries.insert(path, xlsx::patch(&text, &changes, cm)?.into_bytes());
        }
    } else if kind == "ods" {
        let old_text = zip_text(&mut input, "content.xml")?
            .ok_or(Error::Invalid("Missing ODS array source"))?;
        let old_spans = ods::spans(&old_text)?;
        let text = zip_text(&mut output, "content.xml")?
            .ok_or(Error::Invalid("Missing ODS array output"))?;
        let mut changes = BTreeMap::new();
        for sheet in &book.history.current().sheets {
            let old_name = book
                .external_baseline
                .as_ref()
                .and_then(|s| s.sheets.iter().find(|s| s.id == sheet.id))
                .map(|s| s.name.as_str());
            let spans = old_name
                .and_then(|n| old_spans.get(n))
                .map_or(&[][..], Vec::as_slice);
            let calc = calculations
                .get(&sheet.name)
                .ok_or(Error::Invalid("Missing ODS array calculation"))?;
            let update = updates(sheet, spans, calc);
            if !update.is_empty() {
                changes.insert(sheet.name.clone(), update);
            }
        }
        if !changes.is_empty() {
            entries.insert(
                "content.xml".into(),
                ods::patch(&text, &changes)?.into_bytes(),
            );
        }
    }
    rewrite_zip(&bytes, entries)
}

/// Remove only writer-injected value defaults where an unchanged source cell had no explicit style.
pub(super) fn preserve_ods_defaults(
    original: &[u8],
    generated: Vec<u8>,
    changes: &[Vec<((usize, usize), Option<Cell>)>],
    old: &State,
    current: &State,
) -> Result<Vec<u8>, Error> {
    let a = spreadsheet_ods::read_ods_buf(original)
        .map_err(|_| Error::Invalid("Invalid ODS style source"))?;
    let b = spreadsheet_ods::read_ods_buf(&generated)
        .map_err(|_| Error::Invalid("Invalid ODS style output"))?;
    let mut updates = BTreeMap::new();
    for (i, sheet) in old.sheets.iter().enumerate() {
        let changed: std::collections::BTreeSet<_> = changes[i]
            .iter()
            .filter_map(|(coord, after)| {
                let before = sheet.cells.get(coord).map_or(0, |c| c.style);
                let after = after.as_ref().map_or(0, |c| c.style);
                (before != after).then_some(*coord)
            })
            .collect();
        let rows = sheet.rows.max(b.sheet(i).used_grid_size().0 as usize);
        let cols = sheet.cols.max(b.sheet(i).used_grid_size().1 as usize);
        if rows > MAX_ROWS || cols > MAX_COLS {
            return Err(Error::Limit);
        }
        let mut cells = BTreeMap::new();
        for r in 0..rows {
            for c in 0..cols {
                if changed.contains(&(r, c))
                    || current.sheets[i].source_structure
                    || a.sheet(i).cellstyle(r as u32, c as u32).is_some()
                {
                    continue;
                }
                let after = b.sheet(i).cellstyle(r as u32, c as u32);
                if after.is_some()
                    && after == b.def_style(b.sheet(i).value(r as u32, c as u32).value_type())
                {
                    cells.insert(
                        (r, c),
                        Update {
                            style: Some(None),
                            ..Update::default()
                        },
                    );
                }
            }
        }
        if !cells.is_empty() {
            updates.insert(sheet.name.clone(), cells);
        }
    }
    if updates.is_empty() {
        return Ok(generated);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(generated.as_slice()))
        .map_err(|_| Error::Invalid("Invalid ODS style package"))?;
    let text = zip_text(&mut archive, "content.xml")?
        .ok_or(Error::Invalid("Missing ODS style content"))?;
    let content = ods::patch(&text, &updates)?;
    rewrite_zip(
        &generated,
        BTreeMap::from([("content.xml".into(), content.into_bytes())]),
    )
}

#[cfg(test)]
mod tests;
