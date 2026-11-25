//! Sheet filter and page layout state; rendering belongs to the host.
use super::*;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SheetView {
    pub frozen_rows: usize,
    pub frozen_columns: usize,
    pub hidden_rows: std::collections::BTreeSet<usize>,
    pub hidden_columns: std::collections::BTreeSet<usize>,
}
pub(super) fn valid_view(view: &SheetView, sheet: &Sheet) -> bool {
    view.frozen_rows < sheet.rows
        && view.frozen_columns < sheet.cols
        && view.hidden_rows.len() < sheet.rows
        && view.hidden_columns.len() < sheet.cols
        && view.hidden_rows.iter().all(|row| *row < sheet.rows)
        && view.hidden_columns.iter().all(|col| *col < sheet.cols)
        && !sheet.merges.iter().any(|range| {
            view.hidden_rows
                .range(range.top..=range.bottom)
                .next()
                .is_some()
                || view
                    .hidden_columns
                    .range(range.left..=range.right)
                    .next()
                    .is_some()
                || (range.top < view.frozen_rows && range.bottom >= view.frozen_rows)
                || (range.left < view.frozen_columns && range.right >= view.frozen_columns)
        })
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetFilter {
    pub range: Range,
    pub column: usize,
    pub contains: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PrintSettings {
    pub area: Option<Range>,
    pub paper_width_twips: i32,
    pub paper_height_twips: i32,
    pub margin_left_twips: i32,
    pub margin_top_twips: i32,
    pub margin_right_twips: i32,
    pub margin_bottom_twips: i32,
    pub repeat_header_rows: usize,
    pub fit_width: bool,
    pub row_breaks: Vec<usize>,
}
impl Default for PrintSettings {
    fn default() -> Self {
        Self {
            area: None,
            paper_width_twips: 11906,
            paper_height_twips: 16838,
            margin_left_twips: 720,
            margin_top_twips: 720,
            margin_right_twips: 720,
            margin_bottom_twips: 720,
            repeat_header_rows: 0,
            fit_width: false,
            row_breaks: Vec::new(),
        }
    }
}
pub(super) fn valid_filter(f: &SheetFilter, s: &Sheet) -> bool {
    f.range.valid(s)
        && f.range.top < f.range.bottom
        && f.column >= f.range.left
        && f.column <= f.range.right
        && !f.contains.is_empty()
        && f.contains.len() <= 4096
        && !s.merges.iter().any(|m| m.overlaps(f.range))
}
pub(super) fn valid_print(p: &PrintSettings, s: &Sheet) -> bool {
    p.area.is_none_or(|r| r.valid(s))
        && p.paper_width_twips > 0
        && p.paper_height_twips > 0
        && p.paper_width_twips <= 100_000
        && p.paper_height_twips <= 100_000
        && [
            p.margin_left_twips,
            p.margin_top_twips,
            p.margin_right_twips,
            p.margin_bottom_twips,
        ]
        .iter()
        .all(|v| *v >= 0)
        && i64::from(p.margin_left_twips) + i64::from(p.margin_right_twips)
            < i64::from(p.paper_width_twips)
        && i64::from(p.margin_top_twips) + i64::from(p.margin_bottom_twips)
            < i64::from(p.paper_height_twips)
        && p.repeat_header_rows <= p.area.map_or(s.rows, |r| r.bottom - r.top + 1)
        && p.row_breaks.len() <= s.rows
        && p.row_breaks.iter().all(|r| *r > 0 && *r < s.rows)
        && p.row_breaks.windows(2).all(|v| v[0] < v[1])
}
impl Workbook {
    pub fn sheet_view(&self) -> &SheetView {
        &self.history.current().sheet().view
    }
    pub fn set_sheet_view(&mut self, view: SheetView) -> Result<bool, Error> {
        self.require_editable()?;
        let mut state = self.history.current().clone();
        if state.sheet().protected
            && (view.hidden_rows != state.sheet().view.hidden_rows
                || view.hidden_columns != state.sheet().view.hidden_columns)
        {
            return Err(Error::Protected);
        }
        if !valid_view(&view, state.sheet()) {
            return Err(Error::Invalid("Invalid hidden range or frozen pane"));
        }
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "View edits for this external format are not yet supported safely",
            ));
        }
        let selected = state.selection;
        if view.hidden_rows.contains(&selected.top) || view.hidden_columns.contains(&selected.left)
        {
            let rows = state.sheet().rows;
            let cols = state.sheet().cols;
            let row = (selected.top..rows)
                .chain((0..selected.top).rev())
                .find(|r| !view.hidden_rows.contains(r))
                .ok_or(Error::Bounds)?;
            let col = (selected.left..cols)
                .chain((0..selected.left).rev())
                .find(|c| !view.hidden_columns.contains(c))
                .ok_or(Error::Bounds)?;
            state.selection = Range {
                top: row,
                left: col,
                bottom: row,
                right: col,
            };
        }
        state.sheet_mut().view = view;
        if self.has_external_source() {
            self.validate_external_rename(&state)?;
        }
        Ok(self.history.apply(state))
    }
    pub fn sheet_filter(&self) -> Option<&SheetFilter> {
        self.history.current().sheet().filter.as_ref()
    }
    pub fn set_filter(&mut self, filter: Option<SheetFilter>) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let sheet = st.sheet_mut();
        if sheet.protected {
            return Err(Error::Protected);
        }
        if filter.as_ref().is_some_and(|v| {
            !valid_filter(v, sheet) || sheet.merges.iter().any(|m| m.overlaps(v.range))
        }) {
            return Err(Error::Invalid("invalid filter"));
        }
        sheet.filter = filter;
        Ok(self.history.apply(st))
    }
    pub fn visible_rows(&self) -> Vec<usize> {
        let grid = self.display_grid();
        self.visible_rows_from_grid(&grid)
    }
    pub fn visible_rows_from_grid(&self, grid: &[Vec<String>]) -> Vec<usize> {
        let s = self.history.current().sheet();
        let Some(f) = &s.filter else {
            return (0..s.rows)
                .filter(|r| !s.view.hidden_rows.contains(r))
                .collect();
        };
        let needle = f.contains.to_lowercase();
        (0..s.rows)
            .filter(|r| {
                !s.view.hidden_rows.contains(r)
                    && (*r <= f.range.top
                        || *r > f.range.bottom
                        || grid
                            .get(*r)
                            .and_then(|row| row.get(f.column))
                            .is_some_and(|value| value.to_lowercase().contains(&needle)))
            })
            .collect()
    }
    pub fn print_settings(&self) -> &PrintSettings {
        &self.history.current().sheet().print
    }
    pub fn set_print_settings(&mut self, settings: PrintSettings) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if !valid_print(&settings, st.sheet()) {
            return Err(Error::Invalid("invalid print settings"));
        }
        st.sheet_mut().print = settings;
        Ok(self.history.apply(st))
    }
    pub fn print_area(&self) -> Range {
        let s = self.history.current().sheet();
        if let Some(area) = s.print.area {
            return area;
        }
        let mut bottom = 0;
        let mut right = 0;
        for (&(r, c), cell) in &s.cells {
            if !cell.text.raw().is_empty() || cell.style != 0 {
                bottom = bottom.max(r);
                right = right.max(c)
            }
        }
        for m in &s.merges {
            bottom = bottom.max(m.bottom);
            right = right.max(m.right)
        }
        Range {
            top: 0,
            left: 0,
            bottom,
            right,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filter_and_page_settings_project_and_xlsx() {
        let mut b = Workbook::new(6, 2).unwrap();
        for (r, value) in ["Header", "Apple", "Pear", "Apricot", "Plum", "More"]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*value).into()).unwrap();
        }
        let filter = SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 4,
                right: 0,
            },
            column: 0,
            contains: "ap".into(),
        };
        b.set_filter(Some(filter.clone())).unwrap();
        assert_eq!(b.visible_rows(), [0, 1, 3, 5]);
        let mut p = b.print_settings().clone();
        p.area = Some(Range {
            top: 0,
            left: 0,
            bottom: 5,
            right: 1,
        });
        p.repeat_header_rows = 1;
        p.fit_width = true;
        p.row_breaks = vec![3];
        b.set_print_settings(p.clone()).unwrap();
        let bytes = b.xlsx_bytes().unwrap();
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(bytes.as_slice())).unwrap();
        let mut sheet = String::new();
        z.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut sheet)
            .unwrap();
        assert!(sheet.contains("autoFilter"));
        assert!(sheet.contains("rowBreaks"));
        let restored = Workbook::from_office_bytes(&bytes, "xlsx").unwrap();
        assert_eq!(restored.sheet_filter(), Some(&filter));
        assert_eq!(restored.print_settings(), &p);
        assert_eq!(restored.visible_rows(), [0, 1, 3, 5]);
    }
}

#[cfg(test)]
mod sheet_view_regression {
    use super::*;
    #[test]
    fn hidden_and_frozen_view_survives_history_and_formats() {
        let mut book = Workbook::new(8, 6).unwrap();
        book.set_cell(3, 2, "retained".into()).unwrap();
        let mut view = book.sheet_view().clone();
        view.frozen_rows = 2;
        view.frozen_columns = 1;
        view.hidden_rows.insert(3);
        view.hidden_columns.insert(2);
        book.set_sheet_view(view.clone()).unwrap();
        assert!(book.undo());
        assert_eq!(book.sheet_view(), &SheetView::default());
        assert!(book.redo());
        assert_eq!(book.sheet_view(), &view);
        let native = book.project_bytes().unwrap();
        assert_eq!(
            Workbook::from_project_bytes(&native).unwrap().sheet_view(),
            &view
        );
        let xlsx = book.xlsx_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(xlsx.as_slice())).unwrap();
        let mut sheet = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(),
            &mut sheet,
        )
        .unwrap();
        assert!(sheet.contains("state=\"frozen\""));
        assert!(sheet.contains("hidden=\"1\""));
        assert_eq!(
            Workbook::from_office_bytes(&xlsx, "xlsx")
                .unwrap()
                .sheet_view(),
            &view
        );
        let ods = book.ods_bytes().unwrap();
        let external = spreadsheet_ods::read_ods_buf(&ods).unwrap();
        assert_eq!(
            external.sheet(0).row_visible(3),
            spreadsheet_ods::sheet::Visibility::Collapsed
        );
        assert_eq!(
            external.sheet(0).col_visible(2),
            spreadsheet_ods::sheet::Visibility::Collapsed
        );
        assert!(matches!(
            external.sheet(0).config().vert_split_mode,
            spreadsheet_ods::sheet::SplitMode::Heading
        ));
        assert_eq!(external.sheet(0).config().vert_split_pos, 2);
        assert_eq!(
            Workbook::from_office_bytes(&ods, "ods")
                .unwrap()
                .sheet_view(),
            &view
        );
    }
}
