#![cfg_attr(windows, windows_subsystem = "windows")]
mod charts;
mod print;
use std::path::Path;
use xls42_core::{
    Chart, ChartKind, ChartSeries, Condition, ConditionalFormat, DataValidation, Pivot,
    PivotAggregate, Range, SheetFilter, ValidationKind, Workbook,
};
struct Xls {
    book: Workbook,
    row: usize,
    col: usize,
    preview: Option<u64>,
    visible_cache: std::cell::RefCell<Vec<usize>>,
}
impl Xls {
    fn normalize_focus(&mut self) {
        let view = self.book.sheet_view();
        if view.hidden_rows.contains(&self.row) {
            self.row = (self.row..self.book.rows())
                .chain((0..self.row).rev())
                .find(|row| !view.hidden_rows.contains(row))
                .unwrap_or(self.row);
        }
        if view.hidden_columns.contains(&self.col) {
            self.col = (self.col..self.book.cols())
                .chain((0..self.col).rev())
                .find(|col| !view.hidden_columns.contains(col))
                .unwrap_or(self.col);
        }
    }
    fn compute_visible_rows(&self, grid: &[Vec<String>]) -> Vec<usize> {
        let mut rows = self.book.visible_rows_from_grid(grid);
        if !rows.contains(&self.row)
            && self.row < self.book.rows()
            && !self.book.sheet_view().hidden_rows.contains(&self.row)
        {
            rows.push(self.row);
            rows.sort_unstable()
        }
        rows
    }
    fn visible_rows(&self) -> Vec<usize> {
        if self.visible_cache.borrow().is_empty() {
            let rows = self.compute_visible_rows(&self.book.display_grid());
            self.visible_cache.replace(rows);
        }
        self.visible_cache.borrow().clone()
    }
    fn visible_at(&self, index: usize) -> Option<usize> {
        self.visible_rows().get(index).copied()
    }
    fn visible_position(&self, row: usize) -> Option<usize> {
        self.visible_rows().iter().position(|value| *value == row)
    }
    fn clear_visible_cache(&self) {
        self.visible_cache.borrow_mut().clear();
    }
    fn active_range(&self) -> Range {
        if self.book.sheet_filter().is_some() {
            return Range {
                top: self.row,
                left: self.col,
                bottom: self.row,
                right: self.col,
            };
        }
        self.selection_range()
    }
    fn selection_range(&self) -> Range {
        let r = self.book.selection();
        if self.row >= r.top && self.row <= r.bottom && self.col >= r.left && self.col <= r.right {
            r
        } else {
            Range {
                top: self.row,
                left: self.col,
                bottom: self.row,
                right: self.col,
            }
        }
    }
}
impl win42::App for Xls {
    fn supports_production(&self) -> bool {
        true
    }
    fn label_source(&self) -> Result<win42::LabelSource, String> {
        let range = self.selection_range();
        if range.right - range.left >= 200 || range.bottom - range.top >= 5000 {
            return Err("Select up to 5,000 rows and 200 columns.".into());
        }
        let columns = (range.left..=range.right)
            .filter(|column| !self.book.sheet_view().hidden_columns.contains(column))
            .collect::<Vec<_>>();
        if columns.is_empty() {
            return Err("Select visible cells to create labels.".into());
        }
        let grid = self.book.display_grid();
        let rows = self
            .book
            .visible_rows_from_grid(&grid)
            .into_iter()
            .filter(|row| *row >= range.top && *row <= range.bottom)
            .map(|row| {
                let values = columns.iter().map(|&col| grid[row][col].clone()).collect();
                (row + 1, values)
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return Err("Select visible cells to create labels.".into());
        }
        Ok(win42::LabelSource::Rows(rows))
    }
    fn save_formats(&self) -> Vec<win42::FileFormat> {
        vec![
            win42::FileFormat {
                id: "xls42",
                title: "XLS42 workbook",
                extensions: &["xls42"],
            },
            win42::FileFormat {
                id: "csv",
                title: "CSV",
                extensions: &["csv"],
            },
            win42::FileFormat {
                id: "xlsx",
                title: "Excel workbook",
                extensions: &["xlsx"],
            },
            win42::FileFormat {
                id: "ods",
                title: "OpenDocument spreadsheet",
                extensions: &["ods"],
            },
        ]
    }
    fn save_snapshot(&mut self, format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        self.book
            .capture_snapshot(format)
            .map(Some)
            .map_err(|e| e.to_string())
    }
    fn saved_snapshot(&mut self, revision: u64) {
        self.book.saved_snapshot(revision)
    }
    fn supports_new(&self) -> bool {
        true
    }
    fn new_document(&mut self) -> Result<(), String> {
        self.book = Workbook::new(20, 8).map_err(|e| e.to_string())?;
        self.row = 0;
        self.col = 0;
        self.preview = None;
        Ok(())
    }
    fn open_filter(&self) -> String {
        "*.xls42;*.csv;*.xlsx;*.xls;*.ods".into()
    }
    fn command_specs(&self) -> Vec<win42::CommandSpec> {
        [
            (20, "Add sheet"),
            (21, "Rename sheet"),
            (22, "Duplicate sheet"),
            (23, "Delete sheet"),
            (24, "Switch sheet"),
            (25, "Select range"),
            (26, "Copy range to"),
            (27, "Fill range"),
            (28, "Sort range"),
            (29, "Merge selection"),
            (30, "Unmerge selection"),
            (31, "Toggle bold"),
            (32, "Protect sheet"),
            (33, "Unprotect sheet"),
            (34, "Font..."),
            (35, "Text color..."),
            (36, "Fill color..."),
            (37, "Borders..."),
            (38, "Number format..."),
            (39, "Copy cells"),
            (40, "Paste cells"),
            (41, "Validate numeric range..."),
            (42, "Validate list..."),
            (43, "Remove validation"),
            (44, "Conditional formatting..."),
            (45, "Remove conditional formatting"),
            (50, "Insert chart..."),
            (51, "Preview chart..."),
            (52, "Edit chart..."),
            (53, "Delete chart..."),
            (54, "Return to grid"),
            (55, "Create pivot summary..."),
            (56, "Refresh pivot..."),
            (57, "Delete pivot..."),
            (58, "Set print area"),
            (59, "Print layout..."),
            (60, "Page setup..."),
            (61, "Clear print area"),
            (62, "Print workbook sheet..."),
            (63, "Filter selected range..."),
            (64, "Clear filter"),
            (65, "Set row height..."),
            (66, "Set column width..."),
            (67, "Move current sheet..."),
            (68, "Format selection as table..."),
            (69, "Remove table at selection"),
            (70, "Freeze panes..."),
            (71, "Hide selected rows"),
            (72, "Unhide selected rows"),
            (73, "Hide selected columns"),
            (74, "Unhide selected columns"),
            (75, "Fill Serial Numbers…"),
        ]
        .into_iter()
        .map(|(id, title)| win42::CommandSpec {
            id,
            title,
            accelerator: None,
            group: match id {
                0..=24 => "Sheets",
                75 => "Production",
                _ => "Cells",
            },
        })
        .collect()
    }
    fn clipboard_request(&self, id: u16) -> Option<win42::ClipboardRequest> {
        match id {
            39 => {
                let range = self.selection_range();
                let rows: Vec<_> = self
                    .visible_rows()
                    .into_iter()
                    .filter(|row| *row >= range.top && *row <= range.bottom)
                    .collect();
                let cols: Vec<_> = (range.left..=range.right)
                    .filter(|col| !self.book.sheet_view().hidden_columns.contains(col))
                    .collect();
                self.book
                    .copy_clipboard_mapped(&rows, &cols)
                    .ok()
                    .map(|(text, bytes)| win42::ClipboardRequest::WriteDataAndText {
                        format: "Suite42.Spreadsheet.Cells.v1".into(),
                        bytes,
                        text,
                    })
            }
            40 if !self.book.is_read_only() => Some(win42::ClipboardRequest::ReadDataOrText(
                "Suite42.Spreadsheet.Cells.v1".into(),
            )),
            _ => None,
        }
    }
    fn clipboard_data(&mut self, id: u16, bytes: Vec<u8>) -> Result<(), String> {
        if id != 40 {
            return Err("Unexpected clipboard action".into());
        }
        let rows: Vec<_> = self
            .visible_rows()
            .into_iter()
            .filter(|row| *row >= self.row)
            .collect();
        let cols: Vec<_> = (self.col..self.book.cols())
            .filter(|col| !self.book.sheet_view().hidden_columns.contains(col))
            .collect();
        self.book
            .paste_clipboard_mapped(&rows, &cols, &bytes)
            .map_err(|e| e.to_string())?;
        self.clear_visible_cache();
        Ok(())
    }
    fn clipboard_text(&mut self, id: u16, text: String) -> Result<(), String> {
        if id != 40 {
            return Err("Unexpected clipboard action".into());
        }
        let rows: Vec<_> = self
            .visible_rows()
            .into_iter()
            .filter(|row| *row >= self.row)
            .collect();
        let cols: Vec<_> = (self.col..self.book.cols())
            .filter(|col| !self.book.sheet_view().hidden_columns.contains(col))
            .collect();
        self.book
            .paste_tsv_mapped(&rows, &cols, &text)
            .map_err(|e| e.to_string())?;
        self.clear_visible_cache();
        Ok(())
    }
    fn dialog_request(&self, id: u16) -> Option<win42::DialogRequest> {
        if id == 60 {
            let p = self.book.print_settings();
            let mm = |v: i32| ((v as i64 * 2540 + 720) / 1440) as i32;
            return Some(win42::DialogRequest::PageSetup(win42::PageMargins {
                left: mm(p.margin_left_twips),
                top: mm(p.margin_top_twips),
                right: mm(p.margin_right_twips),
                bottom: mm(p.margin_bottom_twips),
                paper_width: mm(p.paper_width_twips),
                paper_height: mm(p.paper_height_twips),
            }));
        }
        match id {
            34 => Some(win42::DialogRequest::Font),
            35 | 36 => {
                let s = self.book.style(self.row, self.col)?;
                let color = if id == 35 { s.foreground } else { s.background }
                    .unwrap_or(if id == 35 { 0 } else { 0xffffff });
                Some(win42::DialogRequest::Color([
                    (color >> 16) as u8,
                    (color >> 8) as u8,
                    color as u8,
                ]))
            }
            _ => None,
        }
    }
    fn dialog_result(&mut self, id: u16, result: win42::DialogResult) -> Result<(), String> {
        if let (60, win42::DialogResult::PageSetup(p)) = (id, &result) {
            let twips = |v: i32| ((v as i64 * 1440 + 1270) / 2540) as i32;
            let mut s = self.book.print_settings().clone();
            s.paper_width_twips = twips(p.paper_width);
            s.paper_height_twips = twips(p.paper_height);
            s.margin_left_twips = twips(p.left);
            s.margin_top_twips = twips(p.top);
            s.margin_right_twips = twips(p.right);
            s.margin_bottom_twips = twips(p.bottom);
            self.book.set_print_settings(s).map_err(|e| e.to_string())?;
            return Ok(());
        }
        let mut style = self
            .book
            .style(self.row, self.col)
            .ok_or("Invalid selection")?
            .clone();
        match (id, result) {
            (34, win42::DialogResult::Font(f)) => {
                style.font = f.family;
                style.font_size = f.points.round().clamp(1.0, u16::MAX as f64) as u16;
                style.bold = f.bold;
                style.italic = f.italic;
                style.underline = f.underline;
                style.foreground =
                    Some((f.color[0] as u32) << 16 | (f.color[1] as u32) << 8 | f.color[2] as u32)
            }
            (35, win42::DialogResult::Color(c)) => {
                style.foreground = Some((c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
            }
            (36, win42::DialogResult::Color(c)) => {
                style.background = Some((c[0] as u32) << 16 | (c[1] as u32) << 8 | c[2] as u32)
            }
            _ => return Err("Unexpected dialog result".into()),
        }
        self.book
            .set_style(self.active_range(), style)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn input_request(&self, id: u16) -> Option<win42::InputRequest> {
        let (title, fields) = match id {
            20 | 21 | 22 => ("Sheet name", vec![("Name", String::new())]),
            75 => (
                "Fill Serial Numbers",
                vec![
                    ("Prefix (optional)", String::new()),
                    ("Start", "1".into()),
                    ("Step", "1".into()),
                    ("Minimum number of digits", "4".into()),
                ],
            ),
            70 => (
                "Freeze panes",
                vec![
                    ("Rows", self.book.sheet_view().frozen_rows.to_string()),
                    ("Columns", self.book.sheet_view().frozen_columns.to_string()),
                ],
            ),
            68 => (
                "Create table from selection",
                vec![
                    ("Table name", "Table1".into()),
                    ("Total row (yes/no)", "no".into()),
                ],
            ),
            24 => ("Switch sheet", vec![("Sheet name", String::new())]),
            25 | 27 => (
                "Range (1-based)",
                vec![
                    ("Top", (self.row + 1).to_string()),
                    ("Left", (self.col + 1).to_string()),
                    ("Bottom", (self.row + 1).to_string()),
                    ("Right", (self.col + 1).to_string()),
                ],
            ),
            26 => (
                "Copy selection to",
                vec![
                    ("Top", (self.row + 1).to_string()),
                    ("Left", (self.col + 1).to_string()),
                ],
            ),
            28 => (
                "Sort selection",
                vec![
                    ("Key column (1-based)", (self.col + 1).to_string()),
                    ("Descending (yes/no)", "no".into()),
                ],
            ),
            37 => (
                "Borders",
                vec![(
                    "Sides (0–15: top 1, right 2, bottom 4, left 8)",
                    "15".into(),
                )],
            ),
            38 => ("Number format", vec![("Excel format code", String::new())]),
            41 => (
                "Numeric validation",
                vec![
                    ("Minimum", "0".into()),
                    ("Maximum", "100".into()),
                    ("Allow blank (yes/no)", "yes".into()),
                ],
            ),
            42 => (
                "List validation",
                vec![
                    ("Allowed values separated by semicolons", String::new()),
                    ("Allow blank (yes/no)", "yes".into()),
                ],
            ),
            67 => ("Move sheet", vec![("New position (1-based)", "1".into())]),
            65 => (
                "Row height",
                vec![
                    ("Row (1-based)", (self.row + 1).to_string()),
                    ("Height in pixels", "20".into()),
                ],
            ),
            66 => (
                "Column width",
                vec![
                    ("Column (1-based)", (self.col + 1).to_string()),
                    ("Width in pixels", "80".into()),
                ],
            ),
            59 => (
                "Print layout",
                vec![
                    (
                        "Repeated header rows",
                        self.book.print_settings().repeat_header_rows.to_string(),
                    ),
                    (
                        "Fit columns to one page wide (yes/no)",
                        if self.book.print_settings().fit_width {
                            "yes".into()
                        } else {
                            "no".into()
                        },
                    ),
                    (
                        "Break before rows (1-based, comma-separated)",
                        self.book
                            .print_settings()
                            .row_breaks
                            .iter()
                            .map(|r| (r + 1).to_string())
                            .collect::<Vec<_>>()
                            .join(","),
                    ),
                ],
            ),
            63 => (
                "Filter selection (first row is header)",
                vec![
                    ("Column (1-based)", (self.col + 1).to_string()),
                    ("Contains text", String::new()),
                ],
            ),
            50 => (
                "Insert chart",
                vec![
                    ("Type (bar, line, pie, scatter)", "bar".into()),
                    ("Title", "Chart".into()),
                ],
            ),
            51 | 53 => (
                "Chart",
                vec![(
                    "Chart ID",
                    self.book
                        .charts()
                        .first()
                        .map(|c| c.id.to_string())
                        .unwrap_or_default(),
                )],
            ),
            52 => (
                "Edit chart",
                vec![
                    (
                        "Chart ID",
                        self.book
                            .charts()
                            .first()
                            .map(|c| c.id.to_string())
                            .unwrap_or_default(),
                    ),
                    ("Type (bar, line, pie, scatter)", "bar".into()),
                    ("Title", "Chart".into()),
                ],
            ),
            55 => (
                "Create pivot (selection is source with header)",
                vec![
                    ("Name", "Summary".into()),
                    (
                        "Group column (1-based)",
                        (self.active_range().left + 1).to_string(),
                    ),
                    (
                        "Value column (1-based)",
                        (self.active_range().right + 1).to_string(),
                    ),
                    ("Aggregate (sum, count, average, min, max)", "sum".into()),
                    (
                        "Destination row (1-based)",
                        (self.active_range().bottom + 2).to_string(),
                    ),
                    (
                        "Destination column (1-based)",
                        (self.active_range().left + 1).to_string(),
                    ),
                ],
            ),
            56 | 57 => (
                "Pivot",
                vec![(
                    "Pivot ID",
                    self.book
                        .pivots()
                        .first()
                        .map(|p| p.id.to_string())
                        .unwrap_or_default(),
                )],
            ),
            44 => (
                "Conditional formatting",
                vec![
                    ("Rule (greater, less, equal, contains)", "greater".into()),
                    ("Threshold or text", "0".into()),
                    ("Background RGB hex (e.g. FFFF00)", "FFFF00".into()),
                ],
            ),
            _ => return None,
        };
        Some(win42::InputRequest { title, fields })
    }
    fn input_action(&mut self, id: u16, v: &[String]) -> Result<(), String> {
        let n = |i: usize| -> Result<usize, String> {
            v.get(i)
                .and_then(|x| x.parse::<usize>().ok())
                .and_then(|x| x.checked_sub(1))
                .ok_or_else(|| "Enter a positive integer".into())
        };
        match id {
            75 => {
                let range = self.selection_range();
                if range.left != range.right
                    || self.book.sheet_view().hidden_columns.contains(&range.left)
                {
                    return Err("Select a single visible column or part of it.".into());
                }
                let [prefix, start, step, digits] = v else {
                    return Err("Invalid serial settings.".into());
                };
                let rows = self
                    .book
                    .visible_rows()
                    .into_iter()
                    .filter(|row| *row >= range.top && *row <= range.bottom)
                    .collect::<Vec<_>>();
                self.book
                    .fill_serials(
                        &rows,
                        range.left,
                        prefix,
                        start
                            .trim()
                            .parse()
                            .map_err(|_| "Enter a whole starting number.")?,
                        step.trim()
                            .parse()
                            .map_err(|_| "Enter a positive whole step.")?,
                        digits
                            .trim()
                            .parse()
                            .map_err(|_| "Enter the minimum number of digits.")?,
                    )
                    .map_err(|e| e.to_string())?;
                self.clear_visible_cache();
            }
            20 => {
                self.book.add_sheet(&v[0]).map_err(|e| e.to_string())?;
            }
            21 => {
                self.book
                    .rename_sheet(self.book.active_sheet(), &v[0])
                    .map_err(|e| e.to_string())?;
            }
            22 => {
                self.book
                    .duplicate_sheet(self.book.active_sheet(), &v[0])
                    .map_err(|e| e.to_string())?;
            }
            24 => {
                let sheet = self
                    .book
                    .sheets()
                    .into_iter()
                    .find(|(_, name)| name.eq_ignore_ascii_case(&v[0]))
                    .ok_or("Sheet not found")?
                    .0;
                self.book.select_sheet(sheet).map_err(|e| e.to_string())?;
            }
            25 | 27 => {
                let range = Range {
                    top: n(0)?,
                    left: n(1)?,
                    bottom: n(2)?,
                    right: n(3)?,
                };
                if id == 25 {
                    self.book.set_selection(range)
                } else {
                    self.book.fill_range(self.active_range(), range)
                }
                .map_err(|e| e.to_string())?;
                self.row = range.top;
                self.col = range.left;
            }
            26 => {
                self.book
                    .copy_range(self.active_range(), n(0)?, n(1)?)
                    .map_err(|e| e.to_string())?;
            }
            28 => {
                let mut range = self.active_range();
                if let Some(table) = self.book.tables().iter().find(|table| table.range == range) {
                    range.top += 1;
                    if table.total_row {
                        range.bottom -= 1;
                    }
                }
                self.book
                    .sort_range(range, n(0)?, v[1].eq_ignore_ascii_case("yes"))
                    .map_err(|e| e.to_string())?;
            }
            70 => {
                let mut view = self.book.sheet_view().clone();
                view.frozen_rows = v[0].parse().map_err(|_| "Invalid frozen row count")?;
                view.frozen_columns = v[1].parse().map_err(|_| "Invalid frozen column count")?;
                self.book.set_sheet_view(view).map_err(|e| e.to_string())?;
            }
            68 => {
                self.book
                    .add_table(&v[0], self.active_range(), v[1].eq_ignore_ascii_case("yes"))
                    .map_err(|e| e.to_string())?;
            }
            67 => {
                self.book
                    .reorder_sheet(self.book.active_sheet(), n(0)?)
                    .map_err(|e| e.to_string())?;
            }
            65 => {
                let size = v[1].parse::<u16>().map_err(|_| "Invalid height")?;
                self.book
                    .set_row_height(n(0)?, size)
                    .map_err(|e| e.to_string())?;
            }
            66 => {
                let size = v[1].parse::<u16>().map_err(|_| "Invalid width")?;
                self.book
                    .set_column_width(n(0)?, size)
                    .map_err(|e| e.to_string())?;
            }
            59 => {
                let mut settings = self.book.print_settings().clone();
                settings.repeat_header_rows =
                    v[0].parse().map_err(|_| "Invalid header row count")?;
                settings.fit_width = v[1].eq_ignore_ascii_case("yes");
                settings.row_breaks = if v[2].trim().is_empty() {
                    Vec::new()
                } else {
                    v[2].split(',')
                        .map(|s| {
                            s.trim()
                                .parse::<usize>()
                                .ok()
                                .and_then(|r| r.checked_sub(1))
                                .ok_or("Invalid page-break row")
                        })
                        .collect::<Result<Vec<_>, _>>()?
                };
                settings.row_breaks.sort_unstable();
                settings.row_breaks.dedup();
                self.book
                    .set_print_settings(settings)
                    .map_err(|e| e.to_string())?;
            }
            63 => {
                let range = self.active_range();
                self.book
                    .set_filter(Some(SheetFilter {
                        range,
                        column: n(0)?,
                        contains: v[1].clone(),
                    }))
                    .map_err(|e| e.to_string())?;
                self.clear_visible_cache();
            }
            50 => {
                let r = self.active_range();
                if r.top == r.bottom {
                    return Err("Select a header row and at least one data row".into());
                }
                let kind = parse_chart_kind(&v[0])?;
                if kind == ChartKind::Scatter && r.left == r.right {
                    return Err("Scatter charts require an X column and a Y column".into());
                }
                let first = if r.left < r.right { r.left + 1 } else { r.left };
                let last = if kind == ChartKind::Pie {
                    first
                } else {
                    r.right
                };
                let mut series = Vec::new();
                for col in first..=last {
                    let categories = (r.left < r.right).then_some(Range {
                        top: r.top + 1,
                        left: r.left,
                        bottom: r.bottom,
                        right: r.left,
                    });
                    series.push(ChartSeries {
                        name: self.book.cell(r.top, col).unwrap_or("").to_owned(),
                        sheet: self.book.active_sheet(),
                        values: Range {
                            top: r.top + 1,
                            left: col,
                            bottom: r.bottom,
                            right: col,
                        },
                        categories,
                    })
                }
                let id = self
                    .book
                    .add_chart(Chart {
                        id: 0,
                        title: v[1].clone(),
                        kind,
                        series,
                        anchor_row: self.row,
                        anchor_col: self.col,
                    })
                    .map_err(|e| e.to_string())?;
                self.preview = Some(id);
            }
            51 => {
                let id = v[0].parse::<u64>().map_err(|_| "Invalid chart ID")?;
                self.book.chart_data(id).map_err(|e| e.to_string())?;
                self.preview = Some(id);
            }
            52 => {
                let id = v[0].parse::<u64>().map_err(|_| "Invalid chart ID")?;
                let mut chart = self
                    .book
                    .charts()
                    .iter()
                    .find(|c| c.id == id)
                    .cloned()
                    .ok_or("Chart not found")?;
                chart.kind = parse_chart_kind(&v[1])?;
                chart.title = v[2].clone();
                self.book.update_chart(chart).map_err(|e| e.to_string())?;
                self.preview = Some(id);
            }
            53 => {
                let id = v[0].parse::<u64>().map_err(|_| "Invalid chart ID")?;
                self.book.delete_chart(id).map_err(|e| e.to_string())?;
                if self.preview == Some(id) {
                    self.preview = None
                }
            }
            55 => {
                let source = self.active_range();
                let aggregate = match v[3].to_lowercase().as_str() {
                    "sum" => PivotAggregate::Sum,
                    "count" => PivotAggregate::Count,
                    "average" => PivotAggregate::Average,
                    "min" => PivotAggregate::Min,
                    "max" => PivotAggregate::Max,
                    _ => return Err("Choose sum, count, average, min or max".into()),
                };
                let pivot = Pivot {
                    id: 0,
                    name: v[0].clone(),
                    source_sheet: self.book.active_sheet(),
                    source,
                    group_column: n(1)?,
                    value_column: n(2)?,
                    aggregate,
                    destination_sheet: self.book.active_sheet(),
                    top: n(4)?,
                    left: n(5)?,
                    output: Vec::new(),
                };
                self.book.add_pivot(pivot).map_err(|e| e.to_string())?;
            }
            56 | 57 => {
                let id = v[0].parse::<u64>().map_err(|_| "Invalid pivot ID")?;
                if id == 56 {
                    self.book.refresh_pivot(id)
                } else {
                    self.book.delete_pivot(id)
                }
                .map_err(|e| e.to_string())?;
            }
            41 => {
                let minimum = v[0].parse::<f64>().map_err(|_| "Invalid minimum")?;
                let maximum = v[1].parse::<f64>().map_err(|_| "Invalid maximum")?;
                let rule = DataValidation {
                    range: self.active_range(),
                    allow_blank: v[2].eq_ignore_ascii_case("yes"),
                    kind: ValidationKind::Number { minimum, maximum },
                };
                self.book.add_validation(rule).map_err(|e| e.to_string())?;
            }
            42 => {
                let values = v[0].split(';').map(str::to_owned).collect();
                let rule = DataValidation {
                    range: self.active_range(),
                    allow_blank: v[1].eq_ignore_ascii_case("yes"),
                    kind: ValidationKind::List { values },
                };
                self.book.add_validation(rule).map_err(|e| e.to_string())?;
            }
            44 => {
                let condition = match v[0].to_lowercase().as_str() {
                    "greater" => {
                        Condition::GreaterThan(v[1].parse().map_err(|_| "Invalid threshold")?)
                    }
                    "less" => Condition::LessThan(v[1].parse().map_err(|_| "Invalid threshold")?),
                    "equal" => Condition::EqualTo(v[1].parse().map_err(|_| "Invalid threshold")?),
                    "contains" => Condition::TextContains(v[1].clone()),
                    _ => return Err("Choose greater, less, equal or contains".into()),
                };
                let hex = v[2].trim().trim_start_matches('#');
                if hex.len() != 6 {
                    return Err("Enter six hexadecimal RGB digits".into());
                }
                let background = u32::from_str_radix(hex, 16).map_err(|_| "Invalid RGB color")?;
                self.book
                    .add_conditional_format(ConditionalFormat {
                        range: self.active_range(),
                        condition,
                        background: Some(background),
                        foreground: None,
                        bold: false,
                    })
                    .map_err(|e| e.to_string())?;
            }
            37 | 38 => {
                let mut style = self
                    .book
                    .style(self.row, self.col)
                    .ok_or("Invalid selection")?
                    .clone();
                if id == 37 {
                    style.borders = v[0]
                        .parse::<u8>()
                        .ok()
                        .filter(|n| *n < 16)
                        .ok_or("Enter a value from 0 to 15")?
                } else {
                    style.number_format = v[0].clone()
                }
                self.book
                    .set_style(self.active_range(), style)
                    .map_err(|e| e.to_string())?;
            }
            _ => return Err("Unsupported command".into()),
        }
        self.row = self.row.min(self.book.rows() - 1);
        self.col = self.col.min(self.book.cols() - 1);
        self.normalize_focus();
        self.clear_visible_cache();
        Ok(())
    }
    fn name(&self) -> &'static str {
        "XLS42"
    }
    fn extension(&self) -> &'static str {
        "csv"
    }
    fn format_name(&self) -> &'static str {
        "CSV"
    }
    fn actions(&self) -> &'static [(u16, &'static str)] {
        &[(10, "Add &Row"), (11, "Add &Column")]
    }
    fn items(&self) -> Vec<String> {
        vec![]
    }
    fn uses_custom_grid(&self) -> bool {
        true
    }
    fn grid_frozen_panes(&self) -> (usize, usize) {
        let rows = self.book.sheet_view().frozen_rows;
        let visible = self
            .visible_rows()
            .into_iter()
            .filter(|row| *row < rows)
            .count();
        (visible, self.book.sheet_view().frozen_columns)
    }
    fn grid_column_width(&self, column: usize) -> u16 {
        if self.book.sheet_view().hidden_columns.contains(&column) {
            0
        } else {
            self.book.column_width(column).unwrap_or(80)
        }
    }
    fn grid_row_height(&self, row: usize) -> u16 {
        self.visible_at(row)
            .map(|r| {
                if self.book.sheet_view().hidden_rows.contains(&r) {
                    0
                } else {
                    self.book.row_height(r).unwrap_or(20)
                }
            })
            .unwrap_or(20)
    }
    fn resize_grid_column(&mut self, column: usize, pixels: u16) -> Result<(), String> {
        self.book
            .set_column_width(column, pixels)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn resize_grid_row(&mut self, row: usize, pixels: u16) -> Result<(), String> {
        let actual = self.visible_at(row).ok_or("Invalid row")?;
        self.book
            .set_row_height(actual, pixels)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    fn grid_row_label(&self, row: usize) -> String {
        self.visible_at(row)
            .map(|r| (r + 1).to_string())
            .unwrap_or_default()
    }
    fn grid_clipboard_commands(&self) -> Option<(u16, u16)> {
        Some((39, 40))
    }
    fn grid_sheets(&self) -> Option<(Vec<String>, usize)> {
        let sheets = self.book.sheets();
        let selected = sheets
            .iter()
            .position(|(id, _)| *id == self.book.active_sheet())?;
        Some((sheets.into_iter().map(|(_, name)| name).collect(), selected))
    }
    fn select_grid_sheet(&mut self, index: usize) {
        if let Some((id, _)) = self.book.sheets().get(index) {
            let _ = self.book.select_sheet(*id);
            self.preview = None;
            self.clear_visible_cache();
            self.row = 0;
            self.col = 0;
            self.normalize_focus();
        }
    }
    fn grid_selection(&self) -> Option<win42::GridRange> {
        let r = self.selection_range();
        let rows = self.visible_rows();
        let top = rows
            .iter()
            .position(|row| *row >= r.top && *row <= r.bottom)?;
        let bottom = rows
            .iter()
            .rposition(|row| *row >= r.top && *row <= r.bottom)?;
        Some(win42::GridRange {
            top,
            left: r.left,
            bottom,
            right: r.right,
        })
    }
    fn select_grid_range(&mut self, r: win42::GridRange) {
        let rows = self.visible_rows();
        let Some(&top) = rows.get(r.top) else { return };
        let Some(&bottom) = rows.get(r.bottom) else {
            return;
        };
        let range = Range {
            top,
            left: r.left,
            bottom,
            right: r.right,
        };
        if self.book.set_selection(range).is_ok() {
            self.row = range.top;
            self.col = range.left
        }
    }
    fn select_grid_range_with_focus(&mut self, r: win42::GridRange, focus: (usize, usize)) {
        let Some(top) = self.visible_at(r.top) else {
            return;
        };
        let Some(bottom) = self.visible_at(r.bottom) else {
            return;
        };
        let Some(active) = self.visible_at(focus.0) else {
            return;
        };
        let range = Range {
            top,
            left: r.left,
            bottom,
            right: r.right,
        };
        if self.book.set_selection(range).is_ok() {
            self.row = active;
            self.col = focus.1;
        }
    }
    fn grid_cell_style(&self, row: usize, column: usize) -> Option<win42::GridCellStyle> {
        let row = self.visible_at(row)?;
        let s = self.book.effective_style(row, column)?;
        let rgb = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
        let merged = self
            .book
            .merges()
            .iter()
            .find(|m| row >= m.top && row <= m.bottom && column >= m.left && column <= m.right)
            .filter(|_| self.book.sheet_filter().is_none())
            .and_then(|m| {
                Some(win42::GridRange {
                    top: self.visible_position(m.top)?,
                    left: m.left,
                    bottom: self.visible_position(m.bottom)?,
                    right: m.right,
                })
            });
        Some(win42::GridCellStyle {
            foreground: s.foreground.map(rgb),
            background: s.background.map(rgb),
            bold: s.bold,
            italic: s.italic,
            borders: s.borders,
            merged,
        })
    }
    fn table(&self) -> Option<Vec<Vec<String>>> {
        if self.preview.is_some() {
            None
        } else {
            let grid = self.book.display_grid();
            let rows = self.compute_visible_rows(&grid);
            self.visible_cache.replace(rows.clone());
            Some(
                rows.into_iter()
                    .filter_map(|r| grid.get(r).cloned())
                    .collect(),
            )
        }
    }
    fn print_job(&self, id: u16) -> Option<win42::PrintJob> {
        if id != 62 {
            return None;
        }
        let mut pages = match print::pages(&self.book, &self.book.visible_rows()) {
            Ok(p) => p,
            Err(_) => return Some(win42::PrintJob::Scenes(Vec::new())),
        };
        let p = self.book.print_settings();
        let geometry = win42::PrintGeometry {
            width_twips: p.paper_width_twips,
            height_twips: p.paper_height_twips,
            margin_left_twips: p.margin_left_twips,
            margin_top_twips: p.margin_top_twips,
            margin_right_twips: p.margin_right_twips,
            margin_bottom_twips: p.margin_bottom_twips,
        };
        let width =
            (p.paper_width_twips - p.margin_left_twips - p.margin_right_twips) as f64 / 20.0;
        let height =
            (p.paper_height_twips - p.margin_top_twips - p.margin_bottom_twips) as f64 / 20.0;
        for chart in self.book.charts() {
            if let Ok(data) = self.book.chart_data(chart.id) {
                pages.push(win42::PrintPage {
                    geometry,
                    scene: charts::print_scene(&data, width, height),
                })
            }
        }
        Some(win42::PrintJob::Scenes(pages))
    }
    fn canvas_scene(&self) -> Option<win42::CanvasScene> {
        self.preview
            .and_then(|id| self.book.chart_data(id).ok())
            .map(|data| charts::scene(&data))
    }
    fn select_cell(&mut self, row: usize, col: usize) {
        let actual = self.visible_at(row).unwrap_or(self.row);
        if actual != self.row {
            self.row = actual;
            self.clear_visible_cache();
        }
        self.col = col.min(self.book.cols() - 1);
    }
    fn selected_cell(&self) -> Option<(usize, usize)> {
        Some((self.visible_position(self.row)?, self.col))
    }
    fn text(&self) -> String {
        self.book
            .cell(self.row, self.col)
            .map(win42::to_native_text)
            .unwrap_or_default()
    }
    fn status(&self) -> String {
        let sheet = self
            .book
            .sheets()
            .into_iter()
            .find(|(id, _)| *id == self.book.active_sheet())
            .map(|(_, name)| name)
            .unwrap_or_default();
        let charts = self
            .book
            .charts()
            .iter()
            .map(|c| format!("{}:{}", c.id, c.title))
            .collect::<Vec<_>>()
            .join(", ");
        let pivots = self
            .book
            .pivots()
            .iter()
            .map(|p| format!("{}:{}", p.id, p.name))
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "{}{} | {}×{} | R{} C{} | Charts [{}] | Pivot summaries [{}]{}",
            if self.book.is_read_only() {
                "READ ONLY | "
            } else if self.book.has_external_source() {
                "External: structural edits limited | "
            } else {
                ""
            },
            sheet,
            self.book.rows(),
            self.book.cols(),
            self.row + 1,
            self.col + 1,
            charts,
            pivots,
            if self.preview.is_some() {
                " | Chart preview"
            } else {
                ""
            }
        )
    }
    fn selected(&self) -> usize {
        self.row * self.book.cols() + self.col
    }
    fn text_readonly(&self) -> bool {
        self.preview.is_some() || self.book.is_read_only() || self.book.is_protected()
    }
    fn can_save(&self) -> bool {
        !self.book.is_read_only()
    }
    fn dirty(&self) -> bool {
        self.book.is_dirty()
    }
    fn can_undo(&self) -> bool {
        self.book.can_undo()
    }
    fn can_redo(&self) -> bool {
        self.book.can_redo()
    }
    fn open(&mut self, p: &Path) -> Result<(), String> {
        let book = match p
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("xls42") => Workbook::open_project(p),
            Some("csv") => Workbook::open_csv(p),
            Some("xlsx") => Workbook::open_office(p, "xlsx"),
            Some("xls") => Workbook::open_office(p, "xls"),
            Some("ods") => Workbook::open_office(p, "ods"),
            _ => return Err("Unsupported workbook format".into()),
        }
        .map_err(|e| e.to_string())?;
        for r in 0..book.rows() {
            for c in 0..book.cols() {
                win42::validate_editable(book.cell(r, c).unwrap_or(""))?;
            }
        }
        self.book = book;
        self.preview = None;
        self.clear_visible_cache();
        self.row = 0;
        self.col = 0;
        self.normalize_focus();
        Ok(())
    }
    fn save(&mut self, p: &Path) -> Result<(), String> {
        match p
            .extension()
            .and_then(|x| x.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("xls42") => self.book.save_project(p),
            Some("csv") => self.book.save_as_csv(p),
            Some("xlsx") => self.book.save_office(p, "xlsx"),
            Some("ods") => self.book.save_office(p, "ods"),
            _ => return Err("Unsupported workbook format".into()),
        }
        .map_err(|e| e.to_string())
    }
    fn edit(&mut self, s: String) -> Result<(), String> {
        self.book
            .set_cell_editor_text(self.row, self.col, win42::from_native_text(&s))
            .map_err(|e| e.to_string())?;
        self.clear_visible_cache();
        Ok(())
    }
    fn select(&mut self, i: usize) {
        self.select_cell(i / self.book.cols(), i % self.book.cols())
    }
    fn enabled(&self, id: u16) -> bool {
        match id {
            10 => self.book.can_add_row(),
            11 => self.book.can_add_column(),
            75 => !self.book.is_read_only() && !self.book.is_protected(),
            _ => true,
        }
    }
    fn action(&mut self, id: u16) -> Result<(), String> {
        if id >= 10 && !self.enabled(id) {
            return Err("Command unavailable".into());
        }
        match id {
            4 => {
                self.book.undo();
            }
            5 => {
                self.book.redo();
            }
            10 => {
                self.book.add_row().map_err(|e| e.to_string())?;
            }
            11 => {
                self.book.add_column().map_err(|e| e.to_string())?;
            }
            71..=74 => {
                let range = self.active_range();
                let mut view = self.book.sheet_view().clone();
                match id {
                    71 => view.hidden_rows.extend(range.top..=range.bottom),
                    72 => {
                        for row in range.top..=range.bottom {
                            view.hidden_rows.remove(&row);
                        }
                    }
                    73 => view.hidden_columns.extend(range.left..=range.right),
                    74 => {
                        for col in range.left..=range.right {
                            view.hidden_columns.remove(&col);
                        }
                    }
                    _ => unreachable!(),
                }
                self.book.set_sheet_view(view).map_err(|e| e.to_string())?;
                let selection = self.book.selection();
                self.row = selection.top;
                self.col = selection.left;
            }
            69 => {
                let table = self
                    .book
                    .tables()
                    .iter()
                    .find(|table| {
                        self.row >= table.range.top
                            && self.row <= table.range.bottom
                            && self.col >= table.range.left
                            && self.col <= table.range.right
                    })
                    .ok_or("Select a cell in a table")?
                    .id;
                self.book.remove_table(table).map_err(|e| e.to_string())?;
            }
            23 => {
                self.book
                    .delete_sheet(self.book.active_sheet())
                    .map_err(|e| e.to_string())?;
            }
            29 => {
                self.book
                    .merge(self.active_range())
                    .map_err(|e| e.to_string())?;
            }
            30 => {
                self.book
                    .unmerge(self.active_range())
                    .map_err(|e| e.to_string())?;
            }
            31 => {
                let mut style = self
                    .book
                    .style(self.row, self.col)
                    .ok_or("Invalid selection")?
                    .clone();
                style.bold = !style.bold;
                self.book
                    .set_style(self.active_range(), style)
                    .map_err(|e| e.to_string())?;
            }
            58 => {
                let mut p = self.book.print_settings().clone();
                p.area = Some(self.active_range());
                self.book.set_print_settings(p).map_err(|e| e.to_string())?;
            }
            61 => {
                let mut p = self.book.print_settings().clone();
                p.area = None;
                self.book.set_print_settings(p).map_err(|e| e.to_string())?;
            }
            64 => {
                self.book.set_filter(None).map_err(|e| e.to_string())?;
                self.clear_visible_cache();
            }
            54 => {
                self.preview = None;
            }
            43 => {
                self.book
                    .remove_validation(self.active_range())
                    .map_err(|e| e.to_string())?;
            }
            45 => {
                self.book
                    .remove_conditional_formats(self.active_range())
                    .map_err(|e| e.to_string())?;
            }
            32 | 33 => {
                if self.book.has_external_source() {
                    return Err(
                        "Changing protection on external workbooks is not yet supported".into(),
                    );
                }
                self.book.set_protected(id == 32);
            }
            _ => {}
        }
        self.row = self.row.min(self.book.rows() - 1);
        self.col = self.col.min(self.book.cols() - 1);
        self.normalize_focus();
        self.clear_visible_cache();
        if self
            .preview
            .is_some_and(|id| !self.book.charts().iter().any(|c| c.id == id))
        {
            self.preview = None
        }
        Ok(())
    }
}
fn main() {
    win42::run(Box::new(Xls {
        book: Workbook::new(20, 8).unwrap(),
        row: 0,
        col: 0,
        preview: None,
        visible_cache: std::cell::RefCell::new(Vec::new()),
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    #[test]
    fn cell_editor_preserves_crlf() {
        let mut b = Workbook::new(1, 1).unwrap();
        b.set_cell(0, 0, "a\r\nb".into()).unwrap();
        let mut a = Xls {
            book: b,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        assert_eq!(a.text(), "a\r\nb");
        a.edit("a".into()).unwrap();
        a.select_cell(0, 0);
        a.edit("a\r\nc".into()).unwrap();
        assert_eq!(a.book.cell(0, 0), Some("a\r\nc"));
    }
}
#[cfg(test)]
mod regression {
    use super::*;
    use win42::App;
    #[test]
    fn typed_copy_moves_formula_while_external_text_is_literal() {
        let mut app = Xls {
            book: Workbook::new(4, 4).unwrap(),
            row: 0,
            col: 1,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        app.book.set_cell(0, 1, "=A1".into()).unwrap();
        let bytes = match app.clipboard_request(39).unwrap() {
            win42::ClipboardRequest::WriteDataAndText { bytes, text, .. } => {
                assert!(text.contains("=A1"));
                bytes
            }
            _ => panic!("missing typed copy"),
        };
        app.select_cell(1, 1);
        app.clipboard_data(40, bytes).unwrap();
        assert_eq!(app.book.cell(1, 1), Some("=A2"));
        app.action(4).unwrap();
        assert_eq!(app.book.cell(1, 1), Some(""));
        app.action(5).unwrap();
        assert_eq!(app.book.cell(1, 1), Some("=A2"));
        app.select_cell(2, 1);
        app.clipboard_text(40, "=A1".into()).unwrap();
        assert_eq!(app.book.cell(2, 1), Some("=A1"));
    }
    #[test]
    fn typed_clipboard_translates_formulas_across_filtered_hidden_gaps() {
        let mut book = Workbook::new(9, 5).unwrap();
        for row in [1, 3, 4, 6] {
            book.set_cell(row, 0, format!("hit{row}")).unwrap();
        }
        book.set_cell(1, 1, "=A2".into()).unwrap();
        book.set_cell(3, 3, "=$A4+A$2+$A$2".into()).unwrap();
        let mut style = book.style(1, 1).unwrap().clone();
        style.italic = true;
        book.set_style(
            Range {
                top: 1,
                left: 1,
                bottom: 1,
                right: 1,
            },
            style,
        )
        .unwrap();
        let mut view = book.sheet_view().clone();
        view.hidden_rows.extend([0, 2, 5]);
        view.hidden_columns.insert(2);
        book.set_sheet_view(view).unwrap();
        book.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 8,
                right: 0,
            },
            column: 0,
            contains: "hit".into(),
        }))
        .unwrap();
        let mut app = Xls {
            book,
            row: 1,
            col: 1,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        assert_eq!(
            app.table()
                .unwrap()
                .iter()
                .map(|r| r[0].as_str())
                .collect::<Vec<_>>(),
            ["hit1", "hit3", "hit4", "hit6"]
        );
        app.select_grid_range_with_focus(
            win42::GridRange {
                top: 0,
                left: 1,
                bottom: 1,
                right: 3,
            },
            (0, 1),
        );
        let (text, bytes) = match app.clipboard_request(39).unwrap() {
            win42::ClipboardRequest::WriteDataAndText { text, bytes, .. } => (text, bytes),
            _ => panic!("missing typed copy"),
        };
        assert!(text.contains("=A2"));
        assert!(text.contains("=$A4+A$2+$A$2"));
        assert!(matches!(
            app.clipboard_request(40),
            Some(win42::ClipboardRequest::ReadDataOrText(_))
        ));
        app.select_cell(2, 1);
        assert_eq!(app.row, 4);
        app.clipboard_data(40, bytes).unwrap();
        assert_eq!(app.book.cell(4, 1), Some("=A5"));
        assert_eq!(app.book.cell(6, 3), Some("=$A7+A$2+$A$2"));
        assert!(app.book.style(4, 1).unwrap().italic);
        assert_eq!(app.book.cell(5, 1), Some(""));
        assert_eq!(app.book.cell(4, 2), Some(""));
        app.action(4).unwrap();
        assert_eq!(app.book.cell(4, 1), Some(""));
        assert_eq!(app.book.cell(6, 3), Some(""));
        app.action(5).unwrap();
        assert_eq!(app.book.cell(4, 1), Some("=A5"));
        app.clipboard_text(40, "=A1".into()).unwrap();
        assert_eq!(app.book.cell(4, 1), Some("=A1"));
        assert!(app.clipboard_data(40, b"bad".to_vec()).is_err());
        assert_eq!(app.book.cell(4, 1), Some("=A1"));
    }
    #[test]
    fn labels_snapshot_only_visible_cells_in_the_selection() {
        let mut book = Workbook::new(4, 3).unwrap();
        book.set_cell(0, 0, "Name".into()).unwrap();
        book.set_cell(0, 1, "Hidden field".into()).unwrap();
        book.set_cell(0, 2, "Code".into()).unwrap();
        book.set_cell(1, 0, "Keep".into()).unwrap();
        book.set_cell(1, 2, "PCB-0007".into()).unwrap();
        book.set_cell(2, 0, "Hidden row".into()).unwrap();
        book.set_cell(3, 0, "Filtered row".into()).unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 3,
            right: 2,
        };
        book.set_filter(Some(SheetFilter {
            range,
            column: 0,
            contains: "Keep".into(),
        }))
        .unwrap();
        let mut view = book.sheet_view().clone();
        view.hidden_rows.insert(2);
        view.hidden_columns.insert(1);
        book.set_sheet_view(view).unwrap();
        book.set_selection(range).unwrap();
        let app = Xls {
            book,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: Default::default(),
        };
        let win42::LabelSource::Rows(rows) = app.label_source().unwrap() else {
            panic!("expected rows")
        };
        assert_eq!(
            rows,
            vec![
                (1, vec!["Name".to_string(), "Code".to_string()]),
                (2, vec!["Keep".to_string(), "PCB-0007".to_string()])
            ]
        );
    }

    #[test]
    fn hidden_row_mapping_drives_render_selection_edit_and_mapped_clipboard() {
        let mut book = Workbook::new(8, 5).unwrap();
        for row in 0..8 {
            book.set_cell(row, 0, format!("row{row}")).unwrap();
        }
        book.set_row_height(3, 44).unwrap();
        let mut style = book.style(3, 0).unwrap().clone();
        style.bold = true;
        book.set_style(
            Range {
                top: 3,
                left: 0,
                bottom: 3,
                right: 0,
            },
            style,
        )
        .unwrap();
        let mut view = book.sheet_view().clone();
        view.hidden_rows.extend([0, 2]);
        view.hidden_columns.insert(1);
        view.frozen_rows = 4;
        book.set_sheet_view(view).unwrap();
        let mut app = Xls {
            book,
            row: 1,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        let rendered = app.table().unwrap();
        assert_eq!(
            rendered
                .iter()
                .take(3)
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            ["row1", "row3", "row4"]
        );
        assert_eq!(app.grid_row_label(0), "2");
        assert_eq!(app.grid_row_label(1), "4");
        assert_eq!(app.grid_row_height(1), 44);
        assert!(app.grid_cell_style(1, 0).unwrap().bold);
        assert_eq!(app.grid_frozen_panes(), (2, 0));
        app.select_cell(1, 0);
        assert_eq!(app.row, 3);
        app.edit("edited".into()).unwrap();
        assert_eq!(app.book.cell(3, 0), Some("edited"));
        assert_eq!(app.book.cell(2, 0), Some("row2"));
        app.clipboard_text(40, "a\tb\nc\td".into()).unwrap();
        assert_eq!(app.book.cell(3, 0), Some("a"));
        assert_eq!(app.book.cell(3, 2), Some("b"));
        assert_eq!(app.book.cell(4, 0), Some("c"));
        assert_eq!(app.book.cell(4, 2), Some("d"));
        assert_eq!(app.book.cell(3, 1), Some(""));
        assert_eq!(app.book.cell(4, 1), Some(""));
        assert_eq!(app.book.cell(2, 0), Some("row2"));
        if let Some(win42::ClipboardRequest::WriteDataAndText { text, .. }) =
            app.clipboard_request(39)
        {
            assert!(text.contains("a\tb"));
            assert!(text.contains("c\td"));
        } else {
            panic!("missing mapped clipboard contents");
        }
        app.select_cell(5, 0);
        assert!(app.clipboard_text(40, "overflow\nextra".into()).is_err());
        assert_eq!(app.book.cell(7, 0), Some("row7"));
        app.action(4).unwrap();
        assert_eq!(app.book.cell(3, 0), Some("edited"));
        assert_eq!(app.book.cell(4, 0), Some("row4"));
    }
    #[test]
    fn filtered_hidden_mapping_pastes_only_into_visible_destinations() {
        let mut book = Workbook::new(7, 5).unwrap();
        for (row, value) in [
            "header",
            "hit one",
            "hidden",
            "hit two",
            "hit three",
            "other",
            "other",
        ]
        .iter()
        .enumerate()
        {
            book.set_cell(row, 0, (*value).into()).unwrap();
        }
        let mut view = book.sheet_view().clone();
        view.hidden_rows.extend([0, 2]);
        view.hidden_columns.insert(2);
        view.frozen_rows = 4;
        book.set_sheet_view(view).unwrap();
        book.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 6,
                right: 0,
            },
            column: 0,
            contains: "hit".into(),
        }))
        .unwrap();
        let mut app = Xls {
            book,
            row: 1,
            col: 1,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        let rendered = app.table().unwrap();
        assert_eq!(
            rendered
                .iter()
                .map(|row| row[0].as_str())
                .collect::<Vec<_>>(),
            ["hit one", "hit two", "hit three"]
        );
        assert_eq!(app.grid_row_label(0), "2");
        assert_eq!(app.grid_row_label(1), "4");
        assert_eq!(app.grid_frozen_panes(), (2, 0));
        app.select_grid_range_with_focus(
            win42::GridRange {
                top: 0,
                left: 1,
                bottom: 1,
                right: 3,
            },
            (0, 1),
        );
        assert_eq!(app.selected_cell(), Some((0, 1)));
        assert_eq!(app.row, 1);
        app.clipboard_text(40, "a\tb\nc\td".into()).unwrap();
        assert_eq!(app.book.cell(1, 1), Some("a"));
        assert_eq!(app.book.cell(1, 3), Some("b"));
        assert_eq!(app.book.cell(3, 1), Some("c"));
        assert_eq!(app.book.cell(3, 3), Some("d"));
        assert_eq!(app.book.cell(2, 1), Some(""));
        assert_eq!(app.book.cell(1, 2), Some(""));
        app.action(4).unwrap();
        assert_eq!(app.book.cell(1, 1), Some(""));
        assert_eq!(app.book.cell(3, 3), Some(""));
    }
    #[test]
    fn view_commands_preserve_hidden_cell_destinations_and_history() {
        let mut app = Xls {
            book: Workbook::new(6, 5).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        app.book.set_cell(1, 1, "hidden value".into()).unwrap();
        app.book
            .set_selection(Range {
                top: 1,
                left: 1,
                bottom: 1,
                right: 1,
            })
            .unwrap();
        app.row = 1;
        app.col = 1;
        app.action(71).unwrap();
        assert_eq!(app.grid_row_label(1), "3");
        assert_eq!(app.table().unwrap().len(), 5);
        assert_ne!(app.row, 1);
        app.select_cell(1, 1);
        app.edit("visible destination".into()).unwrap();
        assert_eq!(app.book.cell(2, 1), Some("visible destination"));
        app.action(4).unwrap();
        assert_eq!(app.book.cell(2, 1), Some(""));
        app.action(4).unwrap();
        assert_eq!(app.grid_row_label(1), "2");
        app.action(5).unwrap();
        assert_eq!(app.grid_row_label(1), "3");
        app.book
            .set_selection(Range {
                top: 0,
                left: 0,
                bottom: 2,
                right: 2,
            })
            .unwrap();
        app.row = 0;
        app.col = 0;
        app.action(72).unwrap();
        assert_eq!(app.grid_row_label(1), "2");
        assert_eq!(app.book.cell(1, 1), Some("hidden value"));
        app.input_action(70, &["2".into(), "1".into()]).unwrap();
        assert_eq!(app.grid_frozen_panes(), (2, 1));
    }
    #[test]
    fn table_commands_create_style_and_convert_to_range() {
        let mut app = Xls {
            book: Workbook::new(5, 2).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        app.book.set_cell(0, 0, "Amount".into()).unwrap();
        app.book.set_cell(1, 0, "10".into()).unwrap();
        app.book.set_cell(2, 0, "20".into()).unwrap();
        app.book
            .set_selection(Range {
                top: 0,
                left: 0,
                bottom: 3,
                right: 0,
            })
            .unwrap();
        app.input_action(68, &["Sales".into(), "yes".into()])
            .unwrap();
        assert_eq!(app.book.display_grid()[3][0], "30");
        assert!(app.grid_cell_style(0, 0).unwrap().bold);
        app.action(69).unwrap();
        assert!(app.book.tables().is_empty());
        app.action(4).unwrap();
        assert_eq!(app.book.tables().len(), 1);
    }
    #[test]
    fn same_shape_open_and_keyboard_selection_edit_the_visible_destination() {
        let root =
            std::env::temp_dir().join(format!("xls42-grid-selection-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir(&root).unwrap();
        let input = root.join("input.csv");
        std::fs::write(&input, "a,b\nc,d\n").unwrap();
        let mut app = Xls {
            book: Workbook::new(2, 2).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        app.select_cell(1, 1);
        let prior = app.table().unwrap();
        app.open(&input).unwrap();
        let after = app.table().unwrap();
        assert!(matches!(
            win42::grid_transition(Some(&prior), &after, app.selected_cell()),
            win42::GridTransition::Update { .. }
        ));
        assert_eq!(app.selected_cell(), Some((0, 0)));
        app.edit("opened".into()).unwrap();
        let selected =
            win42::grid_selection_event(app.selected_cell(), Some(1), None, false).unwrap();
        app.select_cell(selected.0, selected.1);
        app.edit("keyboard".into()).unwrap();
        assert!(win42::grid_selection_event(app.selected_cell(), Some(1), None, true).is_none());
        let output = root.join("out.csv");
        app.save(&output).unwrap();
        let saved = Workbook::open_csv(&output).unwrap();
        for (r, c, value) in [
            (0, 0, "opened"),
            (0, 1, "b"),
            (1, 0, "keyboard"),
            (1, 1, "d"),
        ] {
            assert_eq!(saved.cell(r, c), Some(value));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn selection_survives_structure_and_history_and_saves_exact_destination() {
        let mut a = Xls {
            book: Workbook::new(2, 2).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        a.select_cell(1, 1);
        a.action(11).unwrap();
        assert_eq!(a.selected_cell(), Some((1, 1)));
        a.edit("target".into()).unwrap();
        a.action(4).unwrap();
        assert_eq!(a.book.cell(1, 1), Some(""));
        a.action(5).unwrap();
        assert_eq!(a.book.cell(1, 1), Some("target"));
        a.action(10).unwrap();
        a.select_cell(2, 1);
        a.action(4).unwrap();
        assert_eq!(a.selected_cell(), Some((1, 1)));
        a.edit("final".into()).unwrap();
        let path = std::env::temp_dir().join(format!("xls42-selection-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        a.save(&path).unwrap();
        let b = Workbook::open_csv(&path).unwrap();
        assert_eq!((b.rows(), b.cols()), (2, 3));
        for r in 0..2 {
            for c in 0..3 {
                assert_eq!(
                    b.cell(r, c),
                    Some(if (r, c) == (1, 1) { "final" } else { "" })
                );
            }
        }
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn cell_style_survives_switch_and_no_newlines() {
        let mut b = Workbook::new(1, 2).unwrap();
        b.set_cell(0, 0, "a\r\nb".into()).unwrap();
        let mut a = Xls {
            book: b,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        a.edit("a".into()).unwrap();
        a.select_cell(0, 1);
        a.edit("other".into()).unwrap();
        a.select_cell(0, 0);
        a.edit("a\r\nc".into()).unwrap();
        assert_eq!(a.book.cell(0, 0), Some("a\r\nc"));
        let path = std::env::temp_dir().join(format!("xls42-style-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        a.save(&path).unwrap();
        let b = Workbook::open_csv(&path).unwrap();
        assert_eq!(b.cell(0, 0), Some("a\r\nc"));
        assert_eq!(b.cell(0, 1), Some("other"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn limits_disable_and_refuse_additions() {
        let mut a = Xls {
            book: Workbook::new(1000, 100).unwrap(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        assert!(!a.enabled(10));
        assert!(!a.enabled(11));
        assert!(a.action(10).is_err());
        assert!(a.action(11).is_err());
        assert_eq!((a.book.rows(), a.book.cols()), (1000, 100));
    }
}
#[cfg(test)]
mod policy_regression {
    use super::*;
    use win42::App;
    #[test]
    fn rejection_undo_branch_and_context_preserve_cell_policy() {
        let mut b = Workbook::new(1, 2).unwrap();
        b.set_cell(0, 0, "a\r\nb".into()).unwrap();
        let mut a = Xls {
            book: b,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        a.edit("single".into()).unwrap();
        assert!(a.edit("x".repeat(4 * 1024 * 1024 + 1)).is_err());
        a.action(4).unwrap();
        assert_eq!(a.book.cell(0, 0), Some("a\r\nb"));
        a.edit("branch".into()).unwrap();
        assert!(!a.can_redo());
        a.select_cell(0, 1);
        a.edit("other".into()).unwrap();
        a.select_cell(0, 0);
        a.edit("branch\r\nend".into()).unwrap();
        assert_eq!(a.book.cell(0, 0), Some("branch\r\nend"));
    }
}

fn parse_chart_kind(s: &str) -> Result<ChartKind, String> {
    match s.to_lowercase().as_str() {
        "bar" => Ok(ChartKind::Bar),
        "line" => Ok(ChartKind::Line),
        "pie" => Ok(ChartKind::Pie),
        "scatter" => Ok(ChartKind::Scatter),
        _ => Err("Choose bar, line, pie or scatter".into()),
    }
}
#[cfg(test)]
mod filter_ui_regression {
    use super::*;
    use win42::App;
    #[test]
    fn visible_selection_edits_actual_filtered_row() {
        let mut b = Workbook::new(5, 2).unwrap();
        for (r, v) in ["header", "apple", "pear", "apricot", "plum"]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*v).into()).unwrap();
        }
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 4,
                right: 0,
            },
            column: 0,
            contains: "ap".into(),
        }))
        .unwrap();
        let mut app = Xls {
            book: b,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        assert_eq!(app.table().unwrap().len(), 3);
        app.select_cell(2, 0);
        assert_eq!(app.row, 3);
        app.edit("apex".into()).unwrap();
        assert_eq!(app.book.cell(3, 0), Some("apex"));
        assert_eq!(app.book.cell(2, 0), Some("pear"));
    }
}
#[cfg(test)]
mod custom_grid_regression {
    use super::*;
    use win42::App;
    #[test]
    fn visible_row_resize_selection_and_history_use_real_destination() {
        let mut b = Workbook::new(5, 3).unwrap();
        for (r, v) in ["Header", "apple", "pear", "apricot", "plum"]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*v).into()).unwrap();
        }
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 4,
                right: 0,
            },
            column: 0,
            contains: "ap".into(),
        }))
        .unwrap();
        let mut app = Xls {
            book: b,
            row: 0,
            col: 0,
            preview: None,
            visible_cache: std::cell::RefCell::new(Vec::new()),
        };
        assert!(app.uses_custom_grid());
        assert_eq!(app.table().unwrap().len(), 3);
        assert_eq!(app.grid_row_label(2), "4");
        app.resize_grid_row(2, 45).unwrap();
        assert_eq!(app.book.row_height(3), Some(45));
        assert_eq!(app.book.row_height(2), None);
        app.book.undo();
        assert_eq!(app.book.row_height(3), None);
        app.book.redo();
        assert_eq!(app.book.row_height(3), Some(45));
        app.select_grid_range_with_focus(
            win42::GridRange {
                top: 1,
                left: 1,
                bottom: 2,
                right: 2,
            },
            (2, 2),
        );
        assert_eq!((app.row, app.col), (3, 2));
        app.edit("target".into()).unwrap();
        assert_eq!(app.book.cell(3, 2), Some("target"));
        assert_eq!(app.book.cell(2, 2), Some(""));
    }
}
