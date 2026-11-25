//! Versioned native workbook representation. All model state is encoded explicitly.
use super::*;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Project {
    version: u32,
    active: SheetId,
    next_id: SheetId,
    selection: Range,
    #[serde(default)]
    date_1904: bool,
    styles: Vec<CellStyle>,
    sheets: Vec<ProjectSheet>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectSheet {
    id: SheetId,
    name: String,
    rows: usize,
    cols: usize,
    cells: Vec<ProjectCell>,
    merges: Vec<Range>,
    row_heights: BTreeMap<usize, u16>,
    col_widths: BTreeMap<usize, u16>,
    protected: bool,
    #[serde(default)]
    validations: Vec<DataValidation>,
    #[serde(default)]
    conditional_formats: Vec<ConditionalFormat>,
    #[serde(default)]
    charts: Vec<Chart>,
    #[serde(default)]
    pivots: Vec<Pivot>,
    #[serde(default)]
    tables: Vec<SpreadsheetTable>,
    #[serde(default)]
    view: SheetView,
    #[serde(default)]
    filter: Option<SheetFilter>,
    #[serde(default)]
    print: PrintSettings,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectCell {
    row: usize,
    col: usize,
    content: CellContent,
    style: usize,
}
impl Project {
    fn from_state(s: &State) -> Self {
        Self {
            version: 1,
            active: s.active,
            next_id: s.next_id,
            selection: s.selection,
            date_1904: s.date_1904,
            styles: s.styles.clone(),
            sheets: s
                .sheets
                .iter()
                .map(|sh| ProjectSheet {
                    id: sh.id,
                    name: sh.name.clone(),
                    rows: sh.rows,
                    cols: sh.cols,
                    cells: sh
                        .cells
                        .iter()
                        .map(|(&(row, col), v)| ProjectCell {
                            row,
                            col,
                            content: v.text.clone(),
                            style: v.style,
                        })
                        .collect(),
                    merges: sh.merges.clone(),
                    row_heights: sh.row_heights.clone(),
                    col_widths: sh.col_widths.clone(),
                    protected: sh.protected,
                    validations: sh.validations.clone(),
                    conditional_formats: sh.conditional_formats.clone(),
                    charts: sh.charts.clone(),
                    pivots: sh.pivots.clone(),
                    tables: sh.tables.clone(),
                    view: sh.view.clone(),
                    filter: sh.filter.clone(),
                    print: sh.print.clone(),
                })
                .collect(),
        }
    }
    fn into_state(self) -> Result<State, Error> {
        if self.version != 1
            || self.sheets.is_empty()
            || self.sheets.len() > MAX_SHEETS
            || self.styles.is_empty()
            || self.styles.len() > 100_000
        {
            return Err(Error::Invalid("unsupported or invalid project"));
        }
        let mut state = State {
            sheets: Vec::new(),
            active: self.active,
            next_id: self.next_id,
            selection: self.selection,
            styles: self.styles,
            date_1904: self.date_1904,
        };
        for sh in self.sheets {
            if sh.id == 0
                || sh.id >= state.next_id
                || state.sheets.iter().any(|s| s.id == sh.id)
                || sh.rows == 0
                || sh.rows > MAX_ROWS
                || sh.cols == 0
                || sh.cols > MAX_COLS
            {
                return Err(Error::Invalid("invalid sheet"));
            }
            validate_name(&state, &sh.name, None)?;
            let mut s = Sheet::new(sh.id, sh.name, sh.rows, sh.cols);
            s.protected = sh.protected;
            if sh.validations.len() > 256
                || sh.conditional_formats.len() > 256
                || sh
                    .validations
                    .iter()
                    .any(|v| !rules::valid_validation(v, &s))
                || sh
                    .conditional_formats
                    .iter()
                    .any(|v| !rules::valid_condition(v, &s))
            {
                return Err(Error::Invalid("invalid sheet rules"));
            }
            if sh.validations.iter().enumerate().any(|(i, v)| {
                sh.validations
                    .iter()
                    .skip(i + 1)
                    .any(|w| v.range.overlaps(w.range))
            }) {
                return Err(Error::Invalid("overlapping validations"));
            }
            s.validations = sh.validations;
            s.conditional_formats = sh.conditional_formats;
            s.charts = sh.charts;
            s.pivots = sh.pivots;
            s.tables = sh.tables;
            s.view = sh.view;
            if sh
                .filter
                .as_ref()
                .is_some_and(|f| !layout::valid_filter(f, &s))
                || !layout::valid_print(&sh.print, &s)
            {
                return Err(Error::Invalid("invalid sheet layout"));
            }
            s.filter = sh.filter;
            s.print = sh.print;
            for c in sh.cells {
                if c.row >= s.rows
                    || c.col >= s.cols
                    || c.style >= state.styles.len()
                    || !content_valid(&c.content)
                    || s.cells
                        .insert(
                            (c.row, c.col),
                            Cell {
                                text: c.content,
                                style: c.style,
                            },
                        )
                        .is_some()
                {
                    return Err(Error::Invalid("invalid cell"));
                }
            }
            for m in sh.merges {
                if !m.valid(&s) || s.merges.iter().any(|x| x.overlaps(m)) {
                    return Err(Error::Invalid("invalid merge"));
                }
                s.merges.push(m)
            }
            if !layout::valid_view(&s.view, &s) {
                return Err(Error::Invalid("Invalid sheet view"));
            }
            if sh.row_heights.iter().any(|(r, v)| *r >= s.rows || *v == 0)
                || sh.col_widths.iter().any(|(c, v)| *c >= s.cols || *v == 0)
            {
                return Err(Error::Invalid("invalid geometry"));
            }
            s.row_heights = sh.row_heights;
            s.col_widths = sh.col_widths;
            state.sheets.push(s);
        }
        tables::validate_tables(&state)?;
        let mut object_ids = std::collections::BTreeSet::new();
        for sheet in &state.sheets {
            if !object_ids.insert(sheet.id) {
                return Err(Error::Invalid("duplicate object ID"));
            }
            for id in sheet
                .charts
                .iter()
                .map(|c| c.id)
                .chain(sheet.pivots.iter().map(|p| p.id))
                .chain(sheet.tables.iter().map(|t| t.id))
            {
                if !object_ids.insert(id) {
                    return Err(Error::Invalid("duplicate object ID"));
                }
            }
        }
        if state.sheets.iter().any(|s| {
            s.charts.len() > 128
                || s.pivots.len() > 128
                || s.charts
                    .iter()
                    .any(|c| c.id >= state.next_id || !analytics::valid_chart(&state, s, c))
                || s.pivots.iter().any(|p| {
                    p.id >= state.next_id
                        || p.destination_sheet != s.id
                        || !analytics::valid_pivot(&state, p)
                })
        }) {
            return Err(Error::Invalid("invalid chart or pivot"));
        }
        if state.bytes() > MAX_TEXT
            || !state.sheets.iter().any(|s| s.id == state.active)
            || !state.selection.valid(state.sheet())
        {
            return Err(Error::Invalid("invalid workbook state"));
        }
        Ok(state)
    }
}
pub(super) fn content_valid(c: &CellContent) -> bool {
    let t = match c {
        CellContent::Text(v)
        | CellContent::Number(v)
        | CellContent::Boolean(v)
        | CellContent::Date(v)
        | CellContent::Formula(v)
        | CellContent::Error(v) => v,
    };
    t.policy_consistent()
        && match c {
            CellContent::Number(_) => t.raw().parse::<f64>().is_ok_and(f64::is_finite),
            CellContent::Boolean(_) => {
                t.raw().eq_ignore_ascii_case("true") || t.raw().eq_ignore_ascii_case("false")
            }
            CellContent::Formula(_) => t.raw().starts_with('='),
            _ => true,
        }
}
impl Workbook {
    pub fn capture_snapshot(&mut self, format: &str) -> Result<(u64, Vec<u8>), Error> {
        let bytes = match format {
            "xls42" => self.project_bytes()?,
            "xlsx" => self.xlsx_bytes()?,
            "ods" => self.ods_bytes()?,
            "csv" => self.csv_bytes()?,
            _ => return Err(Error::Invalid("unknown format")),
        };
        let revision = self.next_revision;
        self.next_revision = self.next_revision.checked_add(1).ok_or(Error::Limit)?;
        self.snapshots.clear();
        self.snapshots.insert(revision, self.history.snapshot());
        Ok((revision, bytes))
    }
    pub fn saved_snapshot(&mut self, revision: u64) {
        if let Some(state) = self.snapshots.remove(&revision) {
            self.history.mark_saved_snapshot(state)
        }
    }
    pub fn project_bytes(&self) -> Result<Vec<u8>, Error> {
        self.require_editable()?;
        self.require_no_external_conversion()?;
        let bytes = serde_json::to_vec(&Project::from_state(self.history.current()))
            .map_err(|_| Error::Invalid("cannot encode project"))?;
        if bytes.len() > 16 * MAX_INPUT {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub fn open_project(path: &Path) -> Result<Self, Error> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take((16 * MAX_INPUT + 1) as u64)
            .read_to_end(&mut bytes)?;
        Self::from_project_bytes(&bytes)
    }
    pub fn from_project_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > 16 * MAX_INPUT {
            return Err(Error::Limit);
        }
        let project: Project = serde_json::from_slice(bytes)
            .map_err(|_| Error::Invalid("invalid project encoding"))?;
        let state = project.into_state()?;
        Ok(Self {
            history: SnapshotHistory::new(state, 8),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        })
    }
    pub fn save_project(&mut self, path: &Path) -> Result<(), Error> {
        let bytes = self.project_bytes()?;
        write_new(path, &bytes)?;
        self.history.mark_saved();
        Ok(())
    }
    pub fn content(&self, row: usize, col: usize) -> Option<&CellContent> {
        let s = self.history.current().sheet();
        if row >= s.rows || col >= s.cols {
            return None;
        }
        s.cells.get(&(row, col)).map(|c| &c.text)
    }
    pub fn set_content(
        &mut self,
        row: usize,
        col: usize,
        content: CellContent,
    ) -> Result<bool, Error> {
        self.require_editable()?;
        if !content_valid(&content) {
            return Err(Error::Invalid("invalid cell content"));
        }
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
        let style = s.cells.get(&(row, col)).map_or(0, |c| c.style);
        if content.raw().is_empty() && style == 0 {
            s.cells.remove(&(row, col));
        } else {
            s.cells.insert(
                (row, col),
                Cell {
                    text: content,
                    style,
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
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_roundtrip_typed_state_and_snapshot() {
        let mut b = Workbook::new(4, 4).unwrap();
        b.set_content(0, 0, CellContent::Text(TextValue::from_raw("123".into())))
            .unwrap();
        b.set_content(0, 1, CellContent::Number(TextValue::from_raw("123".into())))
            .unwrap();
        b.set_content(
            0,
            2,
            CellContent::Formula(TextValue::from_raw("=B1*2".into())),
        )
        .unwrap();
        b.set_cell(1, 0, "x".into()).unwrap();
        b.set_row_height(1, 20).unwrap();
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            CellStyle {
                bold: true,
                ..Default::default()
            },
        )
        .unwrap();
        b.merge(Range {
            top: 2,
            left: 0,
            bottom: 2,
            right: 1,
        })
        .unwrap();
        let id = b.add_sheet("Other").unwrap();
        b.set_content(
            0,
            0,
            CellContent::Boolean(TextValue::from_raw("TRUE".into())),
        )
        .unwrap();
        let (revision, bytes) = b.capture_snapshot("xls42").unwrap();
        b.set_cell(1, 1, "later".into()).unwrap();
        b.saved_snapshot(revision);
        assert!(b.is_dirty());
        let mut c = Workbook::from_project_bytes(&bytes).unwrap();
        assert_eq!(c.active_sheet(), id);
        assert!(matches!(c.content(0, 0), Some(CellContent::Boolean(_))));
        c.select_sheet(1).unwrap();
        assert!(matches!(c.content(0, 0), Some(CellContent::Text(_))));
        assert_eq!(c.display_grid()[0][2], "246");
        assert_eq!(c.row_height(1), Some(20));
        assert!(c.style(0, 0).unwrap().bold);
        assert_eq!(c.merges().len(), 1);
        assert!(c.csv_bytes().is_err());
    }
}
