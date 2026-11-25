//! Normalize formula syntax for evaluation and standard Office output, preserving original editor input.
use super::localized_aliases::FUNCTIONS;
use std::borrow::Cow;
const ERRORS: &[(&str, &str)] = &[
    ("#ДЕЛ/0!", "#DIV/0!"),
    ("#ЗНАЧ!", "#VALUE!"),
    ("#ССЫЛКА!", "#REF!"),
    ("#ИМЯ?", "#NAME?"),
    ("#Н/Д", "#N/A"),
    ("#ЧИСЛО!", "#NUM!"),
    ("#ПУСТО!", "#NULL!"),
];
pub(super) fn canonical(raw: &str) -> Cow<'_, str> {
    if raw.is_ascii() && !raw.contains(';') {
        return Cow::Borrowed(raw);
    }
    let mut out = String::with_capacity(raw.len());
    let mut i = 0;
    let mut braces = 0usize;
    while i < raw.len() {
        let ch = raw[i..].chars().next().unwrap();
        if matches!(ch, '\'' | '"') {
            let start = i;
            i += 1;
            while i < raw.len() {
                let c = raw[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == ch {
                    if raw[i..].starts_with(ch) {
                        i += 1;
                    } else {
                        break;
                    }
                }
            }
            out.push_str(&raw[start..i]);
            continue;
        }
        if ch == '[' {
            let start = i;
            let mut depth = 0usize;
            while i < raw.len() {
                let c = raw[i..].chars().next().unwrap();
                i += c.len_utf8();
                if c == '[' {
                    depth += 1;
                } else if c == ']' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            out.push_str(&raw[start..i]);
            continue;
        }
        if ch == '#' {
            let prefix = raw[i..].chars().take(12).collect::<String>().to_uppercase();
            if let Some((old, new)) = ERRORS.iter().find(|(old, _)| prefix.starts_with(old)) {
                out.push_str(new);
                i += old.len();
                continue;
            }
        }
        if ch.is_alphabetic() || ch == '_' {
            let start = i;
            while i < raw.len() {
                let c = raw[i..].chars().next().unwrap();
                if !(c.is_alphanumeric() || matches!(c, '_' | '.')) {
                    break;
                }
                i += c.len_utf8();
            }
            let token = &raw[start..i];
            if raw[i..].starts_with('!') && !token.is_ascii() {
                out.push('\'');
                out.push_str(token);
                out.push('\'');
                continue;
            }
            if raw[i..].trim_start().starts_with('(') {
                let name = token.to_uppercase();
                if let Ok(index) =
                    FUNCTIONS.binary_search_by(|(alias, _)| alias.cmp(&name.as_str()))
                {
                    out.push_str(FUNCTIONS[index].1);
                    continue;
                }
            }
            out.push_str(token);
            continue;
        }
        match ch {
            '{' => {
                braces = braces.saturating_add(1);
                out.push(ch);
            }
            '}' => {
                braces = braces.saturating_sub(1);
                out.push(ch);
            }
            ';' if braces == 0 => out.push(','),
            c if c.is_whitespace() && !c.is_ascii() => out.push(' '),
            _ => out.push(ch),
        }
        i += ch.len_utf8();
    }
    if out == raw {
        Cow::Borrowed(raw)
    } else {
        Cow::Owned(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Workbook;
    use std::io::{Cursor, Read, Write};
    #[test]
    fn unquoted_unicode_sheet_names_are_quoted_for_standard_formats_and_compared_without_case() {
        let mut b = Workbook::new(10, 8).unwrap();
        b.rename_sheet(1, "СУММ").unwrap();
        b.set_cell(0, 0, "7".into()).unwrap();
        b.add_sheet("Results").unwrap();
        b.set_cell(0, 0, "=СУММ(сумм!A:A;5)".into()).unwrap();
        assert_eq!(b.calculated_cells()[&(0, 0)], "12");
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let opened = Workbook::from_office_bytes(&independent(&bytes), kind).unwrap();
            assert_eq!(opened.cell(0, 0), Some("=SUM('сумм'!A:A,5)"));
            assert_eq!(opened.calculated_cells()[&(0, 0)], "12");
        }
    }
    fn independent(bytes: &[u8]) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            if entry.name() == "xls42/project.json" {
                continue;
            }
            if matches!(
                entry.name(),
                "[Content_Types].xml" | "META-INF/manifest.xml"
            ) {
                let mut text = String::new();
                entry.read_to_string(&mut text).unwrap();
                let doc = roxmltree::Document::parse(&text).unwrap();
                let mut remove = doc
                    .descendants()
                    .filter(|n| {
                        n.is_element()
                            && n.attributes()
                                .any(|a| a.value().trim_start_matches('/') == "xls42/project.json")
                    })
                    .map(|n| n.range())
                    .collect::<Vec<_>>();
                remove.sort_by_key(|r| std::cmp::Reverse(r.start));
                for range in remove {
                    text.replace_range(range, "");
                }
                out.start_file(entry.name(), zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(text.as_bytes()).unwrap();
            } else {
                out.raw_copy_file(entry).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }
    #[test]
    fn lexical_normalization_distinguishes_argument_separators_from_array_rows_strings_and_sheet_headers()
     {
        for (raw, expected) in [
            ("=сумм (1;2)", "=SUM (1,2)"),
            ("=СУММ({1,2;3,4};5)", "=SUM({1,2;3,4},5)"),
            (
                "=ЕСЛИ(ЕОШИБКА(#ДЕЛ/0!);\"да;\";\"нет\")",
                "=IF(ISERROR(#DIV/0!),\"да;\",\"нет\")",
            ),
            (
                "=СЦЕП(\"СУММ(1;2)\";\"a\"\";b\")",
                "=CONCAT(\"СУММ(1;2)\",\"a\"\";b\")",
            ),
            (
                "=СУММ('Data''s; sheet'!A1;T[СУММ(1;2)])",
                "=SUM('Data''s; sheet'!A1,T[СУММ(1;2)])",
            ),
            ("=СУММ(СУММ!A1;A2)", "=SUM('СУММ'!A1,A2)"),
        ] {
            assert_eq!(canonical(raw), expected);
        }
    }
    #[test]
    fn russian_functions_semicolon_arguments_errors_and_spills_calculate_and_export_standard_office_syntax()
     {
        let mut b = Workbook::new(30, 16).unwrap();
        b.set_cell(0, 0, "10".into()).unwrap();
        b.set_cell(1, 0, "20".into()).unwrap();
        let cases = [
            ("=СУММ(A1:A2;5)", "35"),
            ("=срзнач(A1:A2)", "15"),
            ("=ЕСЛИ(ЕОШИБКА(#ДЕЛ/0!);\"да\";\"нет\")", "да"),
            ("=СУММ({1,2;3,4};5)", "15"),
            ("=ЕСЛИОШИБКА(#Н/Д;99)", "99"),
            ("=ЕСЛИОШИБКА(#н/д;33)", "33"),
            ("=ПРЕОБР(1;\"cm\";\"mm\")", "10"),
            ("=ОКРУГЛ(2.345;2)", "2.35"),
            ("=СЧЁТЕСЛИ(A:A;\">10\")", "1"),
            ("=ИНДЕКС(A:A;2)", "20"),
            ("=СУММ(ДВССЫЛ(\"A:A\"))", "30"),
            ("=СЦЕП(\"a;\";\"СУММ(1;2)\")", "a;СУММ(1;2)"),
            ("=ЕСЛИОШИБКА(#ЗНАЧ!;\"ошибка\")", "ошибка"),
            ("=ЛОЖЬ()", "FALSE"),
        ];
        for (r, (formula, _)) in cases.iter().enumerate() {
            b.set_cell(r, 1, (*formula).into()).unwrap();
        }
        b.set_cell(0, 4, "=ПОСЛЕДОВ(2;2;5;1)".into()).unwrap();
        b.set_cell(0, 7, "=СУММ(E1:F2)".into()).unwrap();
        b.set_cell(0, 10, "=ТЕКСТРАЗД(\"a|b\";\"|\")".into())
            .unwrap();
        let original = b.project_bytes().unwrap();
        let values = b.calculated_cells();
        for (r, (formula, expected)) in cases.iter().enumerate() {
            assert_eq!(&values[&(r, 1)], expected, "{formula}");
            assert_eq!(b.cell(r, 1), Some(*formula));
        }
        for (row, col, expected) in [
            (0, 4, "5"),
            (0, 5, "6"),
            (1, 4, "7"),
            (1, 5, "8"),
            (0, 7, "26"),
            (0, 10, "a"),
            (0, 11, "b"),
        ] {
            assert_eq!(values[&(row, col)], expected);
        }
        assert_eq!(b.project_bytes().unwrap(), original);
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let native = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(native.cell(0, 1), b.cell(0, 1));
            assert_eq!(native.calculated_cells(), values);
            let opened = Workbook::from_office_bytes(&independent(&bytes), kind).unwrap();
            assert_eq!(opened.cell(0, 1), Some("=SUM(A1:A2,5)"));
            assert_eq!(opened.calculated_cells(), values);
            assert_eq!(opened.cell(3, 1), Some("=SUM({1,2;3,4},5)"));
        }
    }
    #[test]
    fn localized_external_formula_edit_keeps_other_cells_parts_and_full_history() {
        let mut seed = Workbook::new(10, 8).unwrap();
        seed.set_cell(0, 0, "7".into()).unwrap();
        seed.set_cell(0, 1, "KEEP".into()).unwrap();
        for (kind, bytes) in [
            ("xlsx", seed.xlsx_bytes().unwrap()),
            ("ods", seed.ods_bytes().unwrap()),
        ] {
            let source = independent(&bytes);
            let mut b = Workbook::from_office_bytes(&source, kind).unwrap();
            let before = b.history.current().clone();
            b.set_cell(0, 0, "=СУММ(2;3)".into()).unwrap();
            let changed = b.history.current().clone();
            let saved = if kind == "xlsx" {
                b.xlsx_bytes()
            } else {
                b.ods_bytes()
            }
            .unwrap();
            let opened = Workbook::from_office_bytes(&saved, kind).unwrap();
            assert_eq!(opened.cell(0, 0), Some("=SUM(2,3)"));
            assert_eq!(opened.calculated_cells()[&(0, 0)], "5");
            assert_eq!(opened.cell(0, 1), Some("KEEP"));
            assert!(b.undo());
            assert!(b.history.current() == &before);
            assert_eq!(
                if kind == "xlsx" {
                    b.xlsx_bytes()
                } else {
                    b.ods_bytes()
                }
                .unwrap(),
                source
            );
            assert!(b.redo());
            assert!(b.history.current() == &changed);
            assert_eq!(b.external_source.as_deref(), Some(source.as_slice()));
        }
    }
}
