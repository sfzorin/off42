//! Bounded sparse workbook. Raw text remains authoritative for editing and CSV.
mod analytics;
mod clipboard;
mod tables;
pub use tables::SpreadsheetTable;
mod layout;
pub use layout::{PrintSettings, SheetFilter, SheetView};
mod display;
pub use analytics::{Chart, ChartData, ChartKind, ChartSeries, Pivot, PivotAggregate};
mod formula;
mod office;
mod project;
mod rules;
pub use rules::{Condition, ConditionalFormat, DataValidation, ValidationKind};
mod xlsx_styles;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};
use suite42_core::{write_new, SnapshotHistory, TextValue};
const MAX_INPUT: usize = 4 * 1024 * 1024;
const MAX_ROWS: usize = 1000;
const MAX_COLS: usize = 100;
const MAX_TEXT: usize = 4 * 1024 * 1024;
const MAX_SHEETS: usize = 64;
#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Csv(csv::Error),
    Limit,
    Bounds,
    Invalid(&'static str),
    Protected,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::Csv(e) => write!(f, "{e}"),
            Self::Limit => write!(f, "spreadsheet limit exceeded"),
            Self::Bounds => write!(f, "cell outside table"),
            Self::Invalid(e) => write!(f, "{e}"),
            Self::Protected => write!(f, "sheet is protected"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<csv::Error> for Error {
    fn from(e: csv::Error) -> Self {
        Self::Csv(e)
    }
}
pub type SheetId = u64;
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CellStyle {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub font: String,
    pub font_size: u16,
    pub foreground: Option<u32>,
    pub background: Option<u32>,
    pub number_format: String,
    pub borders: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Range {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
}
impl Range {
    fn valid(self, s: &Sheet) -> bool {
        self.top <= self.bottom
            && self.left <= self.right
            && self.bottom < s.rows
            && self.right < s.cols
    }
    fn overlaps(self, b: Self) -> bool {
        self.top <= b.bottom && b.top <= self.bottom && self.left <= b.right && b.left <= self.right
    }
}
/// Authoritative cell kind and editor text; variants preserve explicit text vs numeric types.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum CellContent {
    Text(TextValue),
    Number(TextValue),
    Boolean(TextValue),
    Date(TextValue),
    Formula(TextValue),
    Error(TextValue),
}
impl CellContent {
    fn from_raw(raw: String) -> Self {
        let t = TextValue::from_raw(raw);
        let v = t.raw();
        if v.starts_with('=') {
            Self::Formula(t)
        } else if v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("false") {
            Self::Boolean(t)
        } else if v.parse::<f64>().is_ok_and(f64::is_finite) {
            Self::Number(t)
        } else {
            Self::Text(t)
        }
    }
    fn raw(&self) -> &str {
        match self {
            Self::Text(v)
            | Self::Number(v)
            | Self::Boolean(v)
            | Self::Date(v)
            | Self::Formula(v)
            | Self::Error(v) => v.raw(),
        }
    }
    fn with_edited(&self, raw: &str) -> Self {
        let v = match self {
            Self::Text(v)
            | Self::Number(v)
            | Self::Boolean(v)
            | Self::Date(v)
            | Self::Formula(v)
            | Self::Error(v) => v.with_edited(raw),
        };
        let mut next = Self::from_raw(v.raw().to_owned());
        match &mut next {
            Self::Text(t)
            | Self::Number(t)
            | Self::Boolean(t)
            | Self::Date(t)
            | Self::Formula(t)
            | Self::Error(t) => *t = v,
        };
        next
    }
}
impl Default for CellContent {
    fn default() -> Self {
        Self::Text(TextValue::default())
    }
}
#[derive(Clone, PartialEq)]
struct Cell {
    text: CellContent,
    style: usize,
}
#[derive(Clone, PartialEq)]
struct Sheet {
    id: SheetId,
    name: String,
    rows: usize,
    cols: usize,
    cells: BTreeMap<(usize, usize), Cell>,
    merges: Vec<Range>,
    row_heights: BTreeMap<usize, u16>,
    col_widths: BTreeMap<usize, u16>,
    protected: bool,
    validations: Vec<DataValidation>,
    conditional_formats: Vec<ConditionalFormat>,
    charts: Vec<Chart>,
    pivots: Vec<Pivot>,
    tables: Vec<SpreadsheetTable>,
    view: SheetView,
    opaque_table_headers: Vec<Range>,
    protected_formula_ranges: Vec<Range>,
    shared_formula_cells: Vec<(usize, usize)>,
    filter: Option<SheetFilter>,
    print: PrintSettings,
    copied_from: Option<SheetId>,
    source_structure: bool,
    source_edits: Vec<StructuralEdit>,
}
#[derive(Clone, Debug, PartialEq)]
struct StructuralEdit {
    rows: bool,
    at: usize,
    count: usize,
    delete: bool,
}
impl Sheet {
    fn new(id: SheetId, name: String, rows: usize, cols: usize) -> Self {
        Self {
            id,
            name,
            rows,
            cols,
            cells: BTreeMap::new(),
            merges: Vec::new(),
            row_heights: BTreeMap::new(),
            col_widths: BTreeMap::new(),
            protected: false,
            validations: Vec::new(),
            conditional_formats: Vec::new(),
            charts: Vec::new(),
            pivots: Vec::new(),
            tables: Vec::new(),
            view: SheetView::default(),
            opaque_table_headers: Vec::new(),
            protected_formula_ranges: Vec::new(),
            shared_formula_cells: Vec::new(),
            filter: None,
            print: PrintSettings::default(),
            copied_from: None,
            source_structure: false,
            source_edits: Vec::new(),
        }
    }
}
#[derive(Clone, PartialEq)]
struct State {
    sheets: Vec<Sheet>,
    active: SheetId,
    next_id: SheetId,
    styles: Vec<CellStyle>,
    selection: Range,
    date_1904: bool,
}
impl State {
    fn sheet(&self) -> &Sheet {
        self.sheets.iter().find(|s| s.id == self.active).unwrap()
    }
    fn sheet_mut(&mut self) -> &mut Sheet {
        self.sheets
            .iter_mut()
            .find(|s| s.id == self.active)
            .unwrap()
    }
    fn bytes(&self) -> usize {
        self.sheets
            .iter()
            .flat_map(|s| s.cells.values())
            .map(|c| c.text.raw().len())
            .sum()
    }
}
pub struct Workbook {
    history: SnapshotHistory<State>,
    snapshots: BTreeMap<u64, State>,
    next_revision: u64,
    external_source: Option<Vec<u8>>,
    external_baseline: Option<State>,
    external_format: Option<String>,
    external_cache: BTreeMap<(SheetId, usize, usize), String>,
}
impl Workbook {
    pub fn new(rows: usize, cols: usize) -> Result<Self, Error> {
        if rows == 0 || cols == 0 || rows > MAX_ROWS || cols > MAX_COLS {
            return Err(Error::Limit);
        };
        Ok(Self {
            history: SnapshotHistory::new(
                State {
                    sheets: vec![Sheet::new(1, "Sheet1".into(), rows, cols)],
                    active: 1,
                    next_id: 2,
                    styles: vec![CellStyle::default()],
                    date_1904: false,
                    selection: Range {
                        top: 0,
                        left: 0,
                        bottom: 0,
                        right: 0,
                    },
                },
                8,
            ),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        })
    }
    pub fn open_csv(path: &Path) -> Result<Self, Error> {
        let mut data = Vec::new();
        File::open(path)?
            .take((MAX_INPUT + 1) as u64)
            .read_to_end(&mut data)?;
        if data.len() > MAX_INPUT {
            return Err(Error::Limit);
        }
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(false)
            .flexible(true)
            .from_reader(data.as_slice());
        let mut records = Vec::new();
        let mut cols = 1;
        for rec in reader.records() {
            let rec = rec?;
            if records.len() == MAX_ROWS || rec.len() > MAX_COLS {
                return Err(Error::Limit);
            }
            cols = cols.max(rec.len());
            records.push(rec);
        }
        let mut b = Self::new(records.len().max(1), cols)?;
        let mut state = b.history.current().clone();
        for (r, rec) in records.iter().enumerate() {
            for (c, v) in rec.iter().enumerate() {
                if !v.is_empty() {
                    state.sheet_mut().cells.insert(
                        (r, c),
                        Cell {
                            text: CellContent::from_raw(v.to_owned()),
                            style: 0,
                        },
                    );
                }
            }
        }
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        b.history = SnapshotHistory::new(state, 8);
        Ok(b)
    }
    pub fn display_grid(&self) -> Vec<Vec<String>> {
        self.display_grid_at(chrono::Local::now().naive_local())
    }
    fn display_grid_at(&self, now: chrono::NaiveDateTime) -> Vec<Vec<String>> {
        let mut grid = formula::display_grid_at(self, now);
        if self
            .external_baseline
            .as_ref()
            .is_some_and(|base| base.sheets == self.history.current().sheets)
        {
            for (&(sheet, row, col), value) in &self.external_cache {
                if sheet == self.active_sheet()
                    && row < grid.len()
                    && col < grid[row].len()
                    && grid[row][col] == "#ERROR!"
                {
                    grid[row][col] = value.clone();
                }
            }
        }
        display::apply(self, &mut grid);
        grid
    }
    pub fn is_read_only(&self) -> bool {
        self.external_source.is_some() && self.external_format.as_deref() == Some("xls")
    }
    fn require_editable(&self) -> Result<(), Error> {
        if self.is_read_only() {
            Err(Error::Invalid(
                "External workbook is read-only: unsupported content cannot yet be preserved",
            ))
        } else {
            Ok(())
        }
    }

    fn require_structural_edit(&self) -> Result<(), Error> {
        self.require_editable()?;
        if self.external_source.is_some() {
            Err(Error::Invalid("This edit changes workbook structure or formatting; the original Office parts cannot yet be updated safely"))
        } else {
            Ok(())
        }
    }
    fn require_no_external_conversion(&self) -> Result<(), Error> {
        if self.external_source.is_some() {
            Err(Error::Invalid(
                "Conversion would discard unsupported Office content; save in the original format",
            ))
        } else {
            Ok(())
        }
    }
    pub fn has_external_source(&self) -> bool {
        self.external_source.is_some()
    }
    pub fn rows(&self) -> usize {
        self.history.current().sheet().rows
    }
    pub fn cols(&self) -> usize {
        self.history.current().sheet().cols
    }
    pub fn cell(&self, row: usize, col: usize) -> Option<&str> {
        let s = self.history.current().sheet();
        if row >= s.rows || col >= s.cols {
            None
        } else {
            Some(s.cells.get(&(row, col)).map_or("", |c| c.text.raw()))
        }
    }
    pub fn set_cell(&mut self, row: usize, col: usize, value: String) -> Result<bool, Error> {
        self.set_text(row, col, value, false)
    }
    pub fn set_cell_editor_text(
        &mut self,
        row: usize,
        col: usize,
        value: String,
    ) -> Result<bool, Error> {
        self.set_text(row, col, value, true)
    }
    fn set_text(
        &mut self,
        row: usize,
        col: usize,
        value: String,
        editor: bool,
    ) -> Result<bool, Error> {
        self.require_editable()?;
        let mut state = self.history.current().clone();
        let s = state.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if row >= s.rows || col >= s.cols {
            return Err(Error::Bounds);
        }
        if s.merges.iter().any(|m| {
            row >= m.top
                && row <= m.bottom
                && col >= m.left
                && col <= m.right
                && (row, col) != (m.top, m.left)
        }) {
            return Err(Error::Invalid("Edit the top-left cell of a merged range"));
        }
        let old = s.cells.get(&(row, col)).cloned().unwrap_or(Cell {
            text: CellContent::default(),
            style: 0,
        });
        let text = if editor {
            old.text.with_edited(&value)
        } else {
            CellContent::from_raw(value)
        };
        if text.raw().is_empty() && old.style == 0 {
            s.cells.remove(&(row, col));
        } else {
            s.cells.insert(
                (row, col),
                Cell {
                    text,
                    style: old.style,
                },
            );
        }
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        rules::validate_range(
            &state,
            Range {
                top: row,
                left: col,
                bottom: row,
                right: col,
            },
        )?;
        tables::reconcile_headers(self.history.current(), &mut state)?;
        self.validate_external_table_edit(&state)?;
        Ok(self.history.apply(state))
    }
    pub fn can_add_row(&self) -> bool {
        self.rows() < MAX_ROWS
    }
    pub fn can_add_column(&self) -> bool {
        self.cols() < MAX_COLS
    }
    pub fn add_row(&mut self) -> Result<(), Error> {
        if !self.can_add_row() {
            return Err(Error::Limit);
        }
        self.insert_rows(self.rows(), 1)
    }
    pub fn add_column(&mut self) -> Result<(), Error> {
        if !self.can_add_column() {
            return Err(Error::Limit);
        }
        self.insert_columns(self.cols(), 1)
    }
    pub fn undo(&mut self) -> bool {
        !self.is_read_only() && self.history.undo()
    }
    pub fn redo(&mut self) -> bool {
        !self.is_read_only() && self.history.redo()
    }
    pub fn can_undo(&self) -> bool {
        !self.is_read_only() && self.history.can_undo()
    }
    pub fn can_redo(&self) -> bool {
        !self.is_read_only() && self.history.can_redo()
    }
    pub fn is_dirty(&self) -> bool {
        !self.is_read_only() && self.history.is_dirty()
    }
    pub fn csv_bytes(&self) -> Result<Vec<u8>, Error> {
        self.require_editable()?;
        self.require_no_external_conversion()?;
        let state = self.history.current();
        if state.sheets.len() != 1
            || state
                .sheet()
                .cells
                .values()
                .any(|c| c.style != 0 || c.text != CellContent::from_raw(c.text.raw().to_owned()))
            || !state.sheet().merges.is_empty()
            || !state.sheet().row_heights.is_empty()
            || !state.sheet().col_widths.is_empty()
            || state.sheet().protected
            || !state.sheet().validations.is_empty()
            || !state.sheet().conditional_formats.is_empty()
            || !state.sheet().charts.is_empty()
            || !state.sheet().pivots.is_empty()
            || !state.sheet().tables.is_empty()
            || state.sheet().filter.is_some()
            || state.sheet().view != SheetView::default()
            || state.sheet().print != PrintSettings::default()
        {
            return Err(Error::Invalid(
                "CSV cannot preserve workbook sheets, formatting, geometry, merges or protection",
            ));
        }
        let mut w = csv::WriterBuilder::new().from_writer(Vec::new());
        for r in 0..self.rows() {
            w.write_record((0..self.cols()).map(|c| self.cell(r, c).unwrap()))?;
        }
        let bytes = w.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        if bytes.len() > MAX_INPUT {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub fn save_as_csv(&mut self, path: &Path) -> Result<(), Error> {
        let bytes = self.csv_bytes()?;
        write_new(path, &bytes)?;
        self.history.mark_saved();
        Ok(())
    }
    pub fn sheets(&self) -> Vec<(SheetId, String)> {
        self.history
            .current()
            .sheets
            .iter()
            .map(|s| (s.id, s.name.clone()))
            .collect()
    }
    pub fn active_sheet(&self) -> SheetId {
        self.history.current().active
    }
    pub fn select_sheet(&mut self, id: SheetId) -> Result<bool, Error> {
        let mut st = self.history.current().clone();
        if !st.sheets.iter().any(|s| s.id == id) {
            return Err(Error::Bounds);
        }
        st.active = id;
        st.selection = Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        };
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn add_sheet(&mut self, name: &str) -> Result<SheetId, Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Sheet changes in this external format are not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        validate_name(&st, name, None)?;
        if st.sheets.len() >= MAX_SHEETS {
            return Err(Error::Limit);
        }
        let id = st.next_id;
        st.next_id += 1;
        let (rows, cols) = (st.sheet().rows, st.sheet().cols);
        st.sheets.push(Sheet::new(id, name.into(), rows, cols));
        st.active = id;
        st.selection = Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        };
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        self.history.apply(st);
        Ok(id)
    }
    pub fn duplicate_sheet(&mut self, id: SheetId, name: &str) -> Result<SheetId, Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Sheet changes in this external format are not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        validate_name(&st, name, None)?;
        if st.sheets.len() >= MAX_SHEETS {
            return Err(Error::Limit);
        }
        let mut sheet = st
            .sheets
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or(Error::Bounds)?;
        if self.has_external_source() && !sheet.protected_formula_ranges.is_empty() {
            return Err(Error::Invalid(
                "Duplicating sheets with shared or array formulas is not yet supported safely",
            ));
        }
        if !sheet.charts.is_empty() || !sheet.pivots.is_empty() || !sheet.tables.is_empty() {
            return Err(Error::Invalid(
                "Duplicate chart and pivot sheets after removing their objects",
            ));
        }
        if self.has_external_source() {
            let source = if let Some(source_id) = sheet.copied_from {
                source_id
            } else {
                id
            };
            if self
                .external_baseline
                .as_ref()
                .is_some_and(|base| base.sheets.iter().any(|s| s.id == source))
            {
                sheet.copied_from = Some(source);
            } else {
                sheet.copied_from = None;
            }
        }
        let source_name = sheet.name.clone();
        sheet.id = st.next_id;
        sheet.name = name.into();
        for cell in sheet.cells.values_mut() {
            if matches!(cell.text, CellContent::Formula(_)) {
                cell.text =
                    CellContent::from_raw(rewrite_sheet_refs(cell.text.raw(), &source_name, name));
            }
        }
        st.next_id += 1;
        let new_id = sheet.id;
        st.sheets.push(sheet);
        st.active = new_id;
        if st.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        self.history.apply(st);
        Ok(new_id)
    }
    pub fn rename_sheet(&mut self, id: SheetId, name: &str) -> Result<bool, Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Renaming sheets in this external format is not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        validate_name(&st, name, Some(id))?;
        let old = st
            .sheets
            .iter()
            .find(|s| s.id == id)
            .ok_or(Error::Bounds)?
            .name
            .clone();
        st.sheets.iter_mut().find(|s| s.id == id).unwrap().name = name.into();
        for sheet in &mut st.sheets {
            for cell in sheet.cells.values_mut() {
                if matches!(cell.text, CellContent::Formula(_)) {
                    let changed = rewrite_sheet_refs(cell.text.raw(), &old, name);
                    if changed != cell.text.raw() {
                        cell.text = CellContent::from_raw(changed)
                    }
                }
            }
        }
        if st.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn delete_sheet(&mut self, id: SheetId) -> Result<bool, Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Sheet changes in this external format are not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        if st.sheets.len() == 1 {
            return Err(Error::Invalid("cannot delete last sheet"));
        }
        let idx = st
            .sheets
            .iter()
            .position(|s| s.id == id)
            .ok_or(Error::Bounds)?;
        if st.sheets.iter().filter(|s| s.id != id).any(|s| {
            s.charts
                .iter()
                .any(|c| c.series.iter().any(|v| v.sheet == id))
                || s.pivots
                    .iter()
                    .any(|p| p.source_sheet == id || p.destination_sheet == id)
        }) {
            return Err(Error::Invalid("Sheet is used by a chart or pivot"));
        }
        if self.has_external_source() && st.sheets[idx].protected {
            return Err(Error::Protected);
        }
        let name = st.sheets[idx].name.clone();
        let table_tokens: Vec<_> = st.sheets[idx]
            .tables
            .iter()
            .map(|table| format!("{}[", table.name.to_ascii_lowercase()))
            .collect();
        if st
            .sheets
            .iter()
            .filter(|s| s.id != id)
            .flat_map(|s| s.cells.values())
            .any(|cell| {
                matches!(cell.text, CellContent::Formula(_))
                    && table_tokens
                        .iter()
                        .any(|token| cell.text.raw().to_ascii_lowercase().contains(token))
            })
        {
            return Err(Error::Invalid(
                "Sheet contains a table referenced by another sheet",
            ));
        }
        if st
            .sheets
            .iter()
            .filter(|s| s.id != id)
            .flat_map(|s| s.cells.values())
            .any(|c| {
                matches!(c.text, CellContent::Formula(_))
                    && rewrite_sheet_refs(c.text.raw(), &name, "XLS42_Deleted_Reference")
                        != c.text.raw()
            })
        {
            return Err(Error::Invalid(
                "Sheet is referenced by a formula; remove the references before deleting",
            ));
        }
        st.sheets.remove(idx);
        if st.active == id {
            st.active = st.sheets[idx.min(st.sheets.len() - 1)].id;
            st.selection = Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            }
        }
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn reorder_sheet(&mut self, id: SheetId, index: usize) -> Result<bool, Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Reordering sheets in this external format is not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        if index >= st.sheets.len() {
            return Err(Error::Bounds);
        }
        let old = st
            .sheets
            .iter()
            .position(|s| s.id == id)
            .ok_or(Error::Bounds)?;
        let s = st.sheets.remove(old);
        st.sheets.insert(index, s);
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn selection(&self) -> Range {
        self.history.current().selection
    }
    pub fn set_selection(&mut self, range: Range) -> Result<bool, Error> {
        let mut st = self.history.current().clone();
        if !range.valid(st.sheet()) {
            return Err(Error::Bounds);
        }
        st.selection = range;
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn style(&self, row: usize, col: usize) -> Option<&CellStyle> {
        let st = self.history.current();
        let s = st.sheet();
        if row >= s.rows || col >= s.cols {
            return None;
        }
        Some(&st.styles[s.cells.get(&(row, col)).map_or(0, |c| c.style)])
    }
    pub fn set_style(&mut self, range: Range, style: CellStyle) -> Result<bool, Error> {
        self.require_editable()?;
        if style.borders & !15 != 0 {
            return Err(Error::Invalid("unsupported border style"));
        }
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("ods" | "xlsx"))
        {
            return Err(Error::Invalid(
                "Formatting this Office format cannot yet preserve all original style properties",
            ));
        }
        if self.external_format.as_deref() == Some("ods") {
            if style.borders & !15 != 0 {
                return Err(Error::Invalid("Unsupported ODS border"));
            }
            let sheet = self.history.current().sheet();
            if !range.valid(sheet) {
                return Err(Error::Bounds);
            }
            for r in range.top..=range.bottom {
                for c in range.left..=range.right {
                    if self.style(r, c).unwrap().number_format != style.number_format {
                        return Err(Error::Invalid(
                            "ODS number-format editing is not yet supported",
                        ));
                    }
                }
            }
        }
        let mut st = self.history.current().clone();
        if !range.valid(st.sheet()) {
            return Err(Error::Bounds);
        }
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        let id = match st.styles.iter().position(|s| *s == style) {
            Some(i) => i,
            None => {
                st.styles.push(style);
                st.styles.len() - 1
            }
        };
        let s = st.sheet_mut();
        for r in range.top..=range.bottom {
            for c in range.left..=range.right {
                let cell = s.cells.entry((r, c)).or_insert_with(|| Cell {
                    text: CellContent::default(),
                    style: 0,
                });
                cell.style = id;
                if cell.text.raw().is_empty() && id == 0 {
                    s.cells.remove(&(r, c));
                }
            }
        }
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn merges(&self) -> &[Range] {
        &self.history.current().sheet().merges
    }
    pub fn merge(&mut self, range: Range) -> Result<bool, Error> {
        self.require_editable()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if !range.valid(s) {
            return Err(Error::Bounds);
        }
        if range.top < s.view.frozen_rows && range.bottom >= s.view.frozen_rows
            || range.left < s.view.frozen_columns && range.right >= s.view.frozen_columns
        {
            return Err(Error::Invalid("Merged cells cannot cross a frozen pane"));
        }
        if range.top == range.bottom && range.left == range.right {
            return Err(Error::Invalid("Merge requires at least two cells"));
        }
        if s.merges.iter().any(|m| m.overlaps(range)) {
            return Err(Error::Invalid("overlapping merge"));
        }
        if s.cells.iter().any(|(&(r, c), v)| {
            r >= range.top
                && r <= range.bottom
                && c >= range.left
                && c <= range.right
                && (r, c) != (range.top, range.left)
                && !v.text.raw().is_empty()
        }) {
            return Err(Error::Invalid("merge would hide data"));
        }
        s.merges.push(range);
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn unmerge(&mut self, range: Range) -> Result<bool, Error> {
        self.require_editable()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        s.merges.retain(|m| *m != range);
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn set_protected(&mut self, protected: bool) -> bool {
        if self.is_read_only() || self.has_external_source() {
            return false;
        }
        let mut st = self.history.current().clone();
        st.sheet_mut().protected = protected;
        self.history.apply(st)
    }
    pub fn is_protected(&self) -> bool {
        self.history.current().sheet().protected
    }
    pub fn insert_rows(&mut self, at: usize, count: usize) -> Result<(), Error> {
        self.structure(true, at, count, false)
    }
    pub fn delete_rows(&mut self, at: usize, count: usize) -> Result<(), Error> {
        self.structure(true, at, count, true)
    }
    pub fn insert_columns(&mut self, at: usize, count: usize) -> Result<(), Error> {
        self.structure(false, at, count, false)
    }
    pub fn delete_columns(&mut self, at: usize, count: usize) -> Result<(), Error> {
        self.structure(false, at, count, true)
    }
    fn structure(
        &mut self,
        rows: bool,
        at: usize,
        count: usize,
        delete: bool,
    ) -> Result<(), Error> {
        self.require_editable()?;
        if self.has_external_source()
            && !matches!(self.external_format.as_deref(), Some("xlsx" | "ods"))
        {
            return Err(Error::Invalid(
                "Structural edits are not yet supported for this external format",
            ));
        }
        let mut st = self.history.current().clone();
        let active = st.active;
        if !self.has_external_source() && !st.sheet().tables.is_empty() {
            return Err(Error::Invalid(
                "Resize or remove tables before structural edits",
            ));
        }
        if st.sheets.iter().any(|s| {
            s.charts
                .iter()
                .any(|c| c.series.iter().any(|v| v.sheet == active))
                || s.pivots
                    .iter()
                    .any(|p| p.source_sheet == active || p.destination_sheet == active)
        }) {
            return Err(Error::Invalid(
                "Structural edit has chart or pivot references",
            ));
        }
        let name = st.sheet().name.clone();
        if st
            .sheets
            .iter()
            .flat_map(|sheet| sheet.cells.values())
            .any(|cell| {
                matches!(cell.text, CellContent::Formula(_))
                    && has_3d_sheet_reference(cell.text.raw())
            })
        {
            return Err(Error::Invalid(
                "Three-dimensional sheet references cannot yet be shifted safely",
            ));
        }
        let external = self.has_external_source();
        let s = st.sheet_mut();
        if external {
            if s.source_edits.len() >= 4096 {
                return Err(Error::Limit);
            }
            s.source_structure = true;
            s.source_edits.push(StructuralEdit {
                rows,
                at,
                count,
                delete,
            });
        }
        if !s.validations.is_empty()
            || !s.conditional_formats.is_empty()
            || !s.charts.is_empty()
            || !s.pivots.is_empty()
            || s.filter.is_some()
            || s.print != PrintSettings::default()
        {
            return Err(Error::Invalid(
                "Remove sheet validation and conditional rules before structural edits",
            ));
        }
        if s.protected {
            return Err(Error::Protected);
        }
        let size = if rows { s.rows } else { s.cols };
        let max = if rows { MAX_ROWS } else { MAX_COLS };
        if count == 0
            || at > size
            || delete && (at >= size || count >= size || at + count > size)
            || !delete && size.checked_add(count).is_none_or(|n| n > max)
        {
            return Err(Error::Bounds);
        }
        for table in &mut s.tables {
            table.range = shift_range(table.range, rows, at, count, delete)
                .ok_or(Error::Invalid("Deleting a whole table is not supported"))?;
        }
        s.merges = s
            .merges
            .iter()
            .filter_map(|range| shift_range(*range, rows, at, count, delete))
            .filter(|range| range.top != range.bottom || range.left != range.right)
            .collect();
        let old = std::mem::take(&mut s.cells);
        for ((r, c), mut cell) in old {
            let n = if rows { r } else { c };
            if delete && n >= at && n < at + count {
                continue;
            }
            let n = if delete && n >= at + count {
                n - count
            } else if !delete && n >= at {
                n + count
            } else {
                n
            };
            let (r, c) = if rows { (n, c) } else { (r, n) };
            let raw = cell.text.raw();
            if matches!(cell.text, CellContent::Formula(_)) {
                cell.text = CellContent::from_raw(adjust_formula_refs(
                    raw, &name, true, rows, at, count, delete,
                ));
            }
            s.cells.insert((r, c), cell);
        }
        let hidden = if rows {
            &mut s.view.hidden_rows
        } else {
            &mut s.view.hidden_columns
        };
        *hidden = hidden
            .iter()
            .filter_map(|&value| {
                if delete && value >= at && value < at + count {
                    None
                } else if delete && value >= at + count {
                    Some(value - count)
                } else if !delete && value >= at {
                    Some(value + count)
                } else {
                    Some(value)
                }
            })
            .collect();
        let frozen = if rows {
            &mut s.view.frozen_rows
        } else {
            &mut s.view.frozen_columns
        };
        if delete {
            *frozen = frozen.saturating_sub(count.min(frozen.saturating_sub(at)));
        } else if at < *frozen {
            *frozen = frozen.checked_add(count).ok_or(Error::Limit)?;
        }
        if rows {
            s.rows = if delete { size - count } else { size + count };
            shift_geometry(&mut s.row_heights, at, count, delete)
        } else {
            s.cols = if delete { size - count } else { size + count };
            shift_geometry(&mut s.col_widths, at, count, delete)
        }
        if !layout::valid_view(&st.sheet().view, st.sheet()) {
            return Err(Error::Invalid(
                "Structural edit conflicts with frozen or hidden view",
            ));
        }
        for sheet in st.sheets.iter_mut().filter(|sheet| sheet.id != active) {
            for cell in sheet.cells.values_mut() {
                if matches!(cell.text, CellContent::Formula(_)) {
                    cell.text = CellContent::from_raw(adjust_formula_refs(
                        cell.text.raw(),
                        &name,
                        false,
                        rows,
                        at,
                        count,
                        delete,
                    ));
                }
            }
        }
        st.selection = Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        };
        if self.has_external_source() {
            self.validate_external_rename(&st)?;
        }
        self.history.apply(st);
        Ok(())
    }
}
impl Workbook {
    /// Copy a rectangular region as a single undoable operation.
    pub fn copy_range(&mut self, source: Range, top: usize, left: usize) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let sheet = st.sheet();
        if sheet.protected {
            return Err(Error::Protected);
        }
        if !source.valid(sheet) {
            return Err(Error::Bounds);
        }
        let height = source.bottom - source.top + 1;
        let width = source.right - source.left + 1;
        if top.checked_add(height).is_none_or(|v| v > sheet.rows)
            || left.checked_add(width).is_none_or(|v| v > sheet.cols)
        {
            return Err(Error::Bounds);
        }
        let destination = Range {
            top,
            left,
            bottom: top + height - 1,
            right: left + width - 1,
        };
        if sheet
            .merges
            .iter()
            .any(|m| m.overlaps(source) || m.overlaps(destination))
        {
            return Err(Error::Invalid("copy involving merged cells is unsupported"));
        }
        let cells: Vec<_> = (0..height)
            .flat_map(|r| (0..width).map(move |c| (r, c)))
            .map(|(r, c)| {
                (
                    r,
                    c,
                    sheet.cells.get(&(source.top + r, source.left + c)).cloned(),
                )
            })
            .collect();
        let sheet = st.sheet_mut();
        for (r, c, cell) in cells {
            if let Some(mut cell) = cell {
                if matches!(cell.text, CellContent::Formula(_)) {
                    cell.text = CellContent::from_raw(office::translate_shared_formula(
                        cell.text.raw(),
                        top as isize - source.top as isize,
                        left as isize - source.left as isize,
                    ));
                }
                sheet.cells.insert((top + r, left + c), cell);
            } else {
                sheet.cells.remove(&(top + r, left + c));
            }
        }
        if st.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        st.selection = destination;
        rules::validate_range(&st, destination)?;
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn row_height(&self, row: usize) -> Option<u16> {
        let s = self.history.current().sheet();
        if row >= s.rows {
            None
        } else {
            s.row_heights.get(&row).copied()
        }
    }
    pub fn column_width(&self, col: usize) -> Option<u16> {
        let s = self.history.current().sheet();
        if col >= s.cols {
            None
        } else {
            s.col_widths.get(&col).copied()
        }
    }
    pub fn set_row_height(&mut self, row: usize, height: u16) -> Result<bool, Error> {
        self.require_editable()?;
        if self.has_external_source() && self.external_format.as_deref() != Some("xlsx") {
            return Err(Error::Invalid(
                "Resizing this external Office format is not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if row >= s.rows || height == 0 {
            return Err(Error::Bounds);
        }
        s.row_heights.insert(row, height);
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    pub fn set_column_width(&mut self, col: usize, width: u16) -> Result<bool, Error> {
        self.require_editable()?;
        if self.has_external_source() && self.external_format.as_deref() != Some("xlsx") {
            return Err(Error::Invalid(
                "Resizing this external Office format is not yet supported",
            ));
        }
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if col >= s.cols || width == 0 {
            return Err(Error::Bounds);
        }
        if width > 1785 {
            return Err(Error::Invalid("Column width exceeds Excel's limit"));
        }
        s.col_widths.insert(col, width);
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CellValue {
    Blank,
    Number(f64),
    Boolean(bool),
    Text(String),
    Formula(String),
}
impl Workbook {
    pub fn cell_value(&self, row: usize, col: usize) -> Option<CellValue> {
        self.cell(row, col)?;
        Some(match self.content(row, col) {
            None => CellValue::Blank,
            Some(CellContent::Number(t)) => CellValue::Number(t.raw().parse().ok()?),
            Some(CellContent::Boolean(t)) => {
                CellValue::Boolean(t.raw().eq_ignore_ascii_case("true"))
            }
            Some(CellContent::Formula(t)) => CellValue::Formula(t.raw().into()),
            Some(c) => CellValue::Text(c.raw().into()),
        })
    }
    /// Repeat a source rectangle over a destination, translating relative formulas per tile.
    pub fn fill_range(&mut self, source: Range, destination: Range) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let sheet = st.sheet();
        if sheet.protected {
            return Err(Error::Protected);
        }
        if !source.valid(sheet) || !destination.valid(sheet) {
            return Err(Error::Bounds);
        }
        if sheet
            .merges
            .iter()
            .any(|m| m.overlaps(source) || m.overlaps(destination))
        {
            return Err(Error::Invalid("fill involving merged cells is unsupported"));
        }
        let height = source.bottom - source.top + 1;
        let width = source.right - source.left + 1;
        let template: Vec<_> = (source.top..=source.bottom)
            .flat_map(|r| (source.left..=source.right).map(move |c| (r, c)))
            .map(|key| sheet.cells.get(&key).cloned())
            .collect();
        let sheet = st.sheet_mut();
        for r in destination.top..=destination.bottom {
            for c in destination.left..=destination.right {
                let sr = source.top + (r - destination.top) % height;
                let sc = source.left + (c - destination.left) % width;
                let cell = template[((sr - source.top) * width) + (sc - source.left)].clone();
                if let Some(mut cell) = cell {
                    if matches!(cell.text, CellContent::Formula(_)) {
                        cell.text = CellContent::from_raw(translate_refs(
                            cell.text.raw(),
                            r as isize - sr as isize,
                            c as isize - sc as isize,
                        ));
                    }
                    sheet.cells.insert((r, c), cell);
                } else {
                    sheet.cells.remove(&(r, c));
                }
            }
        }
        if st.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        st.selection = destination;
        rules::validate_range(&st, destination)?;
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    /// Stable sort of complete rows within a rectangular region using visible values.
    pub fn sort_range(
        &mut self,
        range: Range,
        key_column: usize,
        descending: bool,
    ) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let s = st.sheet();
        if s.protected {
            return Err(Error::Protected);
        }
        if !range.valid(s) || key_column < range.left || key_column > range.right {
            return Err(Error::Bounds);
        }
        if s.merges.iter().any(|m| m.overlaps(range)) {
            return Err(Error::Invalid("sort involving merged cells is unsupported"));
        }
        let display = self.display_grid();
        let mut rows: Vec<usize> = (range.top..=range.bottom).collect();
        rows.sort_by(|a, b| {
            let a = &display[*a][key_column];
            let b = &display[*b][key_column];
            let order = match (a.parse::<f64>(), b.parse::<f64>()) {
                (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => a.total_cmp(&b),
                _ => a.to_lowercase().cmp(&b.to_lowercase()),
            };
            if descending {
                order.reverse()
            } else {
                order
            }
        });
        let snapshot: Vec<_> = rows
            .iter()
            .map(|r| {
                (range.left..=range.right)
                    .map(|c| s.cells.get(&(*r, c)).cloned())
                    .collect::<Vec<_>>()
            })
            .collect();
        let s = st.sheet_mut();
        for (offset, (old_row, cells)) in rows.into_iter().zip(snapshot).enumerate() {
            let dest = range.top + offset;
            for (offset, cell) in cells.into_iter().enumerate() {
                let key = (dest, range.left + offset);
                if let Some(mut cell) = cell {
                    if matches!(cell.text, CellContent::Formula(_)) {
                        cell.text = CellContent::from_raw(translate_refs(
                            cell.text.raw(),
                            dest as isize - old_row as isize,
                            0,
                        ));
                    }
                    s.cells.insert(key, cell);
                } else {
                    s.cells.remove(&key);
                }
            }
        }
        rules::validate_range(&st, range)?;
        tables::reconcile_headers(self.history.current(), &mut st)?;
        self.validate_external_table_edit(&st)?;
        Ok(self.history.apply(st))
    }
    /// Return matching row indexes without modifying the sheet.
    pub fn matching_rows(
        &self,
        range: Range,
        column: usize,
        needle: &str,
    ) -> Result<Vec<usize>, Error> {
        let s = self.history.current().sheet();
        if !range.valid(s) || column < range.left || column > range.right {
            return Err(Error::Bounds);
        }
        let grid = self.display_grid();
        let needle = needle.to_lowercase();
        Ok((range.top..=range.bottom)
            .filter(|r| grid[*r][column].to_lowercase().contains(&needle))
            .collect())
    }
}

fn validate_name(st: &State, name: &str, except: Option<SheetId>) -> Result<(), Error> {
    if name.is_empty()
        || name.chars().count() > 31
        || name.chars().any(|c| "[]:*?/\\".contains(c))
        || st
            .sheets
            .iter()
            .any(|s| Some(s.id) != except && s.name.eq_ignore_ascii_case(name))
    {
        Err(Error::Invalid("invalid or duplicate sheet name"))
    } else {
        Ok(())
    }
}
fn has_3d_sheet_reference(input: &str) -> bool {
    input
        .split('!')
        .take(input.matches('!').count())
        .any(|prefix| {
            let name = prefix
                .rsplit(|c: char| {
                    matches!(c, '+' | '-' | '*' | '/' | '(' | ')' | ',' | '=' | '<' | '>')
                })
                .next()
                .unwrap_or("");
            name.contains(':')
        })
}
fn shift_range(
    mut range: Range,
    rows: bool,
    at: usize,
    count: usize,
    delete: bool,
) -> Option<Range> {
    let (lo, hi) = if rows {
        (range.top, range.bottom)
    } else {
        (range.left, range.right)
    };
    let (lo, hi) = if delete {
        let last = at + count - 1;
        if lo >= at && hi <= last {
            return None;
        }
        (
            if lo >= at && lo <= last {
                at
            } else if lo > last {
                lo - count
            } else {
                lo
            },
            if hi >= at && hi <= last {
                at.saturating_sub(1)
            } else if hi > last {
                hi - count
            } else {
                hi
            },
        )
    } else {
        (
            if lo >= at { lo + count } else { lo },
            if hi >= at { hi + count } else { hi },
        )
    };
    if rows {
        range.top = lo;
        range.bottom = hi;
    } else {
        range.left = lo;
        range.right = hi;
    }
    Some(range)
}
fn parse_a1_endpoint(input: &str, start: usize) -> Option<(usize, usize, usize, bool, bool)> {
    let b = input.as_bytes();
    let mut i = start;
    let abs_col = b.get(i) == Some(&b'$');
    if abs_col {
        i += 1;
    }
    let begin = i;
    let mut col = 0usize;
    while i < b.len() && b[i].is_ascii_alphabetic() {
        col = col
            .checked_mul(26)?
            .checked_add((b[i].to_ascii_uppercase() - b'A' + 1) as usize)?;
        i += 1;
    }
    if i == begin || i - begin > 3 {
        return None;
    }
    let abs_row = b.get(i) == Some(&b'$');
    if abs_row {
        i += 1;
    }
    let begin = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == begin
        || i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'!' | b'('))
    {
        return None;
    }
    let row = input[begin..i].parse::<usize>().ok()?;
    if row == 0 || col == 0 {
        return None;
    }
    Some((i, row, col, abs_col, abs_row))
}
fn write_a1_endpoint(out: &mut String, row: usize, col: usize, abs_col: bool, abs_row: bool) {
    if abs_col {
        out.push('$');
    }
    let mut n = col;
    let mut letters = Vec::new();
    while n > 0 {
        n -= 1;
        letters.push((b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    out.extend(letters.iter().rev());
    if abs_row {
        out.push('$');
    }
    out.push_str(&row.to_string());
}
fn parse_axis_range(input: &str, start: usize) -> Option<(usize, usize, usize, bool, bool, bool)> {
    fn part(input: &str, start: usize) -> Option<(usize, usize, bool, bool)> {
        let b = input.as_bytes();
        let mut i = start;
        let absolute = b.get(i) == Some(&b'$');
        if absolute {
            i += 1;
        }
        let begin = i;
        let rows = b.get(i)?.is_ascii_digit();
        let mut value = 0usize;
        while i < b.len()
            && if rows {
                b[i].is_ascii_digit()
            } else {
                b[i].is_ascii_alphabetic()
            }
        {
            value = value
                .checked_mul(if rows { 10 } else { 26 })?
                .checked_add(if rows {
                    (b[i] - b'0') as usize
                } else {
                    (b[i].to_ascii_uppercase() - b'A' + 1) as usize
                })?;
            i += 1;
        }
        if i == begin || !rows && i - begin > 3 || value == 0 {
            return None;
        }
        Some((i, value - 1, absolute, rows))
    }
    let (i, first, abs1, rows) = part(input, start)?;
    if input.as_bytes().get(i) != Some(&b':') {
        return None;
    }
    let (end, last, abs2, row2) = part(input, i + 1)?;
    if rows != row2
        || input
            .as_bytes()
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return None;
    }
    Some((end, first, last, abs1, abs2, rows))
}
fn adjust_axis_range(
    reference: &str,
    rows: bool,
    at: usize,
    count: usize,
    delete: bool,
) -> Option<String> {
    let (_, first, last, abs1, abs2, axis_rows) = parse_axis_range(reference, 0)?;
    if axis_rows != rows {
        return Some(reference.to_owned());
    }
    let range = if rows {
        Range {
            top: first.min(last),
            bottom: first.max(last),
            left: 0,
            right: 0,
        }
    } else {
        Range {
            left: first.min(last),
            right: first.max(last),
            top: 0,
            bottom: 0,
        }
    };
    let Some(range) = shift_range(range, rows, at, count, delete) else {
        return Some("#REF!".into());
    };
    let (lo, hi) = if rows {
        (range.top, range.bottom)
    } else {
        (range.left, range.right)
    };
    let (first, last) = if first <= last { (lo, hi) } else { (hi, lo) };
    let part = |v: usize, absolute: bool| -> String {
        let mut result = String::new();
        if absolute {
            result.push('$');
        }
        if rows {
            result.push_str(&(v + 1).to_string());
        } else {
            let mut n = v + 1;
            let mut chars = Vec::new();
            while n > 0 {
                n -= 1;
                chars.push((b'A' + (n % 26) as u8) as char);
                n /= 26;
            }
            result.extend(chars.iter().rev());
        }
        result
    };
    Some(format!("{}:{}", part(first, abs1), part(last, abs2)))
}
fn adjust_formula_refs(
    input: &str,
    target: &str,
    local: bool,
    rows: bool,
    at: usize,
    count: usize,
    delete: bool,
) -> String {
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut text = String::new();
    let mut qualified = Vec::new();
    let mut in_string = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            text.push('"');
            i += 1;
            if in_string && bytes.get(i) == Some(&b'"') {
                text.push('"');
                i += 1;
            } else {
                in_string = !in_string;
            }
            continue;
        }
        if in_string {
            let ch = input[i..].chars().next().unwrap();
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        if bytes[i] == b'[' {
            let mut depth = 1usize;
            i += 1;
            while i < bytes.len() && depth > 0 {
                if bytes[i] == b'[' {
                    depth += 1;
                }
                if bytes[i] == b']' {
                    depth -= 1;
                }
                i += 1;
            }
            let marker = format!("\u{1}{}\u{2}", qualified.len());
            qualified.push(input[start..i].to_owned());
            text.push_str(&marker);
            continue;
        }
        let mut name = String::new();
        if bytes[i] == b'\'' {
            i += 1;
            while i < bytes.len() {
                let ch = input[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\'' {
                    if bytes.get(i) == Some(&b'\'') {
                        name.push('\'');
                        i += 1;
                    } else {
                        break;
                    }
                } else {
                    name.push(ch);
                }
            }
        } else if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len()
                && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'.'))
            {
                i += 1;
            }
            name.push_str(&input[start..i]);
        } else {
            let ch = input[i..].chars().next().unwrap();
            text.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if bytes.get(i) != Some(&b'!') {
            text.push_str(&input[start..i]);
            continue;
        }
        i += 1;
        let ref_start = i;
        let (mut end, axis) = if let Some((end, _, _, _, _)) = parse_a1_endpoint(input, i) {
            (end, false)
        } else if let Some((end, _, _, _, _, _)) = parse_axis_range(input, i) {
            (end, true)
        } else {
            text.push_str(&input[start..i]);
            continue;
        };
        if !axis && bytes.get(end) == Some(&b':') {
            if let Some((next, _, _, _, _)) = parse_a1_endpoint(input, end + 1) {
                end = next;
            }
        }
        let reference = &input[ref_start..end];
        let replacement = if name.eq_ignore_ascii_case(target) {
            if axis {
                adjust_axis_range(reference, rows, at, count, delete)
                    .unwrap_or_else(|| reference.to_owned())
            } else {
                adjust_refs(reference, rows, at, count, delete)
            }
        } else {
            reference.to_owned()
        };
        let marker = format!("\u{1}{}\u{2}", qualified.len());
        qualified.push(format!("{}{}", &input[start..ref_start], replacement));
        text.push_str(&marker);
        i = end;
    }
    if local {
        text = adjust_refs(&text, rows, at, count, delete);
    }
    for (index, value) in qualified.into_iter().enumerate() {
        text = text.replace(&format!("\u{1}{index}\u{2}"), &value);
    }
    text
}
// Shift unqualified A1 references, including absolute references, without altering quoted text.
fn adjust_refs(input: &str, rows: bool, at: usize, count: usize, delete: bool) -> String {
    let b = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut quoted = false;
    while i < b.len() {
        if b[i] == b'"' {
            out.push('"');
            i += 1;
            if quoted && i < b.len() && b[i] == b'"' {
                out.push('"');
                i += 1
            } else {
                quoted = !quoted
            }
            continue;
        }
        if quoted || !b[i].is_ascii() {
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        let boundary = i == 0
            || (!b[i - 1].is_ascii_alphanumeric()
                && b[i - 1] != b'_'
                && b[i - 1] != b'!'
                && b[i - 1] != b'.');
        if boundary {
            let mut j = i;
            let abs_col = j < b.len() && b[j] == b'$';
            if abs_col {
                j += 1
            }
            let cs = j;
            while j < b.len() && b[j].is_ascii_alphabetic() {
                j += 1
            }
            let ce = j;
            let abs_row = j < b.len() && b[j] == b'$';
            if abs_row {
                j += 1
            }
            let rs = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1
            }
            if ce > cs
                && ce - cs <= 3
                && j > rs
                && (j == b.len()
                    || (!b[j].is_ascii_alphanumeric()
                        && b[j] != b'_'
                        && b[j] != b'!'
                        && b[j] != b'('))
            {
                let mut col = 0usize;
                for ch in &b[cs..ce] {
                    col = col * 26 + (ch.to_ascii_uppercase() - b'A' + 1) as usize
                }
                let row = input[rs..j].parse::<usize>().unwrap_or(0);
                if row > 0 && col > 0 {
                    if j < b.len() && b[j] == b':' {
                        if let Some((end, row2, col2, abs_col2, abs_row2)) =
                            parse_a1_endpoint(input, j + 1)
                        {
                            let a = if rows { row - 1 } else { col - 1 };
                            let z = if rows { row2 - 1 } else { col2 - 1 };
                            let lo = a.min(z);
                            let hi = a.max(z);
                            let shifted = if delete {
                                let last = at + count - 1;
                                if lo >= at && hi <= last {
                                    None
                                } else {
                                    let new_lo = if lo >= at && lo <= last {
                                        at
                                    } else if lo > last {
                                        lo - count
                                    } else {
                                        lo
                                    };
                                    let new_hi = if hi >= at && hi <= last {
                                        at.saturating_sub(1)
                                    } else if hi > last {
                                        hi - count
                                    } else {
                                        hi
                                    };
                                    Some(if a <= z {
                                        (new_lo, new_hi)
                                    } else {
                                        (new_hi, new_lo)
                                    })
                                }
                            } else {
                                Some((
                                    if a >= at { a + count } else { a },
                                    if z >= at { z + count } else { z },
                                ))
                            };
                            if let Some((first, second)) = shifted {
                                let (r1, c1) = if rows {
                                    (first + 1, col)
                                } else {
                                    (row, first + 1)
                                };
                                let (r2, c2) = if rows {
                                    (second + 1, col2)
                                } else {
                                    (row2, second + 1)
                                };
                                write_a1_endpoint(&mut out, r1, c1, abs_col, abs_row);
                                out.push(':');
                                write_a1_endpoint(&mut out, r2, c2, abs_col2, abs_row2);
                            } else {
                                out.push_str("#REF!");
                            }
                            i = end;
                            continue;
                        }
                    }
                    let coord = if rows { row - 1 } else { col - 1 };
                    if delete && coord >= at && coord < at + count {
                        out.push_str("#REF!")
                    } else {
                        let adjusted = if delete && coord >= at + count {
                            coord - count
                        } else if !delete && coord >= at {
                            coord + count
                        } else {
                            coord
                        };
                        let (r, c) = if rows {
                            (adjusted + 1, col)
                        } else {
                            (row, adjusted + 1)
                        };
                        if abs_col {
                            out.push('$')
                        }
                        let mut n = c;
                        let mut letters = Vec::new();
                        while n > 0 {
                            n -= 1;
                            letters.push((b'A' + (n % 26) as u8) as char);
                            n /= 26
                        }
                        out.extend(letters.iter().rev());
                        if abs_row {
                            out.push('$')
                        }
                        out.push_str(&r.to_string());
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(b[start] as char);
        i += 1;
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn structural_range_references_shrink_on_deletion() {
        assert_eq!(
            adjust_refs("=SUM($A$1:B5)", true, 0, 2, true),
            "=SUM($A$1:B3)"
        );
        assert_eq!(adjust_refs("=SUM(A1:B5)", true, 3, 3, true), "=SUM(A1:B3)");
        assert_eq!(adjust_refs("=SUM(A2:B3)", true, 1, 2, true), "=SUM(#REF!)");
        assert_eq!(adjust_refs("=SUM(A1:B5)", true, 2, 1, false), "=SUM(A1:B6)");
        assert_eq!(
            adjust_formula_refs("Data!$1:$3,Data!$A:$B", "Data", false, true, 0, 1, false),
            "Data!$2:$4,Data!$A:$B"
        );
    }
    #[test]
    fn csv_edit_dimensions_history_roundtrip() {
        let dir = std::env::temp_dir().join(format!("xls42-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let input = dir.join("in.csv");
        let out = dir.join("out.csv");
        let _ = std::fs::remove_file(&out);
        std::fs::write(&input, "a,\"b,b\"\n\"line\nnext\",\"say \"\"hi\"\"\"\n").unwrap();
        let mut w = Workbook::open_csv(&input).unwrap();
        assert_eq!(w.cell(1, 0), Some("line\nnext"));
        assert_eq!(w.cell(1, 1), Some("say \"hi\""));
        w.add_column().unwrap();
        w.add_row().unwrap();
        w.set_cell(2, 2, "=SUM(A1:A2)".into()).unwrap();
        w.save_as_csv(&out).unwrap();
        assert!(!w.is_dirty());
        let r = Workbook::open_csv(&out).unwrap();
        assert_eq!((r.rows(), r.cols()), (3, 3));
        assert_eq!(r.cell(2, 2), Some("=SUM(A1:A2)"));
        assert!(w.undo());
        assert!(w.is_dirty());
        assert!(w.redo());
        assert!(!w.is_dirty());
        assert!(w.save_as_csv(&out).is_err());
        assert_eq!(
            Workbook::open_csv(&out).unwrap().cell(2, 2),
            Some("=SUM(A1:A2)")
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn rectangular_limits_and_bounds() {
        assert!(Workbook::new(0, 1).is_err());
        assert!(Workbook::new(1001, 1).is_err());
        let mut w = Workbook::new(2, 2).unwrap();
        assert!(matches!(w.set_cell(2, 0, "x".into()), Err(Error::Bounds)));
        assert!(!w.is_dirty());
        assert!(!w.set_cell(0, 0, String::new()).unwrap());
    }
    #[test]
    fn capacity_actions_match_refusal_and_preserve_state() {
        let mut rows = Workbook::new(MAX_ROWS - 1, 1).unwrap();
        rows.set_cell(0, 0, "row".into()).unwrap();
        assert!(rows.can_add_row());
        rows.add_row().unwrap();
        assert!(!rows.can_add_row());
        assert!(matches!(rows.add_row(), Err(Error::Limit)));
        assert_eq!(rows.rows(), MAX_ROWS);
        assert_eq!(rows.cell(0, 0), Some("row"));
        assert!(rows.undo());
        assert!(rows.can_add_row());
        assert!(rows.redo());
        assert!(!rows.can_add_row());
        let mut cols = Workbook::new(1, MAX_COLS - 1).unwrap();
        cols.set_cell(0, 0, "column".into()).unwrap();
        assert!(cols.can_add_column());
        cols.add_column().unwrap();
        assert!(!cols.can_add_column());
        assert!(matches!(cols.add_column(), Err(Error::Limit)));
        assert_eq!(cols.cols(), MAX_COLS);
        assert_eq!(cols.cell(0, 0), Some("column"));
        assert!(cols.undo());
        assert!(cols.can_add_column());
        assert!(cols.redo());
        assert!(!cols.can_add_column());
    }
    #[test]
    fn cell_policy_is_part_of_structural_history_and_branch() {
        let mut w = Workbook::new(1, 2).unwrap();
        w.set_cell(0, 0, "a\r\nb".into()).unwrap();
        w.set_cell(0, 1, "a\nb".into()).unwrap();
        w.set_cell_editor_text(0, 0, "ab".into()).unwrap();
        w.set_cell_editor_text(0, 1, "ab".into()).unwrap();
        w.add_column().unwrap();
        w.set_cell_editor_text(0, 0, "a\nb".into()).unwrap();
        w.set_cell_editor_text(0, 1, "a\nb".into()).unwrap();
        assert_eq!(w.cell(0, 0), Some("a\r\nb"));
        assert_eq!(w.cell(0, 1), Some("a\nb"));
        assert!(w.undo());
        assert!(w.undo());
        assert!(w.undo());
        assert_eq!(w.cols(), 2);
        assert_eq!(w.cell(0, 0), Some("ab"));
        assert!(w.redo());
        assert_eq!(w.cols(), 3);
        assert!(matches!(
            w.set_cell_editor_text(0, 0, "x".repeat(MAX_TEXT + 1)),
            Err(Error::Limit)
        ));
        assert!(w.can_redo());
        assert_eq!(w.cell(0, 0), Some("ab"));
        assert!(!w.set_cell_editor_text(0, 0, "ab".into()).unwrap());
        assert!(w.can_redo());
        w.set_cell_editor_text(0, 0, "x\ny".into()).unwrap();
        assert_eq!(w.cell(0, 0), Some("x\r\ny"));
        assert!(!w.can_redo());
    }
}

#[cfg(test)]
mod formula_regression {
    use super::*;
    #[test]
    fn formulas_recalculate_history_and_csv_preserves_raw() {
        let mut b = Workbook::new(2, 3).unwrap();
        b.set_cell(0, 0, "2".into()).unwrap();
        b.set_cell(1, 0, "3".into()).unwrap();
        b.set_cell(0, 1, "=SUM(A1:A2)*2".into()).unwrap();
        b.set_cell(1, 1, "=AVERAGE(A1:A2)+MIN(A1:A2)+MAX(A1:A2)".into())
            .unwrap();
        b.set_cell(0, 2, "=-(B1+1)/2".into()).unwrap();
        assert_eq!(b.display_grid()[0], ["2", "10", "-5.5"]);
        assert_eq!(b.display_grid()[1][1], "7.5");
        b.set_cell(0, 0, "4".into()).unwrap();
        assert_eq!(b.display_grid()[0][1], "14");
        b.undo();
        assert_eq!(b.display_grid()[0][1], "10");
        b.redo();
        assert_eq!(b.display_grid()[0][1], "14");
        let path = std::env::temp_dir().join(format!("xls42-formula-{}.csv", std::process::id()));
        let _ = std::fs::remove_file(&path);
        b.save_as_csv(&path).unwrap();
        let reopened = Workbook::open_csv(&path).unwrap();
        assert_eq!(reopened.cell(0, 1), Some("=SUM(A1:A2)*2"));
        assert_eq!(reopened.display_grid()[0][1], "14");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn errors_cycles_recover_and_limits_are_bounded() {
        let mut b = Workbook::new(2, 4).unwrap();
        for (c, value) in ["=B1", "=A1", "=1/0", "=Z99"].iter().enumerate() {
            b.set_cell(0, c, (*value).into()).unwrap();
        }
        assert_eq!(
            b.display_grid()[0],
            ["#CYCLE!", "#CYCLE!", "#DIV/0!", "#REF!"]
        );
        b.set_cell(0, 1, "5".into()).unwrap();
        assert_eq!(b.display_grid()[0][0], "5");
        b.set_cell(1, 0, "=1+".into()).unwrap();
        b.set_cell(1, 1, format!("={}1", "-".repeat(200))).unwrap();
        b.set_cell(1, 2, "text".into()).unwrap();
        b.set_cell(1, 3, "=C2+1".into()).unwrap();
        assert_eq!(
            b.display_grid()[1],
            ["#ERROR!", "#LIMIT!", "text", "#VALUE!"]
        );
    }
}

#[cfg(test)]
mod aggregate_range_regression {
    use super::*;
    #[test]
    fn mixed_empty_and_formula_error_ranges() {
        let mut b = Workbook::new(5, 5).unwrap();
        // A1:A5 contains a real zero, two numbers, plain text and a blank.
        for (r, value) in ["0", "6", "text", "", "-2"].iter().enumerate() {
            b.set_cell(r, 0, (*value).into()).unwrap();
        }
        for (r, f) in ["SUM", "AVERAGE", "MIN", "MAX"].iter().enumerate() {
            b.set_cell(r, 1, format!("={f}(A1:A5)")).unwrap();
            b.set_cell(r, 2, format!("={f}(D1:D2)")).unwrap();
            b.set_cell(r, 4, format!("={f}(D3:D4)")).unwrap();
        }
        b.set_cell(0, 3, "text".into()).unwrap();
        b.set_cell(2, 3, "=1/0".into()).unwrap();
        let grid = b.display_grid();
        assert_eq!(
            (0..4).map(|r| grid[r][1].as_str()).collect::<Vec<_>>(),
            ["4", "1.3333333333333333", "-2", "6"]
        );
        assert_eq!(
            (0..4).map(|r| grid[r][2].as_str()).collect::<Vec<_>>(),
            ["0", "#DIV/0!", "0", "0"]
        );
        assert_eq!(
            (0..4).map(|r| grid[r][4].as_str()).collect::<Vec<_>>(),
            ["#DIV/0!"; 4]
        );
        b.set_cell(2, 3, "=2-2".into()).unwrap();
        assert_eq!(b.display_grid()[1][4], "0");
    }
}
fn translate_refs(input: &str, dr: isize, dc: isize) -> String {
    let b = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut quoted = false;
    while i < b.len() {
        if b[i] == b'"' {
            out.push('"');
            i += 1;
            if quoted && i < b.len() && b[i] == b'"' {
                out.push('"');
                i += 1
            } else {
                quoted = !quoted
            }
            continue;
        }
        if quoted || !b[i].is_ascii() {
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        if i == 0
            || (!b[i - 1].is_ascii_alphanumeric()
                && b[i - 1] != b'_'
                && b[i - 1] != b'!'
                && b[i - 1] != b'.')
        {
            let mut j = i;
            let ac = b[j] == b'$';
            if ac {
                j += 1
            }
            let cs = j;
            while j < b.len() && b[j].is_ascii_alphabetic() {
                j += 1
            }
            let ce = j;
            let ar = j < b.len() && b[j] == b'$';
            if ar {
                j += 1
            }
            let rs = j;
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1
            }
            if ce > cs
                && ce - cs <= 3
                && j > rs
                && (j == b.len()
                    || (!b[j].is_ascii_alphanumeric()
                        && b[j] != b'_'
                        && b[j] != b'!'
                        && b[j] != b'('))
            {
                let mut col = 0isize;
                for c in &b[cs..ce] {
                    col = col * 26 + (c.to_ascii_uppercase() - b'A' + 1) as isize
                }
                let row = input[rs..j].parse::<isize>().unwrap_or(0);
                if row > 0 && col > 0 {
                    let r = row + if ar { 0 } else { dr };
                    col += if ac { 0 } else { dc };
                    if r < 1 || col < 1 {
                        out.push_str("#REF!")
                    } else {
                        if ac {
                            out.push('$')
                        }
                        let mut letters = Vec::new();
                        while col > 0 {
                            col -= 1;
                            letters.push((b'A' + (col % 26) as u8) as char);
                            col /= 26
                        }
                        out.extend(letters.iter().rev());
                        if ar {
                            out.push('$')
                        }
                        out.push_str(&r.to_string());
                    }
                    i = j;
                    continue;
                }
            }
        }
        out.push(b[i] as char);
        i += 1;
    }
    out
}

fn shift_geometry(map: &mut BTreeMap<usize, u16>, at: usize, count: usize, delete: bool) {
    let old = std::mem::take(map);
    for (key, value) in old {
        if delete && key >= at && key < at + count {
            continue;
        }
        let key = if delete && key >= at + count {
            key - count
        } else if !delete && key >= at {
            key + count
        } else {
            key
        };
        map.insert(key, value);
    }
}
#[cfg(test)]
mod workbook_regression {
    use super::*;
    #[test]
    fn sheets_ranges_and_history() {
        let mut b = Workbook::new(4, 4).unwrap();
        b.set_cell(0, 0, "2".into()).unwrap();
        b.set_cell(0, 1, "=$A1+A$1+$A$1".into()).unwrap();
        assert_eq!(b.display_grid()[0][1], "6");
        b.copy_range(
            Range {
                top: 0,
                left: 1,
                bottom: 0,
                right: 1,
            },
            1,
            2,
        )
        .unwrap();
        assert_eq!(b.cell(1, 2), Some("=$A2+B$1+$A$1"));
        b.undo();
        assert_eq!(b.cell(1, 2), Some(""));
        b.redo();
        b.insert_rows(0, 1).unwrap();
        assert_eq!(b.cell(1, 1), Some("=$A2+A$2+$A$2"));
        b.undo();
        assert_eq!(b.cell(0, 1), Some("=$A1+A$1+$A$1"));
        let id = b.add_sheet("Second").unwrap();
        assert_eq!(b.active_sheet(), id);
        b.set_cell(0, 0, "other".into()).unwrap();
        b.select_sheet(1).unwrap();
        assert_eq!(b.cell(0, 0), Some("2"));
        b.undo();
        assert_eq!(b.cell(0, 0), Some("other"));
        assert!(b.rename_sheet(id, "Sheet1").is_err());
    }
}
impl Workbook {
    /// Plain-text clipboard projection. Formulas are copied as their editor text.
    pub fn copy_tsv(&self, range: Range) -> Result<String, Error> {
        if !range.valid(self.history.current().sheet()) {
            return Err(Error::Bounds);
        }
        self.copy_tsv_mapped(
            &(range.top..=range.bottom).collect::<Vec<_>>(),
            &(range.left..=range.right).collect::<Vec<_>>(),
        )
    }
    pub fn copy_tsv_mapped(&self, rows: &[usize], cols: &[usize]) -> Result<String, Error> {
        if rows.is_empty()
            || cols.is_empty()
            || rows.len() > MAX_ROWS
            || cols.len() > MAX_COLS
            || rows.windows(2).any(|x| x[0] >= x[1])
            || cols.windows(2).any(|x| x[0] >= x[1])
            || rows.last().is_none_or(|r| *r >= self.rows())
            || cols.last().is_none_or(|c| *c >= self.cols())
        {
            return Err(Error::Bounds);
        }
        let mut writer = csv::WriterBuilder::new()
            .delimiter(b'\t')
            .from_writer(Vec::new());
        for &row in rows {
            writer.write_record(cols.iter().map(|&col| self.cell(row, col).unwrap()))?;
        }
        let bytes = writer.into_inner().map_err(|e| Error::Io(e.into_error()))?;
        if bytes.len() > MAX_INPUT {
            return Err(Error::Limit);
        }
        String::from_utf8(bytes).map_err(|_| Error::Invalid("invalid clipboard text"))
    }
    /// Paste a complete text rectangle as one undoable transaction.
    pub fn paste_tsv(&mut self, top: usize, left: usize, text: &str) -> Result<bool, Error> {
        if top >= self.rows() || left >= self.cols() {
            return Err(Error::Bounds);
        }
        self.paste_tsv_mapped(
            &(top..self.rows()).collect::<Vec<_>>(),
            &(left..self.cols()).collect::<Vec<_>>(),
            text,
        )
    }
    /// Paste into explicitly mapped physical rows and columns in one history transaction.
    pub fn paste_tsv_mapped(
        &mut self,
        rows: &[usize],
        cols: &[usize],
        text: &str,
    ) -> Result<bool, Error> {
        self.paste_tsv_mapped_as(rows, cols, text, CellContent::from_raw)
    }

    /// Fill the selected physical rows with literal serials in one undoable edit.
    pub fn fill_serials(
        &mut self,
        rows: &[usize],
        column: usize,
        prefix: &str,
        start: u64,
        step: u64,
        digits: usize,
    ) -> Result<bool, Error> {
        if prefix.len() > 64
            || prefix.chars().any(char::is_control)
            || !(1..=20).contains(&digits)
            || step == 0
        {
            return Err(Error::Invalid(
                "Use a short prefix, a positive step and 1–20 digits",
            ));
        }
        if rows.is_empty() || rows.len() > 5000 {
            return Err(Error::Invalid("Select 1–5,000 rows"));
        }
        let mut writer = csv::WriterBuilder::new()
            .delimiter(b'\t')
            .from_writer(Vec::new());
        for index in 0..rows.len() {
            let number = (index as u64)
                .checked_mul(step)
                .and_then(|n| start.checked_add(n))
                .ok_or(Error::Invalid(
                    "The serial sequence exceeds the supported number range",
                ))?;
            writer.write_record([format!("{prefix}{number:0digits$}")])?;
        }
        let bytes = writer
            .into_inner()
            .map_err(|_| Error::Invalid("Could not encode serials"))?;
        let tsv = std::str::from_utf8(&bytes).map_err(|_| Error::Invalid("Invalid serial text"))?;
        self.paste_tsv_mapped_as(rows, &[column], tsv, |value| {
            CellContent::Text(TextValue::from_raw(value))
        })
    }

    fn paste_tsv_mapped_as(
        &mut self,
        rows: &[usize],
        cols: &[usize],
        text: &str,
        parse_cell: impl Fn(String) -> CellContent,
    ) -> Result<bool, Error> {
        self.require_editable()?;
        if text.len() > MAX_INPUT {
            return Err(Error::Limit);
        }
        if rows.is_empty()
            || cols.is_empty()
            || rows.len() > MAX_ROWS
            || cols.len() > MAX_COLS
            || rows.windows(2).any(|x| x[0] >= x[1])
            || cols.windows(2).any(|x| x[0] >= x[1])
            || rows.last().is_none_or(|r| *r >= self.rows())
            || cols.last().is_none_or(|c| *c >= self.cols())
        {
            return Err(Error::Bounds);
        }
        let mut reader = csv::ReaderBuilder::new()
            .delimiter(b'\t')
            .has_headers(false)
            .flexible(true)
            .from_reader(text.as_bytes());
        let mut data = Vec::new();
        let mut width = 0;
        for row in reader.records() {
            let row = row?;
            width = width.max(row.len());
            data.push(row);
            if data.len() > MAX_ROWS || width > MAX_COLS {
                return Err(Error::Limit);
            }
        }
        if data.is_empty() || width == 0 {
            return Ok(false);
        }
        if data.len() > rows.len() || width > cols.len() {
            return Err(Error::Bounds);
        }
        let selected_rows = &rows[..data.len()];
        let selected_cols = &cols[..width];
        let mut state = self.history.current().clone();
        let sheet = state.sheet_mut();
        if sheet.protected {
            return Err(Error::Protected);
        }
        if sheet.merges.iter().any(|m| {
            selected_rows.iter().any(|r| *r >= m.top && *r <= m.bottom)
                && selected_cols.iter().any(|c| *c >= m.left && *c <= m.right)
        }) {
            return Err(Error::Invalid(
                "paste involving merged cells is unsupported",
            ));
        }
        for (r, row) in data.iter().enumerate() {
            for c in 0..width {
                let key = (selected_rows[r], selected_cols[c]);
                let text = row.get(c).unwrap_or("");
                let style = sheet.cells.get(&key).map_or(0, |cell| cell.style);
                if text.is_empty() && style == 0 {
                    sheet.cells.remove(&key);
                } else {
                    sheet.cells.insert(
                        key,
                        Cell {
                            text: parse_cell(text.to_string()),
                            style,
                        },
                    );
                }
            }
        }
        if state.bytes() > MAX_TEXT {
            return Err(Error::Limit);
        }
        let dest = Range {
            top: selected_rows[0],
            left: selected_cols[0],
            bottom: *selected_rows.last().unwrap(),
            right: *selected_cols.last().unwrap(),
        };
        state.selection = dest;
        rules::validate_mapped(&state, selected_rows, selected_cols)?;
        tables::reconcile_headers(self.history.current(), &mut state)?;
        self.validate_external_table_edit(&state)?;
        Ok(self.history.apply(state))
    }
}
#[cfg(test)]
mod more_regressions {
    use super::*;
    #[test]
    fn serial_fill_keeps_leading_zeroes_in_visible_rows_and_undoes_as_one_edit() {
        let mut book = Workbook::new(4, 2).unwrap();
        book.set_cell(1, 0, "hidden".into()).unwrap();
        book.fill_serials(&[0, 2, 3], 0, "", 7, 2, 4).unwrap();
        assert_eq!(book.display_grid()[0][0], "0007");
        assert_eq!(book.display_grid()[1][0], "hidden");
        assert_eq!(book.display_grid()[2][0], "0009");
        assert_eq!(book.display_grid()[3][0], "0011");
        assert!(book.undo());
        assert_eq!(book.cell(0, 0), Some(""));
        assert_eq!(book.cell(2, 0), Some(""));
        assert_eq!(book.cell(1, 0), Some("hidden"));
        assert!(book.fill_serials(&[0, 1], 0, "", u64::MAX, 1, 4).is_err());
        assert_eq!(book.cell(0, 0), Some(""));
    }

    #[test]
    fn mapped_paste_rejects_invalid_destination_atomically() {
        let mut book = Workbook::new(5, 4).unwrap();
        book.set_cell(0, 0, "original".into()).unwrap();
        book.add_validation(DataValidation {
            range: Range {
                top: 2,
                left: 2,
                bottom: 2,
                right: 2,
            },
            allow_blank: false,
            kind: ValidationKind::Number {
                minimum: 1.0,
                maximum: 9.0,
            },
        })
        .unwrap();
        assert!(book
            .paste_tsv_mapped(&[0, 2], &[0, 2], "changed\tok\nother\tbad")
            .is_err());
        assert_eq!(book.cell(0, 0), Some("original"));
        assert_eq!(book.cell(2, 0), Some(""));
        assert_eq!(book.cell(2, 2), Some(""));
        book.paste_tsv_mapped(&[0, 2], &[0, 2], "changed\tok\nother\t5")
            .unwrap();
        assert_eq!(book.cell(2, 2), Some("5"));
        assert!(book.undo());
        assert_eq!(book.cell(0, 0), Some("original"));
        assert_eq!(book.cell(2, 2), Some(""));
    }
    #[test]
    fn functions_arguments_and_clipboard_are_transactional() {
        let mut b = Workbook::new(6, 5).unwrap();
        b.set_cell(0, 0, "2".into()).unwrap();
        b.set_cell(1, 0, "3".into()).unwrap();
        b.set_cell(0, 1, "=SUM(A1:A2,4,ABS(-2))".into()).unwrap();
        b.set_cell(1, 1, "=ROUND(SQRT(2),2)".into()).unwrap();
        assert_eq!(b.display_grid()[0][1], "11");
        assert_eq!(b.display_grid()[1][1], "1.41");
        b.paste_tsv(3, 0, "a\tb\n\"line\nnext\"\t5\n").unwrap();
        assert_eq!(b.cell(4, 0), Some("line\nnext"));
        assert_eq!(b.cell(4, 1), Some("5"));
        assert!(b.undo());
        assert_eq!(b.cell(3, 0), Some(""));
        assert_eq!(b.cell(4, 1), Some(""));
        assert!(b.redo());
        let before = b.cell(4, 1).unwrap().to_owned();
        assert!(b.paste_tsv(5, 4, "x\ty\n").is_err());
        assert_eq!(b.cell(4, 1), Some(before.as_str()));
    }
}

fn rewrite_sheet_refs(input: &str, old: &str, new: &str) -> String {
    let b = input.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    let mut in_string = false;
    while i < b.len() {
        if b[i] == b'"' {
            out.push('"');
            i += 1;
            if in_string && i < b.len() && b[i] == b'"' {
                out.push('"');
                i += 1
            } else {
                in_string = !in_string
            }
            continue;
        }
        if in_string {
            let ch = input[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
            continue;
        }
        let start = i;
        if b[i] == b'\'' {
            i += 1;
            let mut name = String::new();
            let mut closed = false;
            while i < b.len() {
                let ch = input[i..].chars().next().unwrap();
                i += ch.len_utf8();
                if ch == '\'' {
                    if i < b.len() && b[i] == b'\'' {
                        name.push('\'');
                        i += 1
                    } else {
                        closed = true;
                        break;
                    }
                } else {
                    name.push(ch)
                }
            }
            if closed && i < b.len() && b[i] == b'!' && name.eq_ignore_ascii_case(old) {
                out.push('\'');
                out.push_str(&new.replace('\'', "''"));
                out.push_str("'!");
                i += 1
            } else {
                out.push_str(&input[start..i])
            }
            continue;
        }
        if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || matches!(b[i], b'_' | b'.')) {
                i += 1
            }
            if i < b.len() && b[i] == b'!' && input[start..i].eq_ignore_ascii_case(old) {
                out.push('\'');
                out.push_str(&new.replace('\'', "''"));
                out.push_str("'!");
                i += 1
            } else {
                out.push_str(&input[start..i])
            }
            continue;
        }
        let ch = input[i..].chars().next().unwrap();
        out.push(ch);
        i += ch.len_utf8()
    }
    out
}
#[cfg(test)]
mod cross_sheet_regression {
    use super::*;
    #[test]
    fn references_renames_cycles_and_lazy_if() {
        let mut b = Workbook::new(4, 4).unwrap();
        b.set_cell(0, 0, "7".into()).unwrap();
        let second = b.add_sheet("Other Sheet").unwrap();
        b.set_cell(0, 0, "='Sheet1'!$A1*2".into()).unwrap();
        b.set_cell(0, 1, "=SUM(Sheet1!A1:A1,1)".into()).unwrap();
        b.set_cell(0, 2, "=IF(Sheet1!A1>5,99,1/0)".into()).unwrap();
        b.set_cell(0, 3, "=IFERROR(1/0,8)".into()).unwrap();
        assert_eq!(b.display_grid()[0], ["14", "8", "99", "8"]);
        b.rename_sheet(1, "O'Brien").unwrap();
        assert_eq!(b.cell(0, 0), Some("='O''Brien'!$A1*2"));
        assert_eq!(b.display_grid()[0][0], "14");
        assert!(b.delete_sheet(1).is_err());
        b.select_sheet(1).unwrap();
        b.set_cell(0, 0, "='Other Sheet'!A1".into()).unwrap();
        b.select_sheet(second).unwrap();
        assert_eq!(b.display_grid()[0][0], "#CYCLE!");
        b.undo();
        b.undo();
        assert_eq!(b.cell(0, 0), Some("7"));
    }
}
#[cfg(test)]
mod text_formula_regression {
    use super::*;
    #[test]
    fn typed_values_string_functions_and_short_circuit() {
        let mut b = Workbook::new(2, 6).unwrap();
        for (col, formula) in [
            "=\"a\"&\"b\"",
            "=IF(\"abc\"=\"ABC\",UPPER(\"yes\"),1/0)",
            "=LEFT(\"hello\",2)&MID(\"world\",2,2)",
            "=AND(FALSE,1/0)",
            "=OR(TRUE,1/0)",
            "=LEN(\"hé\")",
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(0, col, (*formula).into()).unwrap();
        }
        assert_eq!(
            b.display_grid()[0],
            ["ab", "YES", "heor", "FALSE", "TRUE", "2"]
        );
    }
}
#[cfg(test)]
mod lookup_formula_regression {
    use super::*;
    #[test]
    fn conditional_aggregation_and_lookup() {
        let mut b = Workbook::new(5, 7).unwrap();
        for (r, (name, num)) in [("apple", "2"), ("pear", "5"), ("apricot", "8")]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*name).into()).unwrap();
            b.set_cell(r, 1, (*num).into()).unwrap();
        }
        for (c, f) in [
            "=COUNTIF(A1:A3,\"ap*\")",
            "=SUMIF(A1:A3,\"ap*\",B1:B3)",
            "=INDEX(B1:B3,2)",
            "=MATCH(\"pear\",A1:A3,0)",
            "=VLOOKUP(\"pear\",A1:B3,2,FALSE)",
        ]
        .iter()
        .enumerate()
        {
            b.set_cell(0, c + 2, (*f).into()).unwrap();
        }
        assert_eq!(&b.display_grid()[0][2..], ["2", "10", "5", "2", "5"]);
    }
}
#[cfg(test)]
mod display_regression {
    use super::*;
    #[test]
    fn dates_and_number_formats_are_visible() {
        let mut b = Workbook::new(2, 3).unwrap();
        b.set_cell(0, 0, "1234.5".into()).unwrap();
        b.set_cell(0, 1, "=DATE(2024,1,2)".into()).unwrap();
        b.set_cell(0, 2, "=YEAR(B1)".into()).unwrap();
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            CellStyle {
                number_format: "#,##0.00".into(),
                ..Default::default()
            },
        )
        .unwrap();
        b.set_style(
            Range {
                top: 0,
                left: 1,
                bottom: 0,
                right: 1,
            },
            CellStyle {
                number_format: "yyyy-mm-dd".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(b.display_grid()[0], ["1,234.50", "2024-01-02", "2024"]);
    }
}
