//! Sheet validation and conditional formatting, stored in workbook history.
use super::*;
use std::collections::BTreeSet;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ValidationKind {
    Number { minimum: f64, maximum: f64 },
    List { values: Vec<String> },
    ListRange { source: Range },
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
            ValidationKind::ListRange { source } => valid_list_source(*source, v.range),
        }
}

pub(super) fn valid_list_source(source: Range, target: Range) -> bool {
    source.top <= source.bottom
        && source.left <= source.right
        && source.bottom < MAX_ROWS
        && source.right < MAX_COLS
        && (source.top == source.bottom || source.left == source.right)
        && (source.bottom - source.top).max(source.right - source.left) < 1000
        && !source.overlaps(target)
}

impl ValidationKind {
    /// A fixed row or column on the same worksheet. Relative addresses entered
    /// in the dialog become absolute addresses in the XLSX validation.
    pub fn list_range(address: &str) -> Result<Self, Error> {
        let source = parse_list_range(address, false).ok_or(Error::Invalid(
            "Enter a same-sheet source such as $A$1:$A$10",
        ))?;
        Ok(Self::ListRange { source })
    }

    pub fn is_list(&self) -> bool {
        matches!(self, Self::List { .. } | Self::ListRange { .. })
    }
}

pub(super) fn parse_list_range(address: &str, require_absolute: bool) -> Option<Range> {
    let address = address.trim();
    let address = address.strip_prefix('=').unwrap_or(address);
    let (first, last) = address.split_once(':').unwrap_or((address, address));
    let parse = |endpoint: &str| {
        let (end, row, col, abs_col, abs_row) = parse_a1_endpoint(endpoint, 0)?;
        (end == endpoint.len()
            && (!require_absolute || (abs_col && abs_row))
            && row <= MAX_ROWS
            && col <= MAX_COLS)
            .then_some((row - 1, col - 1))
    };
    let (top, left) = parse(first)?;
    let (bottom, right) = parse(last)?;
    Some(Range {
        top,
        left,
        bottom,
        right,
    })
}

fn range_choices(
    sheet: &Sheet,
    source: Range,
    calculation: &formula::Calculation,
) -> Result<Vec<CellContent>, Error> {
    use formula::Value;
    let mut choices = Vec::new();
    let mut seen = BTreeSet::new();
    let mut bytes = 0;
    for row in source.top..=source.bottom {
        for col in source.left..=source.right {
            let at = (row, col);
            let value = match calculation.values.get(&at) {
                Some(Value::Text(text)) => CellContent::Text(TextValue::from_raw(text.clone())),
                Some(Value::Boolean(flag)) => CellContent::Boolean(TextValue::from_raw(if *flag {
                    "TRUE".into()
                } else {
                    "FALSE".into()
                })),
                Some(value @ Value::Number(number)) if number.is_finite() => {
                    CellContent::Number(TextValue::from_raw(value.clone().display()))
                }
                Some(_) => continue,
                None => match sheet.cells.get(&at).map(|cell| &cell.text) {
                    Some(CellContent::Formula(_) | CellContent::Error(_)) | None => continue,
                    Some(value) => value.clone(),
                },
            };
            if !value.raw().is_empty() && seen.insert(value.raw().to_owned()) {
                bytes += value.raw().len();
                if bytes > 16_384 {
                    return Err(Error::Invalid("The source list exceeds the size limit"));
                }
                choices.push(value);
            }
        }
    }
    Ok(choices)
}

fn current_choices(book: &Workbook, kind: &ValidationKind) -> Result<Vec<CellContent>, Error> {
    match kind {
        ValidationKind::List { values } => Ok(values
            .iter()
            .cloned()
            .map(|value| CellContent::Text(TextValue::from_raw(value)))
            .collect()),
        ValidationKind::ListRange { source } => {
            let calculation = formula::calculation_at(book, chrono::Local::now().naive_local());
            range_choices(book.history.current().sheet(), *source, &calculation)
        }
        ValidationKind::Number { .. } => Err(Error::Invalid("No list validation for this cell")),
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
    pub fn validation_at(&self, row: usize, col: usize) -> Option<&DataValidation> {
        self.validations()
            .iter()
            .find(|rule| contains(rule.range, row, col))
    }
    pub fn list_choice_values(&self, row: usize, col: usize) -> Result<Vec<String>, Error> {
        let rule = self
            .validation_at(row, col)
            .ok_or(Error::Invalid("No list validation for this cell"))?;
        let mut values: Vec<_> = current_choices(self, &rule.kind)?
            .into_iter()
            .map(|value| value.raw().to_owned())
            .collect();
        if rule.allow_blank && !values.iter().any(String::is_empty) {
            values.push(String::new());
        }
        Ok(values)
    }
    pub fn set_list_choice(
        &mut self,
        row: usize,
        col: usize,
        value: String,
    ) -> Result<bool, Error> {
        self.require_editable()?;
        if self.is_protected() {
            return Err(Error::Protected);
        }
        let rule = self
            .validation_at(row, col)
            .ok_or(Error::Invalid("No list validation for this cell"))?;
        let choice = if rule.kind.is_list() && rule.allow_blank && value.is_empty() {
            CellContent::default()
        } else {
            current_choices(self, &rule.kind)?
                .into_iter()
                .find(|item| item.raw() == value)
                .ok_or(Error::Invalid("This value is no longer available"))?
        };
        if self
            .content(row, col)
            .map_or(choice.raw().is_empty(), |current| *current == choice)
        {
            return Ok(false);
        }
        self.set_content(row, col, choice)
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
fn validation_calculation(state: &State) -> formula::Calculation {
    let book = Workbook {
        history: SnapshotHistory::new(state.clone(), 0),
        snapshots: BTreeMap::new(),
        next_revision: 1,
        external_source: None,
        external_baseline: None,
        external_format: None,
        external_cache: BTreeMap::new(),
    };
    formula::calculation_at(&book, chrono::Local::now().naive_local())
}

struct Validation<'a> {
    rule: &'a DataValidation,
    choices: BTreeSet<String>,
}

impl<'a> Validation<'a> {
    fn new(
        rule: &'a DataValidation,
        sheet: &Sheet,
        calculation: Option<&formula::Calculation>,
    ) -> Result<Self, Error> {
        let choices = match rule.kind {
            ValidationKind::ListRange { source } => range_choices(
                sheet,
                source,
                calculation.ok_or(Error::Invalid("Missing validation calculation"))?,
            )?
            .into_iter()
            .map(|value| value.raw().to_lowercase())
            .collect(),
            _ => BTreeSet::new(),
        };
        Ok(Self { rule, choices })
    }

    fn accepts(&self, value: &str) -> bool {
        if value.is_empty() && self.rule.allow_blank {
            return true;
        }
        match &self.rule.kind {
            ValidationKind::Number { minimum, maximum } => value
                .parse::<f64>()
                .is_ok_and(|v| v.is_finite() && v >= *minimum && v <= *maximum),
            ValidationKind::List { values } => values.iter().any(|candidate| candidate == value),
            ValidationKind::ListRange { .. } => self.choices.contains(&value.to_lowercase()),
        }
    }

    fn accepts_cell(
        &self,
        cell: Option<&Cell>,
        coord: (usize, usize),
        calculation: Option<&formula::Calculation>,
    ) -> bool {
        match cell.map(|cell| &cell.text) {
            Some(CellContent::Formula(_)) => {
                let value = calculation
                    .and_then(|grid| grid.values.get(&coord))
                    .cloned()
                    .map(formula::Value::display)
                    .unwrap_or_default();
                // A formula's empty result is not the same as an empty input.
                if value.is_empty() && self.rule.allow_blank {
                    return matches!(&self.rule.kind, ValidationKind::List { values } if values.iter().any(String::is_empty));
                }
                self.accepts(&value)
            }
            Some(content) => self.accepts(content.raw()),
            None => self.accepts(""),
        }
    }
}

const INVALID_VALUE: Error = Error::Invalid("Value does not satisfy cell validation");

pub(super) fn validate_mapped(state: &State, rows: &[usize], cols: &[usize]) -> Result<(), Error> {
    let sheet = state.sheet();
    let rules: Vec<_> = sheet
        .validations
        .iter()
        .filter(|rule| {
            rows.iter()
                .any(|row| (rule.range.top..=rule.range.bottom).contains(row))
                && cols
                    .iter()
                    .any(|col| (rule.range.left..=rule.range.right).contains(col))
        })
        .collect();
    if rules.is_empty() {
        return Ok(());
    }
    let needs_calculation = rules
        .iter()
        .any(|rule| matches!(rule.kind, ValidationKind::ListRange { .. }))
        || rows.iter().any(|&row| {
            cols.iter().any(|&col| {
                sheet
                    .cells
                    .get(&(row, col))
                    .is_some_and(|cell| matches!(cell.text, CellContent::Formula(_)))
                    && rules.iter().any(|rule| contains(rule.range, row, col))
            })
        });
    let calculation = needs_calculation.then(|| validation_calculation(state));
    for rule in rules {
        let validation = Validation::new(rule, sheet, calculation.as_ref())?;
        for &row in rows
            .iter()
            .filter(|row| (rule.range.top..=rule.range.bottom).contains(row))
        {
            for &col in cols
                .iter()
                .filter(|col| (rule.range.left..=rule.range.right).contains(col))
            {
                let coord = (row, col);
                if !validation.accepts_cell(sheet.cells.get(&coord), coord, calculation.as_ref()) {
                    return Err(INVALID_VALUE);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod list_choice_tests {
    use super::*;

    #[test]
    fn list_choices_use_literal_text_and_a_single_history_entry() {
        let mut book = Workbook::new(2, 2).unwrap();
        book.set_cell(0, 0, "Keep".into()).unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 0,
            right: 0,
        };
        book.add_validation(DataValidation {
            range,
            allow_blank: true,
            kind: ValidationKind::List {
                values: vec!["=40+2".into(), "007".into()],
            },
        })
        .unwrap();
        assert!(book.set_list_choice(0, 0, "=40+2".into()).unwrap());
        assert!(
            matches!(book.content(0, 0), Some(CellContent::Text(value)) if value.raw() == "=40+2")
        );
        assert!(!book.set_list_choice(0, 0, "=40+2".into()).unwrap());
        assert!(book.set_list_choice(0, 0, "other".into()).is_err());
        assert!(book.undo());
        assert_eq!(book.cell(0, 0), Some("Keep"));
        assert!(book.redo());
        assert_eq!(book.cell(0, 0), Some("=40+2"));
        assert!(book.set_list_choice(0, 0, String::new()).unwrap());
        assert_eq!(book.cell(0, 0), Some(""));
        assert!(book.set_list_choice(1, 0, "007".into()).is_err());
    }

    #[test]
    fn range_choices_recalculate_and_preserve_types_without_copying_formulas() {
        let mut book = Workbook::new(6, 3).unwrap();
        book.set_content(0, 1, CellContent::Text(TextValue::from_raw("007".into())))
            .unwrap();
        book.set_cell(1, 1, "=40+2".into()).unwrap();
        book.set_cell(2, 1, "Красный".into()).unwrap();
        book.set_cell(3, 1, "=1/0".into()).unwrap();
        let range = Range {
            top: 0,
            left: 0,
            bottom: 1,
            right: 0,
        };
        assert!(book
            .add_validation(DataValidation {
                range,
                allow_blank: false,
                kind: ValidationKind::list_range("A1:B2").unwrap(),
            })
            .is_err());
        assert!(book
            .add_validation(DataValidation {
                range,
                allow_blank: false,
                kind: ValidationKind::list_range("A1:A6").unwrap(),
            })
            .is_err());
        assert!(ValidationKind::list_range("Other!B1:B6").is_err());
        book.add_validation(DataValidation {
            range,
            allow_blank: false,
            kind: ValidationKind::list_range("B1:B6").unwrap(),
        })
        .unwrap();
        assert_eq!(
            book.list_choice_values(0, 0).unwrap(),
            ["007", "42", "Красный"]
        );
        assert!(book.set_list_choice(0, 0, "007".into()).unwrap());
        assert!(
            matches!(book.content(0, 0), Some(CellContent::Text(value)) if value.raw() == "007")
        );
        assert!(book.set_list_choice(1, 0, "42".into()).unwrap());
        assert!(
            matches!(book.content(1, 0), Some(CellContent::Number(value)) if value.raw() == "42")
        );
        assert!(book.undo());
        assert!(book.redo());
        book.set_cell(1, 1, "=40+3".into()).unwrap();
        let baseline = book.project_bytes().unwrap();
        assert!(book.set_list_choice(1, 0, "42".into()).is_err());
        assert!(book.set_cell(1, 0, "other".into()).is_err());
        assert!(book.paste_tsv(0, 0, "43\nother").is_err());
        assert_eq!(book.project_bytes().unwrap(), baseline);
        assert!(book.set_cell(1, 0, "КРАСНЫЙ".into()).unwrap());
        let reopened = Workbook::from_project_bytes(&book.project_bytes().unwrap()).unwrap();
        assert_eq!(
            reopened.list_choice_values(1, 0).unwrap(),
            ["007", "43", "Красный"]
        );
    }
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
    let needs_calculation = rules
        .iter()
        .any(|rule| matches!(rule.kind, ValidationKind::ListRange { .. }))
        || sheet.cells.iter().any(|(&(r, c), cell)| {
            contains(range, r, c)
                && matches!(cell.text, CellContent::Formula(_))
                && rules.iter().any(|rule| contains(rule.range, r, c))
        });
    let calculation = needs_calculation.then(|| validation_calculation(state));
    for rule in rules {
        let top = range.top.max(rule.range.top);
        let bottom = range.bottom.min(rule.range.bottom);
        let left = range.left.max(rule.range.left);
        let right = range.right.min(rule.range.right);
        let validation = Validation::new(rule, sheet, calculation.as_ref())?;
        let mut present = 0usize;
        for (&(row, col), cell) in sheet
            .cells
            .range((top, 0)..=(bottom, usize::MAX))
            .filter(|((_, c), _)| *c >= left && *c <= right)
        {
            present += 1;
            if !validation.accepts_cell(Some(cell), (row, col), calculation.as_ref()) {
                return Err(INVALID_VALUE);
            }
        }
        let area = (bottom - top + 1)
            .checked_mul(right - left + 1)
            .ok_or(Error::Limit)?;
        if present < area && !validation.accepts("") {
            return Err(INVALID_VALUE);
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
