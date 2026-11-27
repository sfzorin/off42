//! The editing command layer (ARCHITECTURE.md, D13 and phase 8): serializable commands over doc42-core documents and
//! xls42-core workbooks, applied through an edit session with one Undo step per request.
//!
//! A request is one command or an array of commands, as JSON: `{"command": "insert_text", ...}` or
//! `[{...}, {...}]`. The toolbar, menus and keyboard of a shell send the same commands that the AI assistant will
//! send (`assistant/tools/*.json` name the planned tool set), so every change, whoever makes it, is one Undo step:
//! a request runs inside one `RichDocument::transact` or one `Workbook::batch`, and a request that fails changes
//! nothing. Results are JSON too: one result object per command (an array for an array request).
//!
//! Sessions take and return bytes and JSON only (no paths, clock or network), so the same layer serves the browser
//! (off42-wasm), and can serve the Windows/Linux shells, the CLI and the mobile bindings.

use serde_json::Value;

mod document;
mod spreadsheet;
pub use document::DocumentSession;
pub use spreadsheet::SpreadsheetSession;

pub type Result<T> = std::result::Result<T, String>;

/// How many Undo steps a session keeps (the cores keep 8 by default).
pub const HISTORY_DEPTH: usize = 100;

/// An open document or workbook with its edit history.
pub enum Session {
    Document(DocumentSession),
    Spreadsheet(SpreadsheetSession),
}

impl Session {
    /// A blank document (`"document"`) or workbook (`"spreadsheet"`).
    pub fn blank(kind: &str) -> Result<Self> {
        match kind {
            "document" => Ok(Self::Document(DocumentSession::blank())),
            "spreadsheet" => Ok(Self::Spreadsheet(SpreadsheetSession::blank())),
            other => Err(format!("Cannot create a blank {other}")),
        }
    }

    /// Opens a file by its format (extension): documents docx, odt, rtf, md, html, txt; workbooks xlsx, ods, xls, csv.
    pub fn open(bytes: &[u8], format: &str) -> Result<Self> {
        let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
        if document::format(&format).is_ok() {
            DocumentSession::open(bytes, &format).map(Self::Document)
        } else if spreadsheet::kind(&format).is_ok() {
            SpreadsheetSession::open(bytes, &format).map(Self::Spreadsheet)
        } else {
            Err(format!("Unsupported format: {format}"))
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Document(_) => "document",
            Self::Spreadsheet(_) => "spreadsheet",
        }
    }

    /// Applies a request (one command object or an array of them) as one Undo step and returns the results.
    pub fn apply(&mut self, request: &str) -> Result<String> {
        let request: Value = serde_json::from_str(request).map_err(|e| format!("Invalid command: {e}"))?;
        self.apply_value(request).map(|v| v.to_string())
    }

    pub fn apply_value(&mut self, request: Value) -> Result<Value> {
        let (commands, single) = match request {
            Value::Array(items) => (items, false),
            item => (vec![item], true),
        };
        if commands.is_empty() {
            return Ok(Value::Array(vec![]));
        }
        if commands.len() > 1000 {
            return Err("At most 1000 commands per request".into());
        }
        let results = match self {
            Self::Document(d) => d.apply(commands)?,
            Self::Spreadsheet(s) => s.apply(commands)?,
        };
        Ok(if single { results.into_iter().next().unwrap_or(Value::Null) } else { Value::Array(results) })
    }

    pub fn undo(&mut self) -> bool {
        match self {
            Self::Document(d) => d.undo(),
            Self::Spreadsheet(s) => s.undo(),
        }
    }

    pub fn redo(&mut self) -> bool {
        match self {
            Self::Document(d) => d.redo(),
            Self::Spreadsheet(s) => s.redo(),
        }
    }

    pub fn can_undo(&self) -> bool {
        match self {
            Self::Document(d) => d.doc.can_undo(),
            Self::Spreadsheet(s) => s.book.can_undo(),
        }
    }

    pub fn can_redo(&self) -> bool {
        match self {
            Self::Document(d) => d.doc.can_redo(),
            Self::Spreadsheet(s) => s.book.can_redo(),
        }
    }

    /// Changed since it was opened, created or last marked saved.
    pub fn is_dirty(&self) -> bool {
        match self {
            Self::Document(d) => d.doc.is_dirty(),
            Self::Spreadsheet(s) => s.book.is_dirty(),
        }
    }

    pub fn mark_saved(&mut self) {
        match self {
            Self::Document(d) => d.doc.mark_saved(),
            Self::Spreadsheet(s) => s.book.mark_saved(),
        }
    }

    /// Undo/Redo availability and the dirty flag as JSON: `{canUndo, canRedo, dirty}`.
    pub fn status(&self) -> String {
        serde_json::json!({ "canUndo": self.can_undo(), "canRedo": self.can_redo(), "dirty": self.is_dirty() }).to_string()
    }

    /// What a shell needs to draw the content, as JSON. Options (JSON object, may be empty): for workbooks
    /// `{sheet, maxRows, maxCols}`.
    pub fn snapshot(&self, options: &str) -> Result<String> {
        let options: Value = if options.trim().is_empty() {
            Value::Object(Default::default())
        } else {
            serde_json::from_str(options).map_err(|e| format!("Invalid snapshot options: {e}"))?
        };
        let mut value = match self {
            Self::Document(d) => d.snapshot(),
            Self::Spreadsheet(s) => s.snapshot(&options)?,
        };
        if let Value::Object(map) = &mut value {
            map.insert("canUndo".into(), self.can_undo().into());
            map.insert("canRedo".into(), self.can_redo().into());
            map.insert("dirty".into(), self.is_dirty().into());
        }
        Ok(value.to_string())
    }

    /// The content in a format. For documents, `drop_package_parts` accepts losing the opaque Word/OpenDocument
    /// parts that only a same-format save keeps (every other fidelity check stays).
    pub fn save(&self, format: &str, drop_package_parts: bool) -> Result<Vec<u8>> {
        match self {
            Self::Document(d) => d.save(format, drop_package_parts),
            Self::Spreadsheet(s) => s.save(format),
        }
    }
}

/// Column letters for a 0-based index (0 → A, 26 → AA).
pub fn column_name(index: usize) -> String {
    let mut name = Vec::new();
    let mut n = index + 1;
    while n > 0 {
        name.push(b'A' + ((n - 1) % 26) as u8);
        n = (n - 1) / 26;
    }
    name.reverse();
    String::from_utf8(name).unwrap_or_default()
}

#[cfg(test)]
mod tests;
