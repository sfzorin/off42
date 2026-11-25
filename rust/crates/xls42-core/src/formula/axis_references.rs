//! Whole-row/column reference syntax; endpoints stay within the global worksheet address space.
use super::*;
impl Parser<'_, '_> {
    fn axis_endpoint(&mut self) -> Option<(bool, usize)> {
        self.space();
        if self.input.get(self.pos) == Some(&b'$') {
            self.pos += 1;
        }
        let begin = self.pos;
        let column = self.input.get(self.pos)?.is_ascii_alphabetic();
        let mut value = 0usize;
        while let Some(&ch) = self.input.get(self.pos) {
            if column && ch.is_ascii_alphabetic() {
                value = value
                    .checked_mul(26)?
                    .checked_add(usize::from(ch.to_ascii_uppercase() - b'A') + 1)?;
            } else if !column && ch.is_ascii_digit() {
                value = value.checked_mul(10)?.checked_add(usize::from(ch - b'0'))?;
            } else {
                break;
            }
            self.pos += 1;
        }
        if self.pos == begin
            || value == 0
            || value
                > if column {
                    crate::MAX_COLS
                } else {
                    crate::MAX_ROWS
                }
        {
            return None;
        }
        Some((column, value - 1))
    }
    pub(super) fn read_whole_axis_range(
        &mut self,
    ) -> Option<(SheetId, usize, usize, usize, usize)> {
        let start = self.pos;
        let result = (|| {
            let first_start = self.pos;
            let sheet = if let Some(id) = self.read_sheet_qualifier() {
                id
            } else {
                self.pos = first_start;
                self.sheet
            };
            let (column, first) = self.axis_endpoint()?;
            if !self.take(b':') {
                return None;
            }
            let second_start = self.pos;
            if let Some(other) = self.read_sheet_qualifier() {
                if other != sheet {
                    return None;
                }
            } else {
                self.pos = second_start;
            }
            let (last_column, last) = self.axis_endpoint()?;
            if column != last_column || first > last {
                return None;
            }
            Some(if column {
                (sheet, 0, first, crate::MAX_ROWS - 1, last)
            } else {
                (sheet, first, 0, last, crate::MAX_COLS - 1)
            })
        })();
        if result.is_none() {
            self.pos = start;
        }
        result
    }
}
#[cfg(test)]
mod tests {
    use crate::Workbook;
    use std::io::{Cursor, Read, Write};
    #[test]
    fn whole_axis_ranges_qualified_names_and_implicit_empty_references_have_excel_boundaries() {
        let mut b = Workbook::new(40, 8).unwrap();
        b.rename_sheet(1, "Data's sheet").unwrap();
        for (r, c, text) in [(0, 0, "10"), (19, 0, "20"), (2, 1, "text"), (3, 1, "=\"\"")] {
            b.set_cell(r, c, text.into()).unwrap();
        }
        b.add_sheet("Results").unwrap();
        let cases = [
            ("=SUM('Data''s sheet'!A:A)", "30"),
            ("=SUM('Data''s sheet'!$A:$B)", "30"),
            ("=COUNT('Data''s sheet'!A:B)", "2"),
            ("=COUNTA('Data''s sheet'!A:B)", "4"),
            ("=COUNTBLANK('Data''s sheet'!A:B)", "2097149"),
            ("=ROWS(A:A)", "1048576"),
            ("=COLUMNS(1:1)", "16384"),
            ("=SUM('Data''s sheet'!1:1)", "10"),
            ("=SUM('Data''s sheet'!$20:$20)", "20"),
            ("=SUM(INDIRECT(\"'Data''s sheet'!A:A\"))", "30"),
            ("=INDEX('Data''s sheet'!A:A,1048576)", "0"),
            ("=XFD1048576", "0"),
            ("=ISBLANK(XFD1048576)", "TRUE"),
            ("=OFFSET(A1048576,1,0)", "#REF!"),
            ("=XFE1", "#REF!"),
            ("=A1048577", "#REF!"),
            ("=COUNTIF('Data''s sheet'!A:A,\">10\")", "1"),
            ("=COUNTIF('Data''s sheet'!B:B,\"\")", "1048575"),
            (
                "=COUNTIFS('Data''s sheet'!A:A,\"\",'Data''s sheet'!B:B,\"\")",
                "1048573",
            ),
            (
                "=SUMIFS('Data''s sheet'!A:A,'Data''s sheet'!B:B,\"\")",
                "30",
            ),
            (
                "=AVERAGEIF('Data''s sheet'!B:B,\"\",'Data''s sheet'!A:A)",
                "15",
            ),
            (
                "=MAXIFS('Data''s sheet'!A:A,'Data''s sheet'!B:B,\"\")",
                "20",
            ),
            (
                "=MINIFS('Data''s sheet'!A:A,'Data''s sheet'!B:B,\"\")",
                "10",
            ),
            ("=COUNTIF('Data''s sheet'!1:1,\"\")", "16383"),
            ("=COUNTIF('Data''s sheet'!B:B,\"te?t\")", "1"),
        ];
        for (r, (f, _)) in cases.iter().enumerate() {
            b.set_cell(r, 0, (*f).into()).unwrap();
        }
        let values = b.calculated_cells();
        for (r, (f, want)) in cases.iter().enumerate() {
            assert_eq!(&values[&(r, 0)], want, "{f}");
        }
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
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
                                && n.attributes().any(|a| {
                                    matches!(a.name(), "PartName" | "full-path")
                                        && a.value().trim_start_matches('/') == "xls42/project.json"
                                })
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
            let independent = out.finish().unwrap().into_inner();
            let opened = Workbook::from_office_bytes(&independent, kind).unwrap();
            assert_eq!(opened.calculated_cells(), values);
        }
    }
}
