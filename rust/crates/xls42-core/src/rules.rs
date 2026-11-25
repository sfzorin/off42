//! Sheet validation and conditional formatting, stored in workbook history.
use super::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ValidationKind {
    Number { minimum: f64, maximum: f64 },
    List { values: Vec<String> },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DataValidation {
    pub range: Range,
    pub allow_blank: bool,
    pub kind: ValidationKind,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Condition {
    GreaterThan(f64),
    LessThan(f64),
    EqualTo(f64),
    TextContains(String),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ConditionalFormat {
    pub range: Range,
    pub condition: Condition,
    pub foreground: Option<u32>,
    pub background: Option<u32>,
    pub bold: bool,
}
fn contains(r: Range, row: usize, col: usize) -> bool {
    row >= r.top && row <= r.bottom && col >= r.left && col <= r.right
}
pub(super) fn valid_validation(v: &DataValidation, s: &Sheet) -> bool {
    v.range.valid(s)
        && match &v.kind {
            ValidationKind::Number { minimum, maximum } => {
                minimum.is_finite() && maximum.is_finite() && minimum <= maximum
            }
            ValidationKind::List { values } => {
                !values.is_empty()
                    && values.len() <= 1000
                    && values.iter().map(String::len).sum::<usize>() <= 16_384
            }
        }
}
pub(super) fn valid_condition(v: &ConditionalFormat, s: &Sheet) -> bool {
    v.range.valid(s)
        && match &v.condition {
            Condition::GreaterThan(n) | Condition::LessThan(n) | Condition::EqualTo(n) => {
                n.is_finite()
            }
            Condition::TextContains(s) => s.len() <= 4096,
        }
}
impl Workbook {
    pub fn validations(&self) -> &[DataValidation] {
        &self.history.current().sheet().validations
    }
    pub fn add_validation(&mut self, rule: DataValidation) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if !valid_validation(&rule, s) {
            return Err(Error::Invalid("invalid validation rule"));
        }
        if s.validations.len() >= 256 {
            return Err(Error::Limit);
        }
        if s.validations.iter().any(|v| v.range.overlaps(rule.range)) {
            return Err(Error::Invalid("validation rules may not overlap"));
        }
        s.validations.push(rule);
        Ok(self.history.apply(st))
    }
    pub fn remove_validation(&mut self, range: Range) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        s.validations.retain(|v| v.range != range);
        Ok(self.history.apply(st))
    }
    pub fn conditional_formats(&self) -> &[ConditionalFormat] {
        &self.history.current().sheet().conditional_formats
    }
    pub fn add_conditional_format(&mut self, rule: ConditionalFormat) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        if !valid_condition(&rule, s) {
            return Err(Error::Invalid("invalid conditional format"));
        }
        if s.conditional_formats.len() >= 256 {
            return Err(Error::Limit);
        }
        s.conditional_formats.push(rule);
        Ok(self.history.apply(st))
    }
    pub fn remove_conditional_formats(&mut self, range: Range) -> Result<bool, Error> {
        self.require_structural_edit()?;
        let mut st = self.history.current().clone();
        let s = st.sheet_mut();
        if s.protected {
            return Err(Error::Protected);
        }
        s.conditional_formats.retain(|v| v.range != range);
        Ok(self.history.apply(st))
    }
    pub fn effective_style(&self, row: usize, col: usize) -> Option<CellStyle> {
        let mut style = self.style(row, col)?.clone();
        let s = self.history.current().sheet();
        if let Some(table) = s
            .tables
            .iter()
            .find(|table| contains(table.range, row, col))
        {
            if row == table.range.top {
                style.bold = true;
                style.foreground.get_or_insert(0xffffff);
                style.background.get_or_insert(0x35618b);
            } else if table.total_row && row == table.range.bottom {
                style.bold = true;
                style.background.get_or_insert(0xdbe8f4);
            } else if (row - table.range.top) % 2 == 0 {
                style.background.get_or_insert(0xeaf2f8);
            }
        }
        let relevant: Vec<_> = s
            .conditional_formats
            .iter()
            .filter(|v| contains(v.range, row, col))
            .collect();
        if relevant.is_empty() {
            return Some(style);
        }
        let raw = self.cell(row, col)?;
        let value = if matches!(self.content(row, col), Some(CellContent::Formula(_))) {
            formula::display_cell(self, row, col)
        } else {
            raw.to_owned()
        };
        for rule in relevant {
            let matches = match &rule.condition {
                Condition::GreaterThan(n) => value.parse::<f64>().is_ok_and(|v| v > *n),
                Condition::LessThan(n) => value.parse::<f64>().is_ok_and(|v| v < *n),
                Condition::EqualTo(n) => value.parse::<f64>().is_ok_and(|v| v == *n),
                Condition::TextContains(t) => value.to_lowercase().contains(&t.to_lowercase()),
            };
            if matches {
                if let Some(c) = rule.foreground {
                    style.foreground = Some(c)
                }
                if let Some(c) = rule.background {
                    style.background = Some(c)
                }
                if rule.bold {
                    style.bold = true
                }
            }
        }
        Some(style)
    }
}
pub(super) fn validate_mapped(state: &State, rows: &[usize], cols: &[usize]) -> Result<(), Error> {
    let sheet = state.sheet();
    if sheet.validations.is_empty() {
        return Ok(());
    }
    let has_formula = rows.iter().any(|&row| {
        cols.iter().any(|&col| {
            sheet
                .cells
                .get(&(row, col))
                .is_some_and(|cell| matches!(cell.text, CellContent::Formula(_)))
                && sheet
                    .validations
                    .iter()
                    .any(|rule| contains(rule.range, row, col))
        })
    });
    let grid = if has_formula {
        let book = Workbook {
            history: SnapshotHistory::new(state.clone(), 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        };
        Some(formula::display_grid(&book))
    } else {
        None
    };
    for &row in rows {
        for &col in cols {
            for rule in sheet
                .validations
                .iter()
                .filter(|rule| contains(rule.range, row, col))
            {
                let cell = sheet.cells.get(&(row, col));
                let raw = cell.map_or("", |cell| cell.text.raw());
                if raw.is_empty() && rule.allow_blank {
                    continue;
                }
                let value = if matches!(cell.map(|cell| &cell.text), Some(CellContent::Formula(_)))
                {
                    grid.as_ref().unwrap()[row][col].as_str()
                } else {
                    raw
                };
                let valid = match &rule.kind {
                    ValidationKind::Number { minimum, maximum } => value
                        .parse::<f64>()
                        .is_ok_and(|v| v.is_finite() && v >= *minimum && v <= *maximum),
                    ValidationKind::List { values } => values.iter().any(|v| v == value),
                };
                if !valid {
                    return Err(Error::Invalid("Value does not satisfy cell validation"));
                }
            }
        }
    }
    Ok(())
}
pub(super) fn validate_range(state: &State, range: Range) -> Result<(), Error> {
    let sheet = state.sheet();
    let rules: Vec<_> = sheet
        .validations
        .iter()
        .filter(|v| v.range.overlaps(range))
        .collect();
    if rules.is_empty() {
        return Ok(());
    }
    let has_formula = sheet.cells.iter().any(|(&(r, c), cell)| {
        contains(range, r, c)
            && matches!(cell.text, CellContent::Formula(_))
            && rules.iter().any(|rule| contains(rule.range, r, c))
    });
    let grid = if has_formula {
        let book = Workbook {
            history: SnapshotHistory::new(state.clone(), 0),
            snapshots: BTreeMap::new(),
            next_revision: 1,
            external_source: None,
            external_baseline: None,
            external_format: None,
            external_cache: BTreeMap::new(),
        };
        Some(formula::display_grid(&book))
    } else {
        None
    };
    for rule in rules {
        let top = range.top.max(rule.range.top);
        let bottom = range.bottom.min(rule.range.bottom);
        let left = range.left.max(rule.range.left);
        let right = range.right.min(rule.range.right);
        for row in top..=bottom {
            for col in left..=right {
                let cell = sheet.cells.get(&(row, col));
                let raw = cell.map_or("", |c| c.text.raw());
                if raw.is_empty() && rule.allow_blank {
                    continue;
                }
                let value = if matches!(cell.map(|c| &c.text), Some(CellContent::Formula(_))) {
                    grid.as_ref().unwrap()[row][col].as_str()
                } else {
                    raw
                };
                let valid = match &rule.kind {
                    ValidationKind::Number { minimum, maximum } => value
                        .parse::<f64>()
                        .is_ok_and(|v| v.is_finite() && v >= *minimum && v <= *maximum),
                    ValidationKind::List { values } => values.iter().any(|s| s == value),
                };
                if !valid {
                    return Err(Error::Invalid("Value does not satisfy cell validation"));
                }
            }
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validation_conditional_history_and_xlsx_roundtrip() {
        let mut b = Workbook::new(4, 3).unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 1,
            right: 0,
        };
        b.add_validation(DataValidation {
            range,
            allow_blank: true,
            kind: ValidationKind::Number {
                minimum: 0.0,
                maximum: 10.0,
            },
        })
        .unwrap();
        b.add_conditional_format(ConditionalFormat {
            range,
            condition: Condition::GreaterThan(5.0),
            foreground: None,
            background: Some(0xff0000),
            bold: true,
        })
        .unwrap();
        b.set_cell(0, 0, "7".into()).unwrap();
        assert_eq!(b.effective_style(0, 0).unwrap().background, Some(0xff0000));
        assert!(b.set_cell(0, 0, "12".into()).is_err());
        assert_eq!(b.cell(0, 0), Some("7"));
        assert!(b.paste_tsv(0, 0, "2\n99\n").is_err());
        assert_eq!(b.cell(0, 0), Some("7"));
        b.set_cell(1, 0, "=2+3".into()).unwrap();
        b.undo();
        assert_eq!(b.cell(1, 0), Some(""));
        b.redo();
        assert_eq!(b.cell(1, 0), Some("=2+3"));
        let project = b.project_bytes().unwrap();
        let restored = Workbook::from_project_bytes(&project).unwrap();
        assert_eq!(restored.validations().len(), 1);
        assert_eq!(restored.conditional_formats().len(), 1);
        let xlsx = b.xlsx_bytes().unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(xlsx.as_slice())).unwrap();
        let mut content = String::new();
        zip.by_name("xl/worksheets/sheet1.xml")
            .unwrap()
            .read_to_string(&mut content)
            .unwrap();
        assert!(content.contains("dataValidation"));
        assert!(content.contains("conditionalFormatting"));
        let restored = Workbook::from_office_bytes(&xlsx, "xlsx").unwrap();
        assert_eq!(restored.validations().len(), 1);
        assert_eq!(restored.conditional_formats().len(), 1);
    }
}
