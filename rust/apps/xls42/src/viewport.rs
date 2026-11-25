//! Per-document visible blank space; never part of workbook history or file contents.
use super::*;
pub(super) fn union(a: Range, b: Range) -> Range {
    Range {
        top: a.top.min(b.top),
        left: a.left.min(b.left),
        bottom: a.bottom.max(b.bottom),
        right: a.right.max(b.right),
    }
}
#[derive(Default)]
pub(super) struct Viewport {
    pub generation: u64,
    bounds: std::collections::BTreeMap<u64, (usize, usize)>,
    pub selection: Option<(u64, Range)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    #[test]
    fn ordinary_range_commands_edit_virtual_destinations_with_complete_history() {
        for id in [26, 27, 75, 41, 44, 29, 58, 70, 71] {
            let mut app = app();
            app.book.set_cell(0, 0, "4".into()).unwrap();
            let range = Range {
                top: 100,
                left: 20,
                bottom: 102,
                right: 20,
            };
            if !matches!(id, 26 | 27 | 70) {
                app.choose_range(range, (100, 20)).unwrap();
            }
            let before = app.book.project_bytes().unwrap();
            let values = |v: &[&str]| v.iter().map(|v| (*v).to_owned()).collect::<Vec<_>>();
            match id {
                26 => app.input_action(id, &values(&["101", "21"])).unwrap(),
                27 => app
                    .input_action(id, &values(&["101", "21", "103", "21"]))
                    .unwrap(),
                75 => app
                    .input_action(id, &values(&["PCB-", "7", "2", "4"]))
                    .unwrap(),
                41 => app.input_action(id, &values(&["1", "10", "yes"])).unwrap(),
                44 => app
                    .input_action(id, &values(&["greater", "1", "FFFF00"]))
                    .unwrap(),
                70 => app.input_action(id, &values(&["100", "20"])).unwrap(),
                _ => app.action(id).unwrap(),
            }
            match id {
                26 => assert_eq!(app.book.cell(100, 20), Some("4")),
                27 => {
                    for row in 100..=102 {
                        assert_eq!(app.book.cell(row, 20), Some("4"));
                    }
                }
                75 => assert_eq!(
                    [
                        app.book.cell(100, 20),
                        app.book.cell(101, 20),
                        app.book.cell(102, 20)
                    ],
                    [Some("PCB-0007"), Some("PCB-0009"), Some("PCB-0011")]
                ),
                41 => assert_eq!(app.book.validations()[0].range, range),
                44 => assert_eq!(app.book.conditional_formats()[0].range, range),
                29 => assert_eq!(app.book.merges(), &[range]),
                58 => assert_eq!(app.book.print_settings().area, Some(range)),
                70 => {
                    assert_eq!(
                        (
                            app.book.sheet_view().frozen_rows,
                            app.book.sheet_view().frozen_columns
                        ),
                        (100, 20)
                    );
                    assert_eq!(app.book.selection().top, 0);
                }
                71 => assert_eq!(
                    app.book
                        .sheet_view()
                        .hidden_rows
                        .iter()
                        .copied()
                        .collect::<Vec<_>>(),
                    vec![100, 101, 102]
                ),
                _ => unreachable!(),
            }
            assert_eq!(app.book.cell(0, 0), Some("4"));
            let changed = app.book.project_bytes().unwrap();
            assert_ne!(changed, before);
            app.action(4).unwrap();
            assert_eq!(app.book.project_bytes().unwrap(), before, "Undo {id}");
            app.action(5).unwrap();
            assert_eq!(app.book.project_bytes().unwrap(), changed, "Redo {id}");
            let reopened = Workbook::from_project_bytes(&changed).unwrap();
            assert_eq!(reopened.project_bytes().unwrap(), changed);
        }
    }
    fn app() -> Xls {
        Xls {
            goal_seek: Default::default(),
            solver: Default::default(),
            book: Workbook::new(2, 2).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: Default::default(),
            picture_cache: Default::default(),
            viewport: Default::default(),
            selected_picture: None,
        }
    }
    #[test]
    fn view_identity_changes_on_open_new_and_sheet_switch_but_not_edits_or_rejected_open() {
        let mut app = app();
        let first = app.grid_view_identity();
        app.extend_grid_view(1, 1);
        assert_eq!(app.grid_view_identity(), first);
        app.edit("KEEP".into()).unwrap();
        assert_eq!(app.grid_view_identity(), first);
        app.book.add_sheet("Second").unwrap();
        assert_ne!(app.grid_view_identity(), first);
        app.book.undo();
        assert_eq!(app.grid_view_identity(), first);
        let path = std::env::temp_dir().join(format!("off42-grid-view-{}.csv", std::process::id()));
        std::fs::write(&path, "new source\n").unwrap();
        app.open(&path).unwrap();
        let opened = app.grid_view_identity();
        assert_ne!(opened, first);
        assert_eq!(app.view_bounds(), (1, 1));
        let original = app.book.project_bytes().unwrap();
        app.extend_grid_view(0, 0);
        let bounds = app.view_bounds();
        std::fs::write(&path, "bad\0text\n").unwrap();
        assert!(app.open(&path).is_err());
        assert_eq!(app.grid_view_identity(), opened);
        assert_eq!(app.view_bounds(), bounds);
        assert_eq!(app.book.project_bytes().unwrap(), original);
        app.new_document().unwrap();
        assert_ne!(app.grid_view_identity(), opened);
        assert_eq!(app.view_bounds(), (20, 8));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn technical_artwork_and_axis_geometry_can_extend_blank_space_without_extra_history_steps() {
        let mut app = app();
        let original = app.book.project_bytes().unwrap();
        let range = Range {
            top: 399,
            left: 30,
            bottom: 399,
            right: 30,
        };
        app.choose_range(range, (399, 30)).unwrap();
        let element = win42::TechnicalElement {
            symbol_id: None,
            code_type: win42::BarcodeFormat::Qr,
            value: "PCB-0042".into(),
            width_mm: 20.0,
            caption: false,
        };
        app.apply_technical_element(
            None,
            win42::TechnicalRaster::render(element.clone()).unwrap(),
        )
        .unwrap();
        let changed = app.book.project_bytes().unwrap();
        assert_eq!((app.book.rows(), app.book.cols()), (400, 31));
        let image = &app.book.images()[0];
        assert_eq!((image.row, image.column), (399, 30));
        assert_eq!(app.technical_targets()[0].element, element);
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), original);
        app.action(5).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        for (kind, bytes) in [
            ("xlsx", app.book.xlsx_bytes().unwrap()),
            ("ods", app.book.ods_bytes().unwrap()),
        ] {
            let opened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            let image = &opened.images()[0];
            assert_eq!((image.row, image.column), (399, 30));
            assert_eq!(
                win42::TechnicalElement::from_png(&image.data),
                Some(element.clone())
            );
        }
        let selection = app.book.selection();
        app.extend_grid_view(449, 30);
        app.resize_grid_column(40, 137).unwrap();
        assert_eq!(app.book.cols(), 41);
        assert_eq!(app.book.column_width(40), Some(137));
        assert_eq!(app.book.selection(), selection);
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        app.action(5).unwrap();
        let wide = app.book.project_bytes().unwrap();
        app.resize_grid_row(500, 37).unwrap();
        assert_eq!(app.book.rows(), 501);
        assert_eq!(app.book.row_height(500), Some(37));
        assert_eq!(app.book.selection(), selection);
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), wide);
        let id = app.book.images()[0].id;
        app.move_grid_picture(id, 800.0, 6000.0).unwrap();
        let moved = app.book.project_bytes().unwrap();
        let picture = &app.book.images()[0];
        assert_eq!(
            (
                picture.row,
                picture.column,
                picture.offset_x,
                picture.offset_y
            ),
            (699, 40, 0.0, 0.0)
        );
        assert_eq!((app.book.rows(), app.book.cols()), (700, 41));
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), wide);
        app.action(5).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), moved);
    }
    #[test]
    fn scrolling_and_selecting_virtual_blanks_do_not_change_document_or_history_and_edit_is_atomic()
    {
        let mut app = app();
        let original = app.book.project_bytes().unwrap();
        assert!(app.extend_grid_view(1, 1));
        let data = app.sparse_table().unwrap();
        assert_eq!((data.len(), data.columns()), (302, 22));
        assert_eq!(app.grid_row_label(301), "302");
        assert!(app.grid_cell_style(301, 21).is_some());
        app.input_action(
            25,
            &["1000".into(), "100".into(), "1000".into(), "100".into()],
        )
        .unwrap();
        assert_eq!(app.text(), "");
        assert_eq!(app.book.project_bytes().unwrap(), original);
        assert!(!app.dirty() && !app.can_undo());
        app.edit(String::new()).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), original);
        app.edit("=10+20".into()).unwrap();
        let changed = app.book.project_bytes().unwrap();
        assert_eq!((app.book.rows(), app.book.cols()), (1000, 100));
        assert_eq!(app.book.calculated_cells()[&(999, 99)], "30");
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), original);
        assert!(!app.dirty());
        app.action(5).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        assert_eq!(app.text(), "=10+20");
    }
    #[test]
    fn empty_cell_formatting_and_normal_clipboard_expand_only_written_bounds_with_one_undo() {
        let mut app = app();
        let original = app.book.project_bytes().unwrap();
        app.choose_range(
            Range {
                top: 399,
                left: 30,
                bottom: 399,
                right: 30,
            },
            (399, 30),
        )
        .unwrap();
        app.action(31).unwrap();
        assert_eq!((app.book.rows(), app.book.cols()), (400, 31));
        assert!(app.book.style(399, 30).unwrap().bold);
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), original);
        app.clipboard_text(40, "10\t20\n30\t=1+2".into()).unwrap();
        let changed = app.book.project_bytes().unwrap();
        assert_eq!((app.book.rows(), app.book.cols()), (401, 32));
        assert_eq!(app.book.cell(400, 31), Some("=1+2"));
        assert_eq!(app.book.calculated_cells()[&(400, 31)], "3");
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), original);
        app.action(5).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        let mut source = Workbook::new(1, 2).unwrap();
        source.set_cell(0, 0, "=A2+1".into()).unwrap();
        source.set_cell(0, 1, "typed".into()).unwrap();
        let (_, payload) = source.copy_clipboard_mapped(&[0], &[0, 1]).unwrap();
        app.choose_range(
            Range {
                top: 500,
                left: 40,
                bottom: 500,
                right: 40,
            },
            (500, 40),
        )
        .unwrap();
        app.clipboard_data(40, payload).unwrap();
        assert_eq!(app.book.cell(500, 40), Some("=AO502+1"));
        assert_eq!(app.book.cell(500, 41), Some("typed"));
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
    }
    #[test]
    fn protected_sheet_allows_blank_browsing_but_refuses_growth_without_losing_redo_or_source() {
        let mut app = app();
        app.book.set_protected(true);
        let original = app.book.project_bytes().unwrap();
        app.choose_range(
            Range {
                top: 100,
                left: 20,
                bottom: 100,
                right: 20,
            },
            (100, 20),
        )
        .unwrap();
        assert!(app.text_readonly());
        assert!(app.edit("bad".into()).is_err());
        assert!(app.clipboard_text(40, "bad".into()).is_err());
        assert!(app.action(31).is_err());
        assert_eq!(app.book.project_bytes().unwrap(), original);
        let mut view = Viewport::default();
        view.reveal(
            &app.book,
            xls42_core::MAX_ROWS - 1,
            xls42_core::MAX_COLS - 1,
        );
        assert_eq!(
            view.bounds(&app.book),
            (xls42_core::MAX_ROWS, xls42_core::MAX_COLS)
        );
        assert!(!view.extend(
            &app.book,
            xls42_core::MAX_ROWS - 1,
            xls42_core::MAX_COLS - 1
        ));
    }
}
impl Viewport {
    pub fn reset(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.bounds.clear();
        self.selection = None;
    }
    pub fn bounds(&self, book: &Workbook) -> (usize, usize) {
        let (rows, cols) = self
            .bounds
            .get(&book.active_sheet())
            .copied()
            .unwrap_or_default();
        (rows.max(book.rows()), cols.max(book.cols()))
    }
    pub fn reveal(&mut self, book: &Workbook, row: usize, col: usize) -> bool {
        let old = self.bounds(book);
        let next = (
            old.0.max(row.saturating_add(51)).min(xls42_core::MAX_ROWS),
            old.1.max(col.saturating_add(11)).min(xls42_core::MAX_COLS),
        );
        self.bounds.insert(book.active_sheet(), next);
        old != next
    }
    pub fn extend(&mut self, book: &Workbook, row: usize, col: usize) -> bool {
        let old = self.bounds(book);
        let next = (
            if row.saturating_add(50) >= old.0 {
                old.0.saturating_add(300).min(xls42_core::MAX_ROWS)
            } else {
                old.0
            },
            if col.saturating_add(10) >= old.1 {
                old.1.saturating_add(20).min(xls42_core::MAX_COLS)
            } else {
                old.1
            },
        );
        self.bounds.insert(book.active_sheet(), next);
        old != next
    }
}
impl Xls {
    pub(super) fn view_bounds(&self) -> (usize, usize) {
        self.viewport.borrow().bounds(&self.book)
    }
    pub(super) fn choose_range(
        &mut self,
        range: Range,
        focus: (usize, usize),
    ) -> Result<(), String> {
        if range.top > range.bottom
            || range.left > range.right
            || range.bottom >= xls42_core::MAX_ROWS
            || range.right >= xls42_core::MAX_COLS
            || focus.0 < range.top
            || focus.0 > range.bottom
            || focus.1 < range.left
            || focus.1 > range.right
        {
            return Err("Invalid range".into());
        }
        if range.bottom < self.book.rows() && range.right < self.book.cols() {
            self.book.set_selection(range).map_err(|e| e.to_string())?;
            self.viewport.borrow_mut().selection = None;
        } else {
            self.viewport.borrow_mut().selection = Some((self.book.active_sheet(), range));
        }
        self.viewport
            .borrow_mut()
            .reveal(&self.book, range.bottom, range.right);
        self.row = focus.0;
        self.col = focus.1;
        self.clear_visible_cache();
        Ok(())
    }
    pub(super) fn edit_range<T>(
        &mut self,
        range: Range,
        edit: impl FnOnce(&mut Workbook) -> Result<T, xls42_core::Error>,
    ) -> Result<T, String> {
        if range.bottom < self.book.rows() && range.right < self.book.cols() {
            edit(&mut self.book)
        } else {
            self.book.edit_expanding(range, edit)
        }
        .map_err(|e| e.to_string())
    }
}
