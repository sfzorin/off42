//! Visible blank grid space is not document data. Grow allocated bounds only with a successful edit.
use super::*;
impl Workbook {
    /// Run an edit in a larger addressable area, committing bounds, selection and data in one history step.
    /// Errors and no-op edits leave the original workbook, dirty state and redo history untouched.
    pub fn edit_expanding<T>(
        &mut self,
        range: Range,
        edit: impl FnOnce(&mut Workbook) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.require_editable()?;
        if range.top > range.bottom
            || range.left > range.right
            || range.bottom >= MAX_ROWS
            || range.right >= MAX_COLS
        {
            return Err(Error::Bounds);
        }
        if self.is_protected() {
            return Err(Error::Protected);
        }
        let mut state = self.history.current().clone();
        let sheet = state.sheet_mut();
        sheet.rows = sheet.rows.max(range.bottom + 1);
        sheet.cols = sheet.cols.max(range.right + 1);
        state.selection = range;
        let mut trial = Workbook {
            history: SnapshotHistory::new(state.clone(), 0),
            snapshots: Default::default(),
            next_revision: 1,
            external_source: self.external_source.clone(),
            external_baseline: self.external_baseline.clone(),
            external_format: self.external_format.clone(),
            external_cache: self.external_cache.clone(),
        };
        let result = edit(&mut trial)?;
        if trial.history.current() != &state {
            let next = trial.history.current().clone();
            self.validate_external_table_edit(&next)?;
            self.history.apply(next);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Write};
    #[test]
    fn growing_an_independent_office_sheet_preserves_source_and_opaque_parts_with_exact_undo() {
        let mut seed = Workbook::new(2, 2).unwrap();
        seed.set_cell(0, 0, "KEEP".into()).unwrap();
        for (kind, native) in [
            ("xlsx", seed.xlsx_bytes().unwrap()),
            ("ods", seed.ods_bytes().unwrap()),
        ] {
            let mut archive = zip::ZipArchive::new(Cursor::new(native)).unwrap();
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
                    let mut ranges = doc
                        .descendants()
                        .filter(|n| {
                            n.is_element()
                                && n.attributes().any(|a| {
                                    a.value().trim_start_matches('/') == "xls42/project.json"
                                })
                        })
                        .map(|n| n.range())
                        .collect::<Vec<_>>();
                    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
                    for r in ranges {
                        text.replace_range(r, "");
                    }
                    out.start_file(entry.name(), zip::write::SimpleFileOptions::default())
                        .unwrap();
                    out.write_all(text.as_bytes()).unwrap();
                } else {
                    out.raw_copy_file(entry).unwrap();
                }
            }
            out.start_file(
                "custom/growth.bin",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
            out.write_all(b"unchanged").unwrap();
            let original = out.finish().unwrap().into_inner();
            let mut b = Workbook::from_office_bytes(&original, kind).unwrap();
            let previous = b.history.current().clone();
            let range = Range {
                top: 499,
                left: 50,
                bottom: 499,
                right: 50,
            };
            b.edit_expanding(range, |b| b.set_cell(499, 50, "=10+20".into()))
                .unwrap();
            let changed = if kind == "xlsx" {
                b.xlsx_bytes()
            } else {
                b.ods_bytes()
            }
            .unwrap();
            let mut zip = zip::ZipArchive::new(Cursor::new(&changed)).unwrap();
            let mut data = Vec::new();
            zip.by_name("custom/growth.bin")
                .unwrap()
                .read_to_end(&mut data)
                .unwrap();
            assert_eq!(data, b"unchanged");
            assert!(zip.by_name("xls42/project.json").is_err());
            let reopened = Workbook::from_office_bytes(&changed, kind).unwrap();
            assert_eq!(reopened.cell(0, 0), Some("KEEP"));
            assert_eq!(reopened.cell(499, 50), Some("=10+20"));
            assert_eq!(reopened.calculated_cells()[&(499, 50)], "30");
            assert!(b.undo());
            assert!(b.history.current() == &previous);
            assert_eq!(
                if kind == "xlsx" {
                    b.xlsx_bytes()
                } else {
                    b.ods_bytes()
                }
                .unwrap(),
                original
            );
            assert!(b.redo());
            assert_eq!(b.cell(499, 50), Some("=10+20"));
            assert_eq!(b.external_source.as_deref(), Some(original.as_slice()));
        }
    }
    #[test]
    fn growth_commits_values_styles_selection_and_dimensions_as_one_edit_and_noops_do_nothing() {
        let mut b = Workbook::new(2, 2).unwrap();
        b.set_cell(0, 0, "KEEP".into()).unwrap();
        let original = b.project_bytes().unwrap();
        let range = Range {
            top: 99,
            left: 25,
            bottom: 100,
            right: 26,
        };
        assert!(
            !b.edit_expanding(range, |b| b.set_cell(99, 25, String::new()))
                .unwrap()
        );
        assert_eq!(b.project_bytes().unwrap(), original);
        b.edit_expanding(range, |b| {
            b.paste_tsv(99, 25, "10\t20\n30\t40")?;
            b.set_style(
                range,
                CellStyle {
                    bold: true,
                    ..Default::default()
                },
            )?;
            Ok(())
        })
        .unwrap();
        let changed = b.project_bytes().unwrap();
        assert_eq!((b.rows(), b.cols()), (101, 27));
        assert_eq!(b.selection(), range);
        assert_eq!(b.cell(100, 26), Some("40"));
        assert!(b.style(100, 26).unwrap().bold);
        assert!(b.undo());
        assert_eq!(b.project_bytes().unwrap(), original);
        let failed = b.edit_expanding(range, |b| {
            b.set_cell(99, 25, "partial".into())?;
            Err::<(), _>(Error::Invalid("reject complete operation"))
        });
        assert!(failed.is_err());
        assert_eq!(b.project_bytes().unwrap(), original);
        assert!(b.redo());
        assert_eq!(b.project_bytes().unwrap(), changed);
        for (kind, bytes) in [
            ("xlsx", b.xlsx_bytes().unwrap()),
            ("ods", b.ods_bytes().unwrap()),
        ] {
            let reopened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!((reopened.rows(), reopened.cols()), (101, 27));
            assert_eq!(reopened.cell(100, 26), Some("40"));
            assert!(reopened.style(100, 26).unwrap().bold);
        }
    }
    #[test]
    fn growth_respects_protection_limits_validation_and_the_last_global_coordinate() {
        let mut b = Workbook::new(2, 2).unwrap();
        let last = Range {
            top: MAX_ROWS - 1,
            left: MAX_COLS - 1,
            bottom: MAX_ROWS - 1,
            right: MAX_COLS - 1,
        };
        b.set_protected(true);
        let protected = b.project_bytes().unwrap();
        assert!(
            b.edit_expanding(last, |b| b.set_cell(last.top, last.left, "1".into()))
                .is_err()
        );
        assert_eq!(b.project_bytes().unwrap(), protected);
        b.set_protected(false);
        let original = b.project_bytes().unwrap();
        assert!(
            b.edit_expanding(
                Range {
                    bottom: MAX_ROWS,
                    ..last
                },
                |_| Ok(())
            )
            .is_err()
        );
        assert!(
            b.edit_expanding(last, |b| {
                b.set_cell(last.top, last.left, "1".into())?;
                b.add_validation(DataValidation {
                    range: last,
                    allow_blank: true,
                    kind: ValidationKind::Number {
                        minimum: 2.0,
                        maximum: 3.0,
                    },
                })?;
                b.set_cell(last.top, last.left, "4".into())
            })
            .is_err()
        );
        assert_eq!(b.project_bytes().unwrap(), original);
        b.edit_expanding(last, |b| b.set_cell(last.top, last.left, "=1+2".into()))
            .unwrap();
        assert_eq!(b.calculated_cells()[&(last.top, last.left)], "3");
        assert!(b.undo());
        assert_eq!(b.project_bytes().unwrap(), original);
    }
}
