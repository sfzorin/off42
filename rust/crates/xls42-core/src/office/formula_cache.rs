//! Correct generated formula caches without changing formula text or unrelated ZIP entries.
use super::*;
mod external;
pub(super) fn refresh_external(bytes:Vec<u8>,book:&Workbook,kind:&str)->Result<Vec<u8>,Error>{external::refresh(bytes,book,kind)}
// Spreadsheet string escapes differ from ordinary XML text. Preserve literal
// _xHHHH_ sequences and encode characters forbidden by XML 1.0.
fn cached_text_xml(value: &str) -> String {
    let mut output = String::new();
    for (index, character) in value.char_indices() {
        let rest = &value.as_bytes()[index..];
        if character == '_'
            && rest.len() >= 7
            && &rest[..2] == b"_x"
            && rest[2..6].iter().all(u8::is_ascii_hexdigit)
            && rest[6] == b'_'
        {
            output.push_str("_x005F_");
        } else if (character < ' ' && !matches!(character, '\t' | '\n'))
            || matches!(character, '\u{fffe}' | '\u{ffff}')
        {
            output.push_str(&format!("_x{:04X}_", character as u32));
        } else {
            match character {
                '&' => output.push_str("&amp;"),
                '<' => output.push_str("&lt;"),
                '>' => output.push_str("&gt;"),
                _ => output.push(character),
            }
        }
    }
    output
}
pub(super) fn patch_text_results(
    bytes: Vec<u8>,
    results: &BTreeMap<usize, BTreeMap<String, String>>,
) -> Result<Vec<u8>, Error> {
    if results.is_empty() {
        return Ok(bytes);
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(&bytes))
        .map_err(|_| Error::Invalid("Invalid generated XLSX"))?;
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| Error::Invalid("Invalid generated XLSX entry"))?;
        let sheet = entry
            .name()
            .strip_prefix("xl/worksheets/sheet")
            .and_then(|p| p.strip_suffix(".xml"))
            .and_then(|p| p.parse::<usize>().ok());
        let Some(values) = sheet.and_then(|sheet| results.get(&sheet)) else {
            writer
                .raw_copy_file(entry)
                .map_err(|_| Error::Invalid("Cannot preserve generated XLSX entry"))?;
            continue;
        };
        if entry.size() > LIMIT as u64 {
            return Err(Error::Limit);
        }
        let mut source = String::new();
        entry.read_to_string(&mut source)?;
        let document = roxmltree::Document::parse(&source)
            .map_err(|_| Error::Invalid("Invalid generated worksheet XML"))?;
        let mut edits = vec![];
        let mut seen = std::collections::BTreeSet::new();
        for cell in document.descendants().filter(|n| n.has_tag_name("c")) {
            let Some((address, value)) = cell
                .attribute("r")
                .and_then(|address| values.get(address).map(|value| (address, value)))
            else {
                continue;
            };
            if !seen.insert(address) {
                return Err(Error::Invalid("Duplicate generated formula cell"));
            }
            let formula = cell.children().find(|n| n.has_tag_name("f"));
            let cached = cell
                .children()
                .find(|n| n.has_tag_name("v"))
                .ok_or(Error::Invalid("Missing generated formula cache"))?;
            if formula.is_none() {
                return Err(Error::Invalid("Missing generated formula"));
            }
            let mut opening = String::from("<c");
            for attribute in cell.attributes().filter(|a| a.name() != "t") {
                if attribute.namespace().is_some() {
                    return Err(Error::Invalid("Unsupported generated cell attribute"));
                }
                opening.push_str(&format!(
                    " {}=\"{}\"",
                    attribute.name(),
                    escape_xml(attribute.value())
                ));
            }
            opening.push_str(" t=\"str\">");
            let begin = cell.range().start;
            let end = begin
                + source[begin..]
                    .find('>')
                    .ok_or(Error::Invalid("Invalid generated cell opening"))?
                + 1;
            edits.push((begin, end, opening));
            edits.push((
                cached.range().start,
                cached.range().end,
                format!("<v>{}</v>", cached_text_xml(value)),
            ));
        }
        if seen.len() != values.len() {
            return Err(Error::Invalid("Generated formula cache is missing"));
        }
        edits.sort_by_key(|edit| std::cmp::Reverse(edit.0));
        for (start, end, value) in edits {
            source.replace_range(start..end, &value);
        }
        writer
            .start_file(
                entry.name(),
                zip::write::SimpleFileOptions::default().compression_method(entry.compression()),
            )
            .map_err(|_| Error::Invalid("Cannot encode generated worksheet"))?;
        writer.write_all(source.as_bytes())?;
    }
    writer
        .finish()
        .map(|out| out.into_inner())
        .map_err(|_| Error::Invalid("Cannot finish generated workbook"))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn part(bytes: &[u8], name: &str) -> String {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut text = String::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        text
    }
    #[test]
    fn formula_text_caches_escape_literal_excel_sequences_and_invalid_xml_characters() {
        let mut book = Workbook::new(20, 8).unwrap();
        book.set_cell(0, 0, "=\"_x0041_<&>\"".into()).unwrap();
        book.set_cell(1, 0, "=\"A\u{1}B\"".into()).unwrap();
        let xlsx = book.xlsx_bytes().unwrap();
        let source = part(&xlsx, "xl/worksheets/sheet1.xml");
        let document = roxmltree::Document::parse(&source).unwrap();
        let values: Vec<_> = document
            .descendants()
            .filter(|n| n.has_tag_name("v"))
            .map(|n| n.text().unwrap_or_default())
            .collect();
        assert_eq!(values, ["_x005F_x0041_<&>", "A_x0001_B"]);
    }
    #[test]
    fn generated_formula_caches_preserve_text_numbers_booleans_and_errors() {
        let mut book = Workbook::new(20, 8).unwrap();
        for (row, formula) in [
            "=DEC2BIN(1,8)",
            "=\"TRUE\"",
            "=TRUE()",
            "=\"\"",
            "=\"#NUM!\"",
            "=1/0",
            "=DEC2BIN(512)",
            "=42",
        ]
        .into_iter()
        .enumerate()
        {
            book.set_cell(row, 0, formula.into()).unwrap();
        }
        let xlsx = book.xlsx_bytes().unwrap();
        let text = part(&xlsx, "xl/worksheets/sheet1.xml");
        let doc = roxmltree::Document::parse(&text).unwrap();
        for (row, (kind, value)) in [
            (Some("str"), "00000001"),
            (Some("str"), "TRUE"),
            (Some("b"), "1"),
            (Some("str"), ""),
            (Some("str"), "#NUM!"),
            (Some("e"), "#DIV/0!"),
            (Some("e"), "#NUM!"),
            (None, "42"),
        ]
        .into_iter()
        .enumerate()
        {
            let address = format!("A{}", row + 1);
            let cell = doc
                .descendants()
                .find(|n| n.has_tag_name("c") && n.attribute("r") == Some(address.as_str()))
                .unwrap();
            assert_eq!(cell.attribute("t"), kind, "{address}");
            assert_eq!(
                cell.children()
                    .find(|n| n.has_tag_name("v"))
                    .unwrap()
                    .text()
                    .unwrap_or_default(),
                value,
                "{address}"
            );
            assert!(cell.children().any(|n| n.has_tag_name("f")));
        }
        let ods = book.ods_bytes().unwrap();
        let text = part(&ods, "content.xml");
        let doc = roxmltree::Document::parse(&text).unwrap();
        let table = "urn:oasis:names:tc:opendocument:xmlns:table:1.0";
        let office = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
        let cells: Vec<_> = doc
            .descendants()
            .filter(|n| n.has_tag_name("table-cell") && n.attribute((table, "formula")).is_some())
            .collect();
        assert_eq!(cells.len(), 8);
        assert_eq!(cells[0].attribute((office, "value-type")), Some("string"));
        assert_eq!(
            cells[0]
                .descendants()
                .find(|n| n.has_tag_name("p"))
                .unwrap()
                .text(),
            Some("00000001")
        );
        assert_eq!(cells[1].attribute((office, "value-type")), Some("string"));
        assert_eq!(cells[2].attribute((office, "value-type")), Some("boolean"));
        assert_eq!(cells[2].attribute((office, "boolean-value")), Some("true"));
        assert_eq!(cells[7].attribute((office, "value-type")), Some("float"));
        assert_eq!(cells[7].attribute((office, "value")), Some("42"));
    }
}

#[cfg(test)]
mod compatibility_prefixes {
    use super::*;
    #[test]
    fn import_hides_excel_function_prefixes_but_keeps_strings_and_quoted_names() {
        for (source,expected)in [
            ("=_xlfn.STDEV.S(A1:A5)","=STDEV.S(A1:A5)"),
            ("_xlfn._xlws.FILTER(A1:A3,A1:A3>2)","=FILTER(A1:A3,A1:A3>2)"),
            ("=_XLFN.FORMULATEXT (INDEX(A1:A3,1))","=FORMULATEXT (INDEX(A1:A3,1))"),
            ("=\"_xlfn.STDEV.S(1)\"&\"Привет\"","=\"_xlfn.STDEV.S(1)\"&\"Привет\""),
            ("='_xlfn.Sheet''s'!A1+_xlfn.MEDIAN(A1:A3)","='_xlfn.Sheet''s'!A1+MEDIAN(A1:A3)"),
            ("=\"a\"\"_xlfn.SUM(\"&A1","=\"a\"\"_xlfn.SUM(\"&A1"),
            ("=_xlfn.named_value+A1","=_xlfn.named_value+A1"),
        ]{assert_eq!(xlsx_formula_to_a1(source),expected);}
        let mut book=Workbook::new(8,4).unwrap();book.set_cell(0,0,"1".into()).unwrap();book.set_cell(1,0,"2".into()).unwrap();book.set_cell(0,1,"=STDEV.S(A1:A2)".into()).unwrap();book.set_cell(0,2,"=FORMULATEXT(B1)".into()).unwrap();
        let source=book.xlsx_bytes().unwrap();let mut archive=zip::ZipArchive::new(Cursor::new(source)).unwrap();let mut writer=zip::ZipWriter::new(Cursor::new(Vec::new()));
        for index in 0..archive.len(){let file=archive.by_index(index).unwrap();if file.name()!=EXTRA{writer.raw_copy_file(file).unwrap();}}
        let bytes=writer.finish().unwrap().into_inner();let mut imported=Workbook::from_office_bytes(&bytes,"xlsx").unwrap();assert_eq!(imported.cell(0,1),Some("=STDEV.S(A1:A2)"));
        imported.set_cell(0,0,"4".into()).unwrap();assert_eq!(imported.display_grid()[0][2],"=STDEV.S(A1:A2)");
        let output=imported.xlsx_bytes().unwrap();let mut archive=zip::ZipArchive::new(Cursor::new(output.as_slice())).unwrap();let sheet=zip_text(&mut archive,"xl/worksheets/sheet1.xml").unwrap().unwrap();
        assert!(sheet.contains("<f>_xlfn.STDEV.S(A1:A2)</f>"));let doc=xml(&sheet).unwrap();let c=doc.descendants().find(|n|n.has_tag_name("c")&&n.attribute("r")==Some("C1")).unwrap();assert_eq!(c.children().find(|n|n.has_tag_name("v")).unwrap().text(),Some("=STDEV.S(A1:A2)"));
    }
}
