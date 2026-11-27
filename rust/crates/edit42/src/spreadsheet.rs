//! Workbook commands over xls42-core. Cells and ranges are in A1 notation ("B3", "A1:D20"); rows in
//! `insert_rows`/`delete_rows` are 1-based numbers and columns are letters, as a user would say them. Every command
//! may name a sheet by its 0-based index (`sheet`); without it the command acts on the active sheet.

use crate::{column_name, Result};
use serde::Deserialize;
use serde_json::{json, Value};
use xls42_core::{CellStyle, Range, SheetId, SortKey, SortOptions, Workbook, MAX_COLS, MAX_ROWS};

pub(crate) fn kind(name: &str) -> Result<String> {
    let kind = name.trim().trim_start_matches('.').to_ascii_lowercase();
    match kind.as_str() {
        "xlsx" | "ods" | "xls" | "csv" => Ok(kind),
        other => Err(format!("Unsupported spreadsheet format: {other}")),
    }
}

/// Parses "B3" into (row, column), 0-based.
pub fn cell_ref(text: &str) -> Result<(usize, usize)> {
    let t = text.trim().replace('$', "").to_ascii_uppercase();
    let split = t.find(|c: char| c.is_ascii_digit()).ok_or_else(|| format!("Invalid cell: {text}"))?;
    let (letters, digits) = t.split_at(split);
    if letters.is_empty() || letters.len() > 3 || !letters.bytes().all(|b| b.is_ascii_uppercase()) {
        return Err(format!("Invalid cell: {text}"));
    }
    let col = letters.bytes().fold(0usize, |n, b| n * 26 + usize::from(b - b'A' + 1)) - 1;
    let row = digits.parse::<usize>().map_err(|_| format!("Invalid cell: {text}"))?;
    if row == 0 || row > MAX_ROWS || col >= MAX_COLS {
        return Err(format!("Cell outside the sheet: {text}"));
    }
    Ok((row - 1, col))
}

/// Parses "A1:D20" (or a single cell) into a range.
pub fn range_ref(text: &str) -> Result<Range> {
    let (a, b) = text.split_once(':').unwrap_or((text, text));
    let (r0, c0) = cell_ref(a)?;
    let (r1, c1) = cell_ref(b)?;
    Ok(Range { top: r0.min(r1), left: c0.min(c1), bottom: r0.max(r1), right: c0.max(c1) })
}

fn column_ref(text: &str) -> Result<usize> {
    cell_ref(&format!("{}1", text.trim())).map(|(_, c)| c)
}

fn a1(row: usize, col: usize) -> String {
    format!("{}{}", column_name(col), row + 1)
}

fn range_text(r: Range) -> String {
    if r.top == r.bottom && r.left == r.right {
        a1(r.top, r.left)
    } else {
        format!("{}:{}", a1(r.top, r.left), a1(r.bottom, r.right))
    }
}

fn cells(r: Range) -> usize {
    (r.bottom - r.top + 1).saturating_mul(r.right - r.left + 1)
}

#[derive(Debug, Deserialize)]
#[serde(tag = "command", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    /// Values row by row from the top-left of `range`; a string starting with = is a formula, "" or null clears.
    /// One value over a larger range fills it (formulas adjusted per cell, like fill down).
    SetCells {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
        values: Vec<Vec<Value>>,
    },
    /// Clear the contents (not the formatting) of a range.
    Clear {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
    },
    InsertRows {
        #[serde(default)]
        sheet: Option<usize>,
        row: usize,
        count: usize,
    },
    DeleteRows {
        #[serde(default)]
        sheet: Option<usize>,
        row: usize,
        count: usize,
    },
    InsertColumns {
        #[serde(default)]
        sheet: Option<usize>,
        column: String,
        count: usize,
    },
    DeleteColumns {
        #[serde(default)]
        sheet: Option<usize>,
        column: String,
        count: usize,
    },
    /// An Excel number format code ("0.00", "#,##0", "0%", "\"€\"#,##0.00", "General" to reset).
    SetNumberFormat {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
        format: String,
    },
    /// Show one more (delta 1) or one fewer (delta -1) decimal place.
    AdjustDecimals {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
        delta: i32,
    },
    SetTextStyle {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
        #[serde(default)]
        bold: Option<bool>,
        #[serde(default)]
        italic: Option<bool>,
        #[serde(default)]
        underline: Option<bool>,
    },
    SortRange {
        #[serde(default)]
        sheet: Option<usize>,
        range: String,
        by_column: String,
        descending: bool,
        has_header: bool,
    },
    GetSheetInfo { sheet: Option<usize>, focus: Option<String> },
    AggregateCells { sheet: Option<usize>, range: String, function: String },
    FindCells { sheet: Option<usize>, text: String, range: Option<String>, after: Option<String> },
    ReadCells {
        sheet: Option<usize>,
        range: String,
        start: Option<usize>,
        length: Option<usize>,
    },
}

impl Command {
    fn sheet(&self) -> Option<usize> {
        match self {
            Self::SetCells { sheet, .. }
            | Self::Clear { sheet, .. }
            | Self::InsertRows { sheet, .. }
            | Self::DeleteRows { sheet, .. }
            | Self::InsertColumns { sheet, .. }
            | Self::DeleteColumns { sheet, .. }
            | Self::SetNumberFormat { sheet, .. }
            | Self::AdjustDecimals { sheet, .. }
            | Self::SetTextStyle { sheet, .. }
            | Self::SortRange { sheet, .. }
            | Self::AggregateCells { sheet, .. }
            | Self::FindCells { sheet, .. }
            | Self::GetSheetInfo { sheet, .. }
            | Self::ReadCells { sheet, .. } => *sheet,
        }
    }
    fn reads_only(&self) -> bool {
        matches!(self, Self::AggregateCells { .. } | Self::FindCells { .. } | Self::GetSheetInfo { .. } | Self::ReadCells { .. })
    }
}

pub struct SpreadsheetSession {
    pub(crate) book: Workbook,
}

fn failed(e: xls42_core::Error) -> String {
    e.to_string()
}

impl SpreadsheetSession {
    pub fn blank() -> Self {
        let mut book = Workbook::new(1, 1).expect("a 1×1 workbook");
        book.set_history_limit(crate::HISTORY_DEPTH);
        Self { book }
    }

    pub fn open(bytes: &[u8], from: &str) -> Result<Self> {
        let kind = kind(from)?;
        let mut book = if kind == "csv" { Workbook::from_csv_bytes(bytes) } else { Workbook::from_office_bytes(bytes, &kind) }
            .map_err(failed)?;
        book.set_history_limit(crate::HISTORY_DEPTH);
        Ok(Self { book })
    }

    pub fn workbook(&self) -> &Workbook {
        &self.book
    }

    pub(crate) fn undo(&mut self) -> bool {
        self.book.undo()
    }

    pub(crate) fn redo(&mut self) -> bool {
        self.book.redo()
    }

    fn sheet_id(&self, index: Option<usize>) -> Result<SheetId> {
        match index {
            None => Ok(self.book.active_sheet()),
            Some(i) => self.book.sheets().get(i).map(|s| s.0).ok_or_else(|| format!("No sheet {i}")),
        }
    }

    pub(crate) fn apply(&mut self, commands: Vec<Value>) -> Result<Vec<Value>> {
        let commands = commands
            .into_iter()
            .map(|c| serde_json::from_value::<Command>(c).map_err(|e| format!("Invalid command: {e}")))
            .collect::<Result<Vec<_>>>()?;
        if commands.iter().all(Command::reads_only) {
            return commands
                .iter()
                .map(|c| {
                    let id = self.sheet_id(c.sheet())?;
                    self.book.with_sheet(id, |w| read(w, c)).map_err(failed)?
                })
                .collect();
        }
        if self.book.is_read_only() {
            return Err("XLS workbooks are read-only: save the values as CSV or open an XLSX or ODS copy.".into());
        }
        let ids = commands.iter().map(|c| self.sheet_id(c.sheet())).collect::<Result<Vec<_>>>()?;
        let active = self.book.active_sheet();
        let mut error = None;
        let outcome = self.book.batch(|w| {
            let mut results = Vec::new();
            for (command, id) in commands.iter().zip(ids) {
                if w.active_sheet() != id {
                    w.select_sheet(id)?;
                }
                let result = if command.reads_only() { read(w, command) } else { write(w, command) };
                match result {
                    Ok(v) => results.push(v),
                    Err(e) => {
                        error = Some(e);
                        return Err(xls42_core::Error::Invalid(""));
                    }
                }
            }
            if w.active_sheet() != active {
                w.select_sheet(active)?;
            }
            Ok(results)
        });
        if let Some(e) = error {
            return Err(e);
        }
        outcome.map_err(failed)
    }

    /// One sheet for drawing: `{sheets: [{name}], active, sheet: {index, name, rows, cols, truncated,
    /// cells: [[row, col, text]], inputs: [[row, col, input]], styles: [[row, col, {bold, italic, underline,
    /// format}]]}}`. `inputs` lists cells whose input differs from the displayed text (formulas, formatted
    /// numbers); `styles` cells with non-default bold, italic, underline or number format.
    pub(crate) fn snapshot(&self, options: &Value) -> Result<Value> {
        let sheets = self.book.sheets();
        let active = sheets.iter().position(|s| s.0 == self.book.active_sheet()).unwrap_or(0);
        let index = options.get("sheet").and_then(Value::as_u64).map_or(active, |i| i as usize).min(sheets.len() - 1);
        let max_rows = options.get("maxRows").and_then(Value::as_u64).map_or(1000, |n| n as usize);
        let max_cols = options.get("maxCols").and_then(Value::as_u64).map_or(100, |n| n as usize);
        let (id, name) = sheets[index].clone();
        let sheet = self
            .book
            .with_sheet(id, |w| {
                let mut truncated = false;
                let display = w.display_cells();
                let mut cells = Vec::new();
                let mut inputs = Vec::new();
                let mut styles = Vec::new();
                for (&(row, col), text) in &display {
                    if row >= max_rows || col >= max_cols {
                        truncated = true;
                        continue;
                    }
                    cells.push(json!([row, col, text]));
                }
                for (sheet, row, col, input) in w.cell_inputs() {
                    if sheet != id || input.is_empty() {
                        continue;
                    }
                    if row >= max_rows || col >= max_cols {
                        truncated = true;
                        continue;
                    }
                    if display.get(&(row, col)).map(String::as_str) != Some(input) {
                        inputs.push(json!([row, col, input]));
                    }
                    if let Some(style) = w.style(row, col).filter(|s| **s != CellStyle::default()) {
                        styles.push(style_json(row, col, style));
                    }
                }
                // Formatted empty cells too (bold set before typing).
                for row in 0..w.rows().min(max_rows) {
                    for col in 0..w.cols().min(max_cols) {
                        if w.cell(row, col) == Some("") {
                            if let Some(style) = w.style(row, col).filter(|s| **s != CellStyle::default()) {
                                styles.push(style_json(row, col, style));
                            }
                        }
                    }
                }
                json!({
                    "index": index,
                    "name": name,
                    "rows": w.rows(),
                    "cols": w.cols(),
                    "truncated": truncated,
                    "cells": cells,
                    "inputs": inputs,
                    "styles": styles,
                })
            })
            .map_err(failed)?;
        let read_only = self.book.is_read_only();
        Ok(json!({
            "kind": "spreadsheet",
            "editable": !read_only,
            "reason": if read_only { Some("XLS workbooks are read-only: export the values as CSV.") } else { None },
            "readOnly": read_only,
            "sheets": sheets.iter().map(|(_, name)| json!({ "name": name })).collect::<Vec<_>>(),
            "active": active,
            "sheet": sheet,
        }))
    }

    pub(crate) fn save(&self, to: &str) -> Result<Vec<u8>> {
        match kind(to)?.as_str() {
            "xlsx" => self.book.xlsx_bytes(),
            "ods" => self.book.ods_bytes(),
            "csv" => self.book.csv_bytes(),
            other => return Err(format!("Cannot save spreadsheets as {other}")),
        }
        .map_err(failed)
    }
}

fn style_json(row: usize, col: usize, style: &CellStyle) -> Value {
    json!([row, col, { "bold": style.bold, "italic": style.italic, "underline": style.underline, "format": style.number_format }])
}

fn value_text(value: &Value) -> Result<String> {
    Ok(match value {
        Value::Null => String::new(),
        Value::Bool(b) => if *b { "TRUE" } else { "FALSE" }.into(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        _ => return Err("Cell values are text, numbers, true/false or null".into()),
    })
}

/// Runs `edit` with the sheet's allocated area grown to cover `range` when it does not already.
fn within(w: &mut Workbook, range: Range, edit: impl FnOnce(&mut Workbook) -> Result<Value>) -> Result<Value> {
    if range.bottom < w.rows() && range.right < w.cols() {
        return edit(w);
    }
    let mut error = None;
    let outcome = w.edit_expanding(range, |w| {
        edit(w).map_err(|e| {
            error = Some(e);
            xls42_core::Error::Invalid("")
        })
    });
    match error {
        Some(e) => Err(e),
        None => outcome.map_err(failed),
    }
}

fn patch_styles(w: &mut Workbook, range: Range, patch: impl Fn(&CellStyle) -> CellStyle) -> Result<Value> {
    within(w, range, |w| w.update_style(range, &patch).map(|changed| json!({ "changed": changed })).map_err(failed))
}

/// Adds or removes one decimal place in a number format code ("" or General start from the shown value).
fn adjust_decimals(code: &str, shown: &str, delta: i32) -> String {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("general") {
        let decimals = shown.split_once('.').map_or(0, |(_, f)| f.chars().take_while(char::is_ascii_digit).count());
        let next = (decimals as i32 + delta).clamp(0, 15) as usize;
        return if next == 0 { "0".into() } else { format!("0.{}", "0".repeat(next)) };
    }
    // The first run of digit placeholders is the number; keep what surrounds it (currency, %).
    let Some(start) = code.find(['0', '#']) else { return code.into() };
    let end = code[start..].find(|c: char| !matches!(c, '0' | '#' | ',' | '.')).map_or(code.len(), |n| start + n);
    let number = &code[start..end];
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let next = (fraction.len() as i32 + delta).clamp(0, 15) as usize;
    let fraction = if next == 0 { String::new() } else { format!(".{}", "0".repeat(next)) };
    format!("{}{whole}{fraction}{}", &code[..start], &code[end..])
}

fn write(w: &mut Workbook, command: &Command) -> Result<Value> {
    match command {
        Command::SetCells { range, values, .. } => {
            let target = range_ref(range)?;
            if values.is_empty() || values.iter().any(Vec::is_empty) {
                return Err("Give at least one value".into());
            }
            let texts = values
                .iter()
                .map(|row| row.iter().map(value_text).collect::<Result<Vec<_>>>())
                .collect::<Result<Vec<_>>>()?;
            let fill = texts.len() == 1 && texts[0].len() == 1 && cells(target) > 1;
            let area = if fill {
                target
            } else {
                let width = texts.iter().map(Vec::len).max().unwrap_or(1);
                Range {
                    top: target.top,
                    left: target.left,
                    bottom: target.top + texts.len() - 1,
                    right: target.left + width - 1,
                }
            };
            if area.bottom >= MAX_ROWS || area.right >= MAX_COLS {
                return Err("The values do not fit on the sheet".into());
            }
            if cells(area) > 100_000 {
                return Err("At most 100,000 cells at a time".into());
            }
            within(w, area, |w| {
                if fill {
                    let text = &texts[0][0];
                    w.set_cell(area.top, area.left, text.clone()).map_err(failed)?;
                    if text.starts_with('=') {
                        let source = Range { top: area.top, left: area.left, bottom: area.top, right: area.left };
                        w.fill_range(source, area).map_err(failed)?;
                    } else {
                        for r in area.top..=area.bottom {
                            for c in area.left..=area.right {
                                w.set_cell(r, c, text.clone()).map_err(failed)?;
                            }
                        }
                    }
                } else {
                    for (i, row) in texts.iter().enumerate() {
                        for (j, text) in row.iter().enumerate() {
                            w.set_cell(area.top + i, area.left + j, text.clone()).map_err(failed)?;
                        }
                    }
                }
                Ok(json!({ "range": range_text(area) }))
            })
        }
        Command::Clear { range, .. } => {
            let mut r = range_ref(range)?;
            if r.top >= w.rows() || r.left >= w.cols() {
                return Ok(json!({ "cleared": 0 }));
            }
            r.bottom = r.bottom.min(w.rows() - 1);
            r.right = r.right.min(w.cols() - 1);
            let mut count = 0;
            for row in r.top..=r.bottom {
                for col in r.left..=r.right {
                    if w.cell(row, col).is_some_and(|t| !t.is_empty()) {
                        w.set_cell(row, col, String::new()).map_err(failed)?;
                        count += 1;
                    }
                }
            }
            Ok(json!({ "cleared": count }))
        }
        Command::InsertRows { row, count, .. } | Command::DeleteRows { row, count, .. } => {
            let delete = matches!(command, Command::DeleteRows { .. });
            if *row == 0 || *count == 0 || *count > 1000 {
                return Err("Rows are numbered from 1; insert or delete 1 to 1000 at a time".into());
            }
            let at = row - 1;
            if at >= w.rows() {
                // Beyond the used area every row is already empty.
                return Ok(json!({ "changed": false }));
            }
            if delete {
                let count = (*count).min(w.rows() - at);
                w.delete_rows(at, count).map_err(failed)?;
            } else {
                w.insert_rows(at, *count).map_err(failed)?;
            }
            Ok(json!({ "changed": true }))
        }
        Command::InsertColumns { column, count, .. } | Command::DeleteColumns { column, count, .. } => {
            let delete = matches!(command, Command::DeleteColumns { .. });
            if *count == 0 || *count > 100 {
                return Err("Insert or delete 1 to 100 columns at a time".into());
            }
            let at = column_ref(column)?;
            if at >= w.cols() {
                return Ok(json!({ "changed": false }));
            }
            if delete {
                let count = (*count).min(w.cols() - at);
                w.delete_columns(at, count).map_err(failed)?;
            } else {
                w.insert_columns(at, *count).map_err(failed)?;
            }
            Ok(json!({ "changed": true }))
        }
        Command::SetNumberFormat { range, format, .. } => {
            let r = range_ref(range)?;
            let code = if format.trim().eq_ignore_ascii_case("general") { String::new() } else { format.trim().to_owned() };
            patch_styles(w, r, |s| CellStyle { number_format: code.clone(), ..s.clone() })
        }
        Command::AdjustDecimals { range, delta, .. } => {
            let r = range_ref(range)?;
            if *delta == 0 {
                return Ok(json!({ "changed": false }));
            }
            if cells(r) > 100_000 {
                return Err("At most 100,000 cells at a time".into());
            }
            let shown = w.display_cells();
            let mut changed = false;
            within(w, r, |w| {
                for row in r.top..=r.bottom {
                    for col in r.left..=r.right {
                        let old = w.style(row, col).map(|s| s.number_format.clone()).unwrap_or_default();
                        let text = shown.get(&(row, col)).map(String::as_str).unwrap_or("");
                        let code = adjust_decimals(&old, text, *delta);
                        if code != old {
                            let cell = Range { top: row, left: col, bottom: row, right: col };
                            changed |= w
                                .update_style(cell, |s| CellStyle { number_format: code.clone(), ..s.clone() })
                                .map_err(failed)?;
                        }
                    }
                }
                Ok(json!({ "changed": changed }))
            })
        }
        Command::SetTextStyle { range, bold, italic, underline, .. } => {
            if bold.is_none() && italic.is_none() && underline.is_none() {
                return Err("Choose bold, italic or underline".into());
            }
            let r = range_ref(range)?;
            patch_styles(w, r, |s| CellStyle {
                bold: bold.unwrap_or(s.bold),
                italic: italic.unwrap_or(s.italic),
                underline: underline.unwrap_or(s.underline),
                ..s.clone()
            })
        }
        Command::SortRange { range, by_column, descending, has_header, .. } => {
            let mut r = range_ref(range)?;
            let key = column_ref(by_column)?;
            if key < r.left || key > r.right {
                return Err(format!("Column {by_column} is outside {range}"));
            }
            if r.top >= w.rows() || r.left >= w.cols() {
                return Ok(json!({ "changed": false }));
            }
            r.bottom = r.bottom.min(w.rows() - 1);
            r.right = r.right.min(w.cols() - 1);
            let options = SortOptions { header: *has_header, ..Default::default() };
            let changed = w
                .sort_with_options(r, &[SortKey { index: key, descending: *descending }], &options)
                .map_err(failed)?;
            Ok(json!({ "changed": changed }))
        }
        Command::AggregateCells { .. } | Command::FindCells { .. } | Command::GetSheetInfo { .. } | Command::ReadCells { .. } => read(w, command),
    }
}

fn read(w: &Workbook, command: &Command) -> Result<Value> {
    if let Command::AggregateCells { range, function, .. } = command {
        let value = w.range_statistic(range_ref(range)?, function).map_err(str::to_owned)?;
        return Ok(json!({"range":range,"function":function,"value":value}));
    }
    let shown = w.display_cells();
    match command {
        Command::GetSheetInfo { focus, .. } => {
            let used = shown.keys().fold(None, |acc: Option<(usize, usize)>, &(r, c)| {
                Some(acc.map_or((r, c), |(mr, mc)| (mr.max(r), mc.max(c))))
            });
            let name = w.sheets().into_iter().find(|s| s.0 == w.active_sheet()).map(|s| s.1).unwrap_or_default();
            let (rows, cols) = used.map_or((0, 0), |(r, c)| (r + 1, c + 1));
            let used_range = used.map(|(bottom, right)| range_text(Range { top: 0, left: 0, bottom, right }));
            if let Some(focus) = focus {
                let selection = range_ref(focus)?;
                let (r, c) = (selection.top, selection.left);
                let left = c.saturating_sub(1);
                let right = (c + 1).min(MAX_COLS - 1);
                let cell = |row, col| {
                    let text = shown.get(&(row, col)).map(String::as_str).unwrap_or("");
                    let mut chars = text.chars();
                    let value: String = chars.by_ref().take(64).collect();
                    json!({"address":a1(row,col),"value":value,"truncated":chars.next().is_some()})
                };
                let headers: Vec<_> = (left..=right).map(|col| cell(0, col)).collect();
                let nearby: Vec<_> = (r.saturating_sub(1)..=(r + 1).min(MAX_ROWS - 1))
                    .flat_map(|row| (left..=right).map(move |col| cell(row, col)))
                    .collect();
                return Ok(
                    json!({"kind":"spreadsheet","sheet":name,"used_range":used_range,"selection":range_text(selection),
                    "headers_row_1":headers,"near_selection":nearby}),
                );
            }
            let preview: Vec<Vec<String>> = (0..rows.min(5))
                .map(|r| {
                    (0..cols.min(6))
                        .map(|c| shown.get(&(r, c)).map(|v| v.chars().take(100).collect()).unwrap_or_default())
                        .collect()
                })
                .collect();
            Ok(json!({
                "name": name,
                "used_range": used_range, "columns_not_shown":cols.saturating_sub(6),
                "rows": preview,
                "rows_not_shown": rows.saturating_sub(5),
            }))
        }
        Command::FindCells { text, range, after, .. } => {
            if text.is_empty() {
                return Err("Give text to find".into());
            }
            let needle = text.to_lowercase();
            let area = range.as_deref().map(range_ref).transpose()?;
            let after = after.as_deref().map(cell_ref).transpose()?;
            let mut matches = Vec::new();
            let mut next_after = None;
            let mut last = String::new();
            for (&(r, c), value) in &shown {
                if after.is_some_and(|p| (r, c) <= p)
                    || area.is_some_and(|a| r < a.top || r > a.bottom || c < a.left || c > a.right)
                {
                    continue;
                }
                if value.to_lowercase().contains(&needle) {
                    if matches.len() == 12 {
                        next_after = Some(last);
                        break;
                    }
                    last = a1(r, c);
                    matches.push(json!({"address":last,"preview":value.chars().take(100).collect::<String>()}));
                }
            }
            Ok(json!({"matches":matches,"next_after":next_after}))
        }
        Command::ReadCells { range, start, length, .. } => {
            let r = range_ref(range)?;
            if cells(r) > 2000 {
                return Err("Read at most 2000 cells at a time".into());
            }
            let start = start.unwrap_or(0);
            let length = length.unwrap_or(usize::MAX);
            let mut next_starts = serde_json::Map::new();
            let mut part = |text: &str, address: &str| {
                let mut chars = text.chars().skip(start);
                let result: String = chars.by_ref().take(length).collect();
                if chars.next().is_some() {
                    next_starts.insert(address.into(), json!(start.saturating_add(length)));
                }
                result
            };
            let mut formulas = serde_json::Map::new();
            let mut formats = serde_json::Map::new();
            let values: Vec<Vec<String>> = (r.top..=r.bottom)
                .map(|row| {
                    (r.left..=r.right)
                        .map(|col| {
                            if let Some(input) = w.cell(row, col).filter(|t| t.starts_with('=')) {
                                formulas.insert(a1(row, col), part(input, &a1(row, col)).into());
                            }
                            if let Some(style) = w.style(row, col).filter(|s| !s.number_format.is_empty()) {
                                formats.insert(a1(row, col), part(&style.number_format, &a1(row, col)).into());
                            }
                            shown.get(&(row, col)).map(|v| part(v, &a1(row, col))).unwrap_or_default()
                        })
                        .collect()
                })
                .collect();
            Ok(
                json!({ "range": range_text(r), "values": values, "formulas": formulas, "formats": formats, "next_starts":next_starts }),
            )
        }
        _ => Err("Not a read command".into()),
    }
}
