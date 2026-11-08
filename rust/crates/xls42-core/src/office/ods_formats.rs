//! Standard fixed-decimal, zero-pad, grouping and percentage styles; original numeric values stay exact.
use super::*;
const N: &str = "urn:oasis:names:tc:opendocument:xmlns:datastyle:1.0";
const S: &str = "urn:oasis:names:tc:opendocument:xmlns:style:1.0";
const O: &str = "urn:oasis:names:tc:opendocument:xmlns:office:1.0";
const T: &str = "urn:oasis:names:tc:opendocument:xmlns:text:1.0";
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Number {
    integer: usize,
    decimals: usize,
    grouping: bool,
    percent: bool,
}
impl Number {
    fn parse(code: &str) -> Option<Self> {
        let code = code.trim();
        let (body, percent) = code.strip_suffix('%').map_or((code, false), |v| (v, true));
        if body.ends_with('.') {
            return None;
        }
        let (integer, decimals) = body.split_once('.').unwrap_or((body, ""));
        let (grouping, integer) = integer
            .strip_prefix("#,##")
            .map_or((false, integer), |v| (true, v));
        if integer.is_empty()
            || integer.len() > 255
            || !integer.bytes().all(|v| v == b'0')
            || decimals.len() > 127
            || !decimals.bytes().all(|v| v == b'0')
        {
            return None;
        }
        Some(Self {
            integer: integer.len(),
            decimals: decimals.len(),
            grouping,
            percent,
        })
    }
    fn code(self) -> String {
        format!(
            "{}{}{}{}",
            if self.grouping { "#,##" } else { "" },
            "0".repeat(self.integer),
            if self.decimals == 0 {
                String::new()
            } else {
                format!(".{}", "0".repeat(self.decimals))
            },
            if self.percent { "%" } else { "" }
        )
    }
    fn xml(self, name: &str) -> String {
        let kind = if self.percent {
            "percentage-style"
        } else {
            "number-style"
        };
        format!(
            "<number:{kind} xmlns:number=\"{N}\" xmlns:style=\"{S}\" style:name=\"{name}\" number:language=\"en\" number:country=\"US\"><number:number number:min-integer-digits=\"{}\" number:decimal-places=\"{}\" number:min-decimal-places=\"{}\" number:grouping=\"{}\"/>{}</number:{kind}>",
            self.integer,
            self.decimals,
            self.decimals,
            self.grouping,
            if self.percent {
                "<number:text>%</number:text>"
            } else {
                ""
            }
        )
    }
    fn read(node: roxmltree::Node<'_, '_>) -> Option<Self> {
        let percent = node.has_tag_name((N, "percentage-style"));
        if !percent && !node.has_tag_name((N, "number-style")) {
            return None;
        }
        if node.attribute((N, "language")).is_some_and(|s| s != "en")
            || node.attribute((N, "country")).is_some_and(|s| s != "US")
            || node
                .attribute((N, "rfc-language-tag"))
                .is_some_and(|s| !matches!(s, "en" | "en-US"))
        {
            return None;
        }
        let children = node
            .children()
            .filter(|n| n.is_element())
            .collect::<Vec<_>>();
        if children.len() != if percent { 2 } else { 1 }
            || !children[0].has_tag_name((N, "number"))
            || percent
                && (!children[1].has_tag_name((N, "text")) || children[1].text() != Some("%"))
        {
            return None;
        }
        let number = children[0];
        if number.children().any(|n| n.is_element())
            || number.attributes().any(|a| {
                a.namespace() != Some(N)
                    || !matches!(
                        a.name(),
                        "min-integer-digits" | "decimal-places" | "min-decimal-places" | "grouping"
                    )
            })
        {
            return None;
        }
        let integer = number
            .attribute((N, "min-integer-digits"))
            .unwrap_or("1")
            .parse::<usize>()
            .ok()?;
        let decimals = number
            .attribute((N, "decimal-places"))?
            .parse::<usize>()
            .ok()?;
        let minimum = number
            .attribute((N, "min-decimal-places"))
            .map(|v| v.parse::<usize>())
            .transpose()
            .ok()?
            .unwrap_or(decimals);
        if !(1..=255).contains(&integer) || decimals > 127 || minimum != decimals {
            return None;
        }
        let grouping = match number.attribute((N, "grouping")).unwrap_or("false") {
            "true" | "1" => true,
            "false" | "0" => false,
            _ => return None,
        };
        Some(Self {
            integer,
            decimals,
            grouping,
            percent,
        })
    }
}
pub(super) fn supported(code: &str) -> bool {
    code.trim().is_empty()
        || code.trim().eq_ignore_ascii_case("general")
        || Number::parse(code).is_some()
}
pub(super) fn presentation(code: &str, raw: &str) -> Option<(bool, String)> {
    let number = Number::parse(code)?;
    let value = raw.parse::<f64>().ok().filter(|v| v.is_finite())?;
    Some((
        number.percent,
        crate::display::number_pattern(value, &number.code())?,
    ))
}
pub(super) fn patch_native(bytes: &[u8], styles: &[CellStyle]) -> Result<Vec<u8>, Error> {
    let mut formats = BTreeMap::<Number, String>::new();
    let mut references = BTreeMap::new();
    for (i, style) in styles.iter().enumerate() {
        let Some(number) = Number::parse(&style.number_format) else {
            if supported(&style.number_format) {
                continue;
            } else {
                return Err(Error::Invalid("Unsupported ODS number format"));
            }
        };
        if !formats.contains_key(&number) && formats.len() >= 4096 {
            return Err(Error::Limit);
        }
        let next = format!("off42number{}", formats.len());
        let name = formats.entry(number).or_insert(next).clone();
        references.insert(format!("xls42style{i}"), (name, number));
    }
    if references.is_empty() {
        return Ok(bytes.to_vec());
    }
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid ODS package"))?;
    let content =
        zip_text(&mut archive, "content.xml")?.ok_or(Error::Invalid("missing ODS content"))?;
    let doc = xml(&content)?;
    let mut edits = Vec::new();
    let mut estimated = content.len();
    for node in doc.descendants().filter(|n| n.has_tag_name((S, "style"))) {
        let Some((name, _)) = node.attribute((S, "name")).and_then(|n| references.get(n)) else {
            continue;
        };
        let changed = dynamic_arrays::xml_nodes::edit(
            &content,
            node,
            &[(S, "data-style-name", Some(name.clone()))],
            &[],
            "",
        )?;
        let range = node.range();
        estimated = estimated
            .checked_sub(range.end - range.start)
            .and_then(|n| n.checked_add(changed.len()))
            .filter(|n| *n <= LIMIT)
            .ok_or(Error::Limit)?;
        edits.push((range.start, range.end, changed));
    }
    for node in doc
        .descendants()
        .filter(|n| n.has_tag_name((sparse_ods::TABLE, "table-cell")))
    {
        let Some((_, number)) = node
            .attribute((sparse_ods::TABLE, "style-name"))
            .and_then(|n| references.get(n))
        else {
            continue;
        };
        if !matches!(
            node.attribute((O, "value-type")),
            Some("float" | "percentage")
        ) {
            continue;
        }
        let Some(value) = node
            .attribute((O, "value"))
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite())
        else {
            continue;
        };
        let text = crate::display::number_pattern(value, &number.code())
            .ok_or(Error::Invalid("Cannot render ODS number format"))?;
        let changed = dynamic_arrays::xml_nodes::edit(
            &content,
            node,
            &[(
                O,
                "value-type",
                Some(
                    if number.percent {
                        "percentage"
                    } else {
                        "float"
                    }
                    .into(),
                ),
            )],
            &[(T, "p")],
            &format!("<text:p xmlns:text=\"{T}\">{}</text:p>", escape_xml(&text)),
        )?;
        let range = node.range();
        estimated = estimated
            .checked_sub(range.end - range.start)
            .and_then(|n| n.checked_add(changed.len()))
            .filter(|n| *n <= LIMIT)
            .ok_or(Error::Limit)?;
        edits.push((range.start, range.end, changed));
    }
    let changed = apply_edits(content, edits);
    let doc = xml(&changed)?;
    let automatic = doc
        .root_element()
        .children()
        .find(|n| n.has_tag_name((O, "automatic-styles")))
        .ok_or(Error::Invalid("missing ODS automatic styles"))?;
    let definitions = formats
        .iter()
        .map(|(number, name)| number.xml(name))
        .collect::<String>();
    if changed.len().saturating_add(definitions.len()) > LIMIT {
        return Err(Error::Limit);
    }
    let changed = append_xml_children(&changed, automatic, &definitions)?;
    dynamic_arrays::rewrite_zip(
        bytes,
        BTreeMap::from([("content.xml".into(), changed.into_bytes())]),
    )
}
/// Resolve explicitly assigned or inherited cell styles from standard styles.xml/content.xml definitions.
pub(super) fn imported(bytes: &[u8]) -> Result<BTreeMap<String, String>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::Invalid("invalid ODS package"))?;
    let mut numbers = BTreeMap::new();
    let mut styles = BTreeMap::new();
    for part in ["styles.xml", "content.xml"] {
        let Some(text) = zip_text(&mut archive, part)? else {
            continue;
        };
        let doc = xml(&text)?;
        for node in doc.descendants() {
            if let Some(number) = Number::read(node) {
                if let Some(name) = node.attribute((S, "name")) {
                    numbers.insert(name.to_owned(), number.code());
                }
            }
            if node.has_tag_name((S, "style"))
                && node.attribute((S, "family")) == Some("table-cell")
            {
                if let Some(name) = node.attribute((S, "name")) {
                    styles.insert(
                        name.to_owned(),
                        (
                            node.attribute((S, "data-style-name")).map(str::to_owned),
                            node.attribute((S, "parent-style-name")).map(str::to_owned),
                        ),
                    );
                }
            }
        }
    }
    let mut result = BTreeMap::new();
    for name in styles.keys() {
        let mut current = name.as_str();
        for _ in 0..64 {
            let Some((number, parent)) = styles.get(current) else {
                break;
            };
            if let Some(number) = number {
                if let Some(code) = numbers.get(number) {
                    result.insert(name.clone(), code.clone());
                }
                break;
            }
            let Some(parent) = parent else { break };
            current = parent;
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn independent(bytes: &[u8]) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            if entry.name() == EXTRA {
                continue;
            }
            if entry.name() == "META-INF/manifest.xml" {
                let mut data = Vec::new();
                entry.read_to_end(&mut data).unwrap();
                let data = project_declaration(&data, "ods", false).unwrap();
                out.start_file(entry.name(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(&data).unwrap();
            } else {
                out.raw_copy_file(entry).unwrap();
            }
        }
        out.start_file(
            "custom/number-format.bin",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        out.write_all(b"keep unknown number part").unwrap();
        out.finish().unwrap().into_inner()
    }
    #[test]
    fn standard_number_styles_and_display_caches_preserve_unrounded_numeric_values_types_and_independent_reopen()
     {
        let cases = [
            ("0", "1234.567", "1235"),
            ("00", "7", "07"),
            ("000000", "42", "000042"),
            ("0.00", "1.23456789", "1.23"),
            ("#,##0.00", "12345.6789", "12,345.68"),
            ("0.00%", "0.123456789", "12.35%"),
            ("0%", "0.25", "25%"),
            ("0.000", "=1/3", "0.333"),
        ];
        let mut b = Workbook::new(20, 4).unwrap();
        for (r, (code, input, _)) in cases.iter().enumerate() {
            b.set_cell(r, 0, (*input).into()).unwrap();
            b.set_content(
                r,
                1,
                CellContent::Text(TextValue::from_raw("1.23456789".into())),
            )
            .unwrap();
            b.set_cell(r, 2, "TRUE".into()).unwrap();
            b.set_style(
                Range {
                    top: r,
                    left: 0,
                    bottom: r,
                    right: 2,
                },
                CellStyle {
                    number_format: (*code).into(),
                    bold: true,
                    foreground: Some(0x506070),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        b.set_cell(0, 3, "=\"0007\"".into()).unwrap();
        b.set_cell(1, 3, "=TRUE()".into()).unwrap();
        b.set_content(2, 3, CellContent::Error(TextValue::from_raw("#N/A".into())))
            .unwrap();
        for row in 0..3 {
            let style = b.style(row, 0).unwrap().clone();
            b.set_style(
                Range {
                    top: row,
                    left: 3,
                    bottom: row,
                    right: 3,
                },
                style,
            )
            .unwrap();
        }
        let original = b.project_bytes().unwrap();
        let bytes = b.ods_bytes().unwrap();
        assert_eq!(b.project_bytes().unwrap(), original);
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let content = zip_text(&mut archive, "content.xml").unwrap().unwrap();
        let doc = xml(&content).unwrap();
        let nums = doc
            .descendants()
            .filter(|n| {
                Number::read(*n).is_some()
                    && n.attribute((S, "name"))
                        .is_some_and(|v| v.starts_with("off42number"))
            })
            .count();
        assert_eq!(nums, cases.len());
        let mut rows = Vec::new();
        let mut budget = 0;
        sparse_ods::visit(
            sparse_ods::tables(&doc).next().unwrap(),
            &mut budget,
            |r, c, node| {
                if c == 0 && r < cases.len() {
                    rows.push(r);
                    assert_eq!(
                        node.children()
                            .find(|n| n.has_tag_name((T, "p")))
                            .unwrap()
                            .text(),
                        Some(cases[r].2)
                    );
                    assert_eq!(
                        node.attribute((O, "value-type")),
                        Some(if cases[r].0.ends_with('%') {
                            "percentage"
                        } else {
                            "float"
                        })
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(rows.len(), cases.len());
        let source = independent(&bytes);
        let mut opened = Workbook::from_office_bytes(&source, "ods").unwrap();
        for (r, (code, input, display)) in cases.iter().enumerate() {
            assert_eq!(opened.style(r, 0).unwrap().number_format, *code);
            assert!(opened.style(r, 0).unwrap().bold);
            assert_eq!(opened.style(r, 0).unwrap().foreground, Some(0x506070));
            assert_eq!(opened.display_cells()[&(r, 0)], *display);
            assert_eq!(opened.cell(r, 0), Some(*input));
            assert_eq!(opened.display_cells()[&(r, 1)], "1.23456789");
            assert_eq!(opened.display_cells()[&(r, 2)], "TRUE");
        }
        let typed = formula::calculation_at(&opened, chrono::Local::now().naive_local());
        assert_eq!(typed.values[&(0, 3)], formula::Value::Text("0007".into()));
        assert_eq!(typed.values[&(1, 3)], formula::Value::Boolean(true));
        assert!(matches!(opened.content(2, 3), Some(CellContent::Error(_))));
        opened.set_cell(5, 0, "0.987654321".into()).unwrap();
        let saved = opened.ods_bytes().unwrap();
        let mut saved_zip = zip::ZipArchive::new(Cursor::new(saved.as_slice())).unwrap();
        let saved_content = zip_text(&mut saved_zip, "content.xml").unwrap().unwrap();
        let saved_doc = xml(&saved_content).unwrap();
        let mut checked = 0;
        let mut work = 0;
        sparse_ods::visit(
            sparse_ods::tables(&saved_doc).next().unwrap(),
            &mut work,
            |r, c, node| {
                if c == 0 && matches!(r, 5 | 7) {
                    checked += 1;
                    assert_eq!(
                        node.children()
                            .find(|n| n.has_tag_name((T, "p")))
                            .unwrap()
                            .text(),
                        Some(if r == 5 { "98.77%" } else { "0.333" })
                    );
                    assert_eq!(
                        node.attribute((O, "value-type")),
                        Some(if r == 5 { "percentage" } else { "float" })
                    );
                }
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(checked, 2);
        let original_auto = doc
            .descendants()
            .find(|n| n.has_tag_name((O, "automatic-styles")))
            .unwrap();
        let saved_auto = saved_doc
            .descendants()
            .find(|n| n.has_tag_name((O, "automatic-styles")))
            .unwrap();
        assert_eq!(
            &content[original_auto.range()],
            &saved_content[saved_auto.range()]
        );
        let reopened = Workbook::from_office_bytes(&saved, "ods").unwrap();
        assert_eq!(reopened.display_cells()[&(5, 0)], "98.77%");
        assert_eq!(reopened.cell(5, 0), Some("0.987654321"));
        let mut archive = zip::ZipArchive::new(Cursor::new(&saved)).unwrap();
        let mut opaque = Vec::new();
        archive
            .by_name("custom/number-format.bin")
            .unwrap()
            .read_to_end(&mut opaque)
            .unwrap();
        assert_eq!(opaque, b"keep unknown number part");
        assert!(opened.undo());
        assert_eq!(opened.ods_bytes().unwrap(), source);
        assert!(opened.redo());
        assert_eq!(opened.cell(5, 0), Some("0.987654321"));
    }
    #[test]
    fn namespace_prefixes_and_parent_number_styles_resolve_while_unsupported_formats_refuse_without_mutation()
     {
        let mut b = Workbook::new(2, 2).unwrap();
        b.set_cell(0, 0, "12.3".into()).unwrap();
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            CellStyle {
                number_format: "0.00".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let source = independent(&b.ods_bytes().unwrap());
        let mut archive = zip::ZipArchive::new(Cursor::new(source.as_slice())).unwrap();
        let text = zip_text(&mut archive, "content.xml").unwrap().unwrap();
        let doc = xml(&text).unwrap();
        let auto = doc
            .descendants()
            .find(|n| n.has_tag_name((O, "automatic-styles")))
            .unwrap();
        let changed=append_xml_children(&text,auto,&format!("<style:style xmlns:style=\"{S}\" style:name=\"number-child\" style:family=\"table-cell\" style:parent-style-name=\"xls42style1\"/>")).unwrap();
        let changed = changed
            .replace("number:", "n:")
            .replace("xmlns:number=", "xmlns:n=")
            .replace(
                "table:style-name=\"xls42style1\"",
                "table:style-name=\"number-child\"",
            );
        let altered = dynamic_arrays::rewrite_zip(
            &source,
            BTreeMap::from([("content.xml".into(), changed.into_bytes())]),
        )
        .unwrap();
        let opened = Workbook::from_office_bytes(&altered, "ods").unwrap();
        assert_eq!(opened.style(0, 0).unwrap().number_format, "0.00");
        assert_eq!(opened.display_cells()[&(0, 0)], "12.30");
        let baseline = b.project_bytes().unwrap();
        for code in ["0.00E+00", "[Red]0.00", "0.##", "$0.00", "yyyy-mm-dd"] {
            let mut b = Workbook::from_project_bytes(&baseline).unwrap();
            let mut style = b.style(0, 0).unwrap().clone();
            style.number_format = code.into();
            b.set_style(
                Range {
                    top: 0,
                    left: 0,
                    bottom: 0,
                    right: 0,
                },
                style,
            )
            .unwrap();
            let before = b.project_bytes().unwrap();
            assert!(b.ods_bytes().is_err(), "{code}");
            assert_eq!(b.project_bytes().unwrap(), before);
        }
    }
}
