//! Persisted charts and materialized pivot summaries.
use super::*;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartKind {
    Bar,
    Line,
    Pie,
    Scatter,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub sheet: SheetId,
    pub values: Range,
    pub categories: Option<Range>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Chart {
    pub id: u64,
    pub title: String,
    pub kind: ChartKind,
    pub series: Vec<ChartSeries>,
    pub anchor_row: usize,
    pub anchor_col: usize,
}
#[derive(Clone, Debug, PartialEq)]
pub struct ChartData {
    pub title: String,
    pub kind: ChartKind,
    pub series: Vec<(String, Vec<(String, Option<f64>)>)>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PivotAggregate {
    Sum,
    Count,
    Average,
    Min,
    Max,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pivot {
    pub id: u64,
    pub name: String,
    pub source_sheet: SheetId,
    pub source: Range,
    pub group_column: usize,
    pub value_column: usize,
    pub aggregate: PivotAggregate,
    pub destination_sheet: SheetId,
    pub top: usize,
    pub left: usize,
    pub output: Vec<(usize, usize, String)>,
}
fn col_range(r: Range) -> bool {
    r.left == r.right && r.top <= r.bottom
}
pub(super) fn valid_chart(state: &State, owner: &Sheet, chart: &Chart) -> bool {
    chart.id > 0
        && chart.title.len() <= 1024
        && chart.anchor_row < owner.rows
        && chart.anchor_col < owner.cols
        && !chart.series.is_empty()
        && chart.series.len() <= 32
        && (chart.kind != ChartKind::Pie || chart.series.len() == 1)
        && chart.series.iter().all(|series| {
            let Some(sheet) = state.sheets.iter().find(|s| s.id == series.sheet) else {
                return false;
            };
            series.name.len() <= 1024
                && (chart.kind != ChartKind::Scatter || series.categories.is_some())
                && (chart.kind != ChartKind::Pie
                    || series.values.bottom.saturating_sub(series.values.top) < 128)
                && series.values.valid(sheet)
                && col_range(series.values)
                && series.categories.is_none_or(|c| {
                    c.valid(sheet)
                        && col_range(c)
                        && (c.bottom - c.top) == (series.values.bottom - series.values.top)
                })
        })
}
pub(super) fn valid_pivot(state: &State, p: &Pivot) -> bool {
    let Some(source) = state.sheets.iter().find(|s| s.id == p.source_sheet) else {
        return false;
    };
    let Some(dest) = state.sheets.iter().find(|s| s.id == p.destination_sheet) else {
        return false;
    };
    p.id > 0
        && p.name.len() <= 1024
        && p.source.valid(source)
        && p.source.top < p.source.bottom
        && p.group_column >= p.source.left
        && p.group_column <= p.source.right
        && p.value_column >= p.source.left
        && p.value_column <= p.source.right
        && p.top < dest.rows
        && p.left.checked_add(1).is_some_and(|c| c < dest.cols)
        && p.output.len() <= MAX_ROWS * 2
        && p.output
            .iter()
            .map(|(r, c, _)| (*r, *c))
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == p.output.len()
        && p.output
            .iter()
            .all(|(r, c, v)| *r < dest.rows && *c < dest.cols && v.len() <= MAX_TEXT)
}
fn evaluation_book(state: &State, sheet: SheetId) -> Workbook {
    let mut s = state.clone();
    s.active = sheet;
    Workbook {
        history: SnapshotHistory::new(s, 0),
        snapshots: BTreeMap::new(),
        next_revision: 1,
        external_source: None,
        external_baseline: None,
        external_format: None,
        external_cache: BTreeMap::new(),
    }
}
impl Workbook {
    pub fn charts(&self) -> &[Chart] {
        &self.history.current().sheet().charts
    }
    pub fn add_chart(&mut self, mut chart: Chart) -> Result<u64, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        if st.sheet().charts.len() >= 128 {
            return Err(Error::Limit);
        }
        chart.id = st.next_id;
        if !valid_chart(&st, st.sheet(), &chart) {
            return Err(Error::Invalid("invalid chart source or type"));
        }
        st.next_id = st.next_id.checked_add(1).ok_or(Error::Limit)?;
        let id = chart.id;
        st.sheet_mut().charts.push(chart);
        self.history.apply(st);
        Ok(id)
    }
    pub fn update_chart(&mut self, chart: Chart) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        if !valid_chart(&st, st.sheet(), &chart) {
            return Err(Error::Invalid("invalid chart"));
        }
        let target = st
            .sheet_mut()
            .charts
            .iter_mut()
            .find(|c| c.id == chart.id)
            .ok_or(Error::Bounds)?;
        *target = chart;
        Ok(self.history.apply(st))
    }
    pub fn delete_chart(&mut self, id: u64) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        st.sheet_mut().charts.retain(|c| c.id != id);
        Ok(self.history.apply(st))
    }
    pub fn chart_data(&self, id: u64) -> Result<ChartData, Error> {
        let state = self.history.current();
        let chart = state
            .sheet()
            .charts
            .iter()
            .find(|c| c.id == id)
            .ok_or(Error::Bounds)?;
        let mut series = Vec::new();
        let mut cache = BTreeMap::new();
        for s in &chart.series {
            if let std::collections::btree_map::Entry::Vacant(e) = cache.entry(s.sheet) {
                e.insert(formula::display_grid(&evaluation_book(state, s.sheet)));
            }
            let grid = &cache[&s.sheet];
            let mut points = Vec::new();
            for (index, r) in (s.values.top..=s.values.bottom).enumerate() {
                let label = s
                    .categories
                    .map(|v| grid[v.top + index][v.left].clone())
                    .unwrap_or_else(|| (index + 1).to_string());
                let n = grid[r][s.values.left]
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite());
                points.push((label, n));
            }
            series.push((s.name.clone(), points));
        }
        Ok(ChartData {
            title: chart.title.clone(),
            kind: chart.kind,
            series,
        })
    }
    pub fn pivots(&self) -> &[Pivot] {
        &self.history.current().sheet().pivots
    }
    pub fn add_pivot(&mut self, mut pivot: Pivot) -> Result<u64, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        pivot.id = st.next_id;
        pivot.destination_sheet = st.active;
        pivot.output.clear();
        if !valid_pivot(&st, &pivot) {
            return Err(Error::Invalid("invalid pivot source or destination"));
        }
        if st.sheet().pivots.len() >= 128 {
            return Err(Error::Limit);
        }
        st.next_id = st.next_id.checked_add(1).ok_or(Error::Limit)?;
        let id = pivot.id;
        refresh(&mut st, &mut pivot)?;
        st.sheet_mut().pivots.push(pivot);
        self.history.apply(st);
        Ok(id)
    }
    pub fn refresh_pivot(&mut self, id: u64) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        let index = st
            .sheet()
            .pivots
            .iter()
            .position(|p| p.id == id)
            .ok_or(Error::Bounds)?;
        let mut pivot = st.sheet().pivots[index].clone();
        refresh(&mut st, &mut pivot)?;
        st.sheet_mut().pivots[index] = pivot;
        Ok(self.history.apply(st))
    }
    pub fn delete_pivot(&mut self, id: u64) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        if st.sheet().protected {
            return Err(Error::Protected);
        }
        let index = st
            .sheet()
            .pivots
            .iter()
            .position(|p| p.id == id)
            .ok_or(Error::Bounds)?;
        let p = st.sheet().pivots[index].clone();
        for (r, c, value) in &p.output {
            if st.sheet().cells.get(&(*r, *c)).map_or("", |v| v.text.raw()) != value
                || st
                    .sheet()
                    .cells
                    .get(&(*r, *c))
                    .is_some_and(|v| v.style != 0)
            {
                return Err(Error::Invalid(
                    "Pivot output has been edited; restore it before removal",
                ));
            }
        }
        for (r, c, _) in p.output {
            st.sheet_mut().cells.remove(&(r, c));
        }
        st.sheet_mut().pivots.remove(index);
        Ok(self.history.apply(st))
    }
}
fn refresh(state: &mut State, p: &mut Pivot) -> Result<(), Error> {
    if !valid_pivot(state, p) {
        return Err(Error::Invalid("invalid pivot"));
    }
    let source_grid = formula::display_grid(&evaluation_book(state, p.source_sheet));
    let mut groups: BTreeMap<String, (f64, usize, f64, f64)> = BTreeMap::new();
    for r in p.source.top + 1..=p.source.bottom {
        let key = source_grid[r][p.group_column].clone();
        if key.is_empty() {
            continue;
        }
        let value = source_grid[r][p.value_column]
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite());
        let entry = groups
            .entry(key)
            .or_insert((0.0, 0, f64::INFINITY, f64::NEG_INFINITY));
        if p.aggregate == PivotAggregate::Count {
            if !source_grid[r][p.value_column].is_empty() {
                entry.1 += 1
            }
        } else if let Some(n) = value {
            entry.0 += n;
            entry.1 += 1;
            entry.2 = entry.2.min(n);
            entry.3 = entry.3.max(n)
        }
    }
    let mut output = vec![
        (p.top, p.left, "Group".to_string()),
        (p.top, p.left + 1, format!("{:?}", p.aggregate)),
    ];
    for (i, (name, (sum, count, min, max))) in groups.into_iter().enumerate() {
        let r = p.top + i + 1;
        let value = match p.aggregate {
            PivotAggregate::Count => count as f64,
            PivotAggregate::Sum => sum,
            PivotAggregate::Average if count > 0 => sum / count as f64,
            PivotAggregate::Min if count > 0 => min,
            PivotAggregate::Max if count > 0 => max,
            _ => 0.0,
        };
        if !value.is_finite() {
            return Err(Error::Invalid("pivot result is not finite"));
        }
        output.push((r, p.left, name));
        output.push((r, p.left + 1, value.to_string()));
    }
    let dest = state
        .sheets
        .iter()
        .find(|s| s.id == p.destination_sheet)
        .ok_or(Error::Bounds)?;
    let oldkeys: std::collections::BTreeSet<_> =
        p.output.iter().map(|(r, c, _)| (*r, *c)).collect();
    for (r, c, value) in &p.output {
        if dest.cells.get(&(*r, *c)).map_or("", |v| v.text.raw()) != value
            || dest.cells.get(&(*r, *c)).is_some_and(|v| v.style != 0)
        {
            return Err(Error::Invalid(
                "Pivot output has been edited; restore it before refresh",
            ));
        }
    }
    for (r, c, _) in &output {
        if *r >= dest.rows || *c >= dest.cols {
            return Err(Error::Bounds);
        }
        if p.source_sheet == p.destination_sheet
            && *r >= p.source.top
            && *r <= p.source.bottom
            && *c >= p.source.left
            && *c <= p.source.right
        {
            return Err(Error::Invalid("Pivot output overlaps its source"));
        }
        if dest
            .merges
            .iter()
            .any(|m| *r >= m.top && *r <= m.bottom && *c >= m.left && *c <= m.right)
        {
            return Err(Error::Invalid("Pivot output overlaps a merge"));
        }
        if !oldkeys.contains(&(*r, *c))
            && dest
                .cells
                .get(&(*r, *c))
                .is_some_and(|c| !c.text.raw().is_empty() || c.style != 0)
        {
            return Err(Error::Invalid("Pivot output would overwrite cells"));
        }
    }
    let dest = state
        .sheets
        .iter_mut()
        .find(|s| s.id == p.destination_sheet)
        .unwrap();
    for (r, c, _) in &p.output {
        dest.cells.remove(&(*r, *c));
    }
    for (r, c, value) in &output {
        dest.cells.insert(
            (*r, *c),
            Cell {
                text: CellContent::from_raw(value.clone()),
                style: 0,
            },
        );
    }
    p.output = output;
    if state.bytes() > MAX_TEXT {
        return Err(Error::Limit);
    }
    let saved_active = state.active;
    state.active = p.destination_sheet;
    for (r, c, _) in &p.output {
        rules::validate_range(
            state,
            Range {
                top: *r,
                left: *c,
                bottom: *r,
                right: *c,
            },
        )?;
    }
    state.active = saved_active;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chart_and_pivot_refresh_history_project_and_xlsx() {
        let mut b = Workbook::new(12, 5).unwrap();
        for (r, (label, value)) in [("Group", "Value"), ("A", "2"), ("B", "3"), ("A", "4")]
            .iter()
            .enumerate()
        {
            b.set_cell(r, 0, (*label).into()).unwrap();
            b.set_cell(r, 1, (*value).into()).unwrap();
        }
        let chart = b
            .add_chart(Chart {
                id: 0,
                title: "Sales".into(),
                kind: ChartKind::Bar,
                series: vec![ChartSeries {
                    name: "Value".into(),
                    sheet: 1,
                    values: Range {
                        top: 1,
                        left: 1,
                        bottom: 3,
                        right: 1,
                    },
                    categories: Some(Range {
                        top: 1,
                        left: 0,
                        bottom: 3,
                        right: 0,
                    }),
                }],
                anchor_row: 0,
                anchor_col: 3,
            })
            .unwrap();
        assert_eq!(
            b.chart_data(chart).unwrap().series[0].1[2],
            ("A".into(), Some(4.0))
        );
        let id = b
            .add_pivot(Pivot {
                id: 0,
                name: "Summary".into(),
                source_sheet: 1,
                source: Range {
                    top: 0,
                    left: 0,
                    bottom: 3,
                    right: 1,
                },
                group_column: 0,
                value_column: 1,
                aggregate: PivotAggregate::Sum,
                destination_sheet: 1,
                top: 5,
                left: 0,
                output: vec![],
            })
            .unwrap();
        assert_eq!(b.cell(6, 1), Some("6"));
        b.set_cell(1, 1, "5".into()).unwrap();
        b.refresh_pivot(id).unwrap();
        assert_eq!(b.cell(6, 1), Some("9"));
        b.undo();
        assert_eq!(b.cell(6, 1), Some("6"));
        b.redo();
        assert_eq!(b.cell(6, 1), Some("9"));
        let project = b.project_bytes().unwrap();
        let restored = Workbook::from_project_bytes(&project).unwrap();
        assert_eq!(restored.charts().len(), 1);
        assert_eq!(restored.pivots().len(), 1);
        let xlsx = b.xlsx_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(xlsx.as_slice())).unwrap();
        assert!(zip.by_name("xl/charts/chart1.xml").is_ok());
        let reopened = Workbook::from_office_bytes(&xlsx, "xlsx").unwrap();
        assert_eq!(reopened.cell(6, 1), Some("9"));
        assert_eq!(reopened.pivots().len(), 1);
    }
}
