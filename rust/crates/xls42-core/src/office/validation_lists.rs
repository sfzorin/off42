use super::*;

const SHEET: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// Project standard inline, visible list rules with stop-style validation.
/// Unsupported formulas and alert policies remain in the original package.
pub(super) fn import_xlsx(document: &roxmltree::Document<'_>, sheet: &mut Sheet) {
    let Some(container) = document
        .root_element()
        .children()
        .find(|node| node.has_tag_name((SHEET, "dataValidations")))
    else {
        return;
    };
    for node in container
        .children()
        .filter(|node| node.has_tag_name((SHEET, "dataValidation")))
    {
        if node.attribute("type") != Some("list")
            || !matches!(node.attribute("showErrorMessage"), Some("1" | "true"))
            || !matches!(node.attribute("errorStyle"), None | Some("stop"))
            || !matches!(node.attribute("showDropDown"), None | Some("0" | "false"))
        {
            continue;
        }
        let Some(values) = node
            .children()
            .find(|node| node.has_tag_name((SHEET, "formula1")))
            .and_then(|node| node.text())
            .and_then(inline_values)
        else {
            continue;
        };
        let Some(ranges) = node.attribute("sqref").and_then(|references| {
            references
                .split_ascii_whitespace()
                .take(257)
                .map(dynamic_arrays::range)
                .collect::<Option<Vec<_>>>()
        }) else {
            continue;
        };
        if sheet.validations.len() + ranges.len() > 256
            || ranges.is_empty()
            || ranges.iter().enumerate().any(|(index, range)| {
                ranges[..index].iter().any(|other| other.overlaps(*range))
                    || sheet
                        .validations
                        .iter()
                        .any(|other| other.range.overlaps(*range))
            })
        {
            continue;
        }
        let allow_blank = matches!(node.attribute("allowBlank"), Some("1" | "true"));
        for range in ranges {
            sheet.rows = sheet.rows.max(range.bottom + 1);
            sheet.cols = sheet.cols.max(range.right + 1);
            sheet.validations.push(DataValidation {
                range,
                allow_blank,
                kind: ValidationKind::List {
                    values: values.clone(),
                },
            });
        }
    }
}

fn inline_values(formula: &str) -> Option<Vec<String>> {
    let formula = formula.trim();
    let literal = formula.strip_prefix('"')?.strip_suffix('"')?;
    if literal.is_empty() || literal.encode_utf16().count() > 255 {
        return None;
    }
    let mut plain = String::new();
    let mut chars = literal.chars();
    while let Some(ch) = chars.next() {
        if ch == '"' && chars.next() != Some('"') {
            return None;
        }
        plain.push(ch);
    }
    Some(plain.split(',').map(str::to_owned).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_xlsxwriter::DataValidation as ExcelValidation;

    #[test]
    fn external_list_choices_preserve_original_standard_validation_xml() {
        fn sheet_xml(bytes: &[u8]) -> String {
            let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            zip_text(&mut zip, "xl/worksheets/sheet1.xml")
                .unwrap()
                .unwrap()
        }
        fn validation(text: &str) -> &str {
            let doc = xml(text).unwrap();
            let container = doc
                .descendants()
                .find(|node| node.has_tag_name((SHEET, "dataValidations")))
                .unwrap();
            &text[container.range()]
        }
        let mut source = rust_xlsxwriter::Workbook::new();
        let sheet = source.add_worksheet();
        let rule = ExcelValidation::new()
            .allow_list_strings(&["007", "=40+2"])
            .unwrap();
        sheet.add_data_validation(20, 7, 21, 7, &rule).unwrap();
        let bytes = source.save_to_buffer().unwrap();
        let original = sheet_xml(&bytes);
        let mut book = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert!(book.has_external_source());
        assert!(book.validation_at(20, 7).is_some());
        assert!(book.set_list_choice(20, 7, "007".into()).unwrap());
        assert!(
            matches!(book.content(20, 7), Some(CellContent::Text(text)) if text.raw() == "007")
        );
        assert!(book.set_cell(21, 7, "invalid".into()).is_err());
        let changed = book.xlsx_bytes().unwrap();
        assert_eq!(validation(&original), validation(&sheet_xml(&changed)));
        let reopened = Workbook::from_office_bytes(&changed, "xlsx").unwrap();
        assert_eq!(reopened.cell(20, 7), Some("007"));
        assert!(reopened.validation_at(21, 7).is_some());
    }
}
