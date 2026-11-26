//! UniFFI bindings over the off42 cores for the Android app (Kotlin) and the iPad/iPhone app (Swift),
//! ARCHITECTURE.md D10/D11.
//!
//! The capability set and the core calls are those of the web version: every function here calls the plain Rust
//! API of `off42-wasm` (which on native targets is an ordinary library; its JavaScript exports exist only for
//! wasm32) and turns its JSON results into typed records. Bytes in, bytes out: no file paths, no network, no
//! threads. Calls are synchronous and some take seconds on large files, so clients run them off the UI thread.
//!
//! Errors are [`Off42Error`], a Kotlin/Swift exception with a variant per kind of failure the UI handles
//! differently; the message is the core's (English).

use std::collections::HashMap;

use off42_wasm as api;
use serde::Deserialize;

uniffi::setup_scaffolding!();

// MARK: - Errors

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Error)]
#[uniffi(flat_error)]
pub enum Off42Error {
    /// The file type or the requested target format is not supported.
    UnsupportedFormat { message: String },
    /// A document conversion would drop opaque package parts (unknown styles, settings, embedded extras) that only
    /// a same-format save keeps. Ask the user, then call `document_convert` again with `drop_package_parts`.
    PackagePartsWouldBeLost { message: String },
    /// An argument is out of range or malformed (empty name, unknown template, bad label request, ...).
    InvalidInput { message: String },
    /// The core could not read, convert or draw the content.
    Failed { message: String },
}

impl std::fmt::Display for Off42Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Off42Error {}

impl Off42Error {
    pub fn message(&self) -> &str {
        match self {
            Self::UnsupportedFormat { message }
            | Self::PackagePartsWouldBeLost { message }
            | Self::InvalidInput { message }
            | Self::Failed { message } => message,
        }
    }

    fn classify(message: String) -> Self {
        let lower = message.to_ascii_lowercase();
        if lower.contains("package parts") {
            Self::PackagePartsWouldBeLost { message }
        } else if lower.starts_with("unsupported") || lower.starts_with("cannot save") {
            Self::UnsupportedFormat { message }
        } else if ["Unknown ", "Invalid seed", "Invalid design", "Enter ", "No such "].iter().any(|p| message.starts_with(p))
            || message.contains("Select a ")
        {
            // Messages of the binding layer and label42 about the caller's arguments (case-sensitive: the cores'
            // own "invalid document: ..." errors are about the file and stay `Failed`).
            Self::InvalidInput { message }
        } else {
            Self::Failed { message }
        }
    }
}

type Result<T> = std::result::Result<T, Off42Error>;

fn core<T>(result: api::Result<T>) -> Result<T> {
    result.map_err(Off42Error::classify)
}

fn parse<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(|e| Off42Error::Failed { message: format!("Unexpected engine output: {e}") })
}

fn invalid(message: impl Into<String>) -> Off42Error {
    Off42Error::InvalidInput { message: message.into() }
}

// MARK: - Files

/// What a file is, by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum FileKind {
    Document,
    Spreadsheet,
    Presentation,
    Pdf,
}

fn extension_of(name: &str) -> String {
    let name = name.trim();
    let ext = name.rsplit_once('.').map(|(_, e)| e).unwrap_or(name);
    match ext.to_ascii_lowercase().as_str() {
        "markdown" => "md".into(),
        "htm" => "html".into(),
        "text" => "txt".into(),
        other => other.into(),
    }
}

/// The engine's format name for a file name or extension ("Report.DOCX" → "docx", "notes.markdown" → "md").
#[uniffi::export]
pub fn normalized_extension(name: String) -> String {
    extension_of(&name)
}

/// Which viewer opens a file name or extension, or none.
#[uniffi::export]
pub fn file_kind(name: String) -> Option<FileKind> {
    Some(match extension_of(&name).as_str() {
        "docx" | "odt" | "rtf" | "md" | "html" | "txt" => FileKind::Document,
        "xlsx" | "ods" | "xls" | "csv" => FileKind::Spreadsheet,
        "pptx" | "odp" => FileKind::Presentation,
        "pdf" => FileKind::Pdf,
        _ => return None,
    })
}

/// Formats a file of this kind can be saved as, in menu order.
#[uniffi::export]
pub fn save_formats(kind: FileKind) -> Vec<String> {
    let list: &[&str] = match kind {
        FileKind::Document => &["docx", "odt", "rtf", "md", "html", "txt"],
        FileKind::Spreadsheet => &["xlsx", "ods", "csv"],
        FileKind::Presentation => &["pptx", "odp"],
        FileKind::Pdf => &[],
    };
    list.iter().map(|s| s.to_string()).collect()
}

/// The MIME type for an extension (application/octet-stream when unknown).
#[uniffi::export]
pub fn mime_type(extension: String) -> String {
    match extension_of(&extension).as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        "md" => "text/markdown",
        "html" => "text/html",
        "txt" => "text/plain",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ods" => "application/vnd.oasis.opendocument.spreadsheet",
        "xls" => "application/vnd.ms-excel",
        "csv" => "text/csv",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "odp" => "application/vnd.oasis.opendocument.presentation",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
    .into()
}

/// The engine version (the crate version, the same for every off42 binding).
#[uniffi::export]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

// MARK: - Documents

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DocumentInfo {
    pub words: u64,
    pub characters: u64,
    pub characters_without_spaces: u64,
    pub paragraphs: u64,
}

/// Converts between docx, odt, rtf, md, html and txt. See [`Off42Error::PackagePartsWouldBeLost`].
#[uniffi::export]
pub fn document_convert(bytes: Vec<u8>, from: String, to: String, drop_package_parts: bool) -> Result<Vec<u8>> {
    core(api::document_convert(&bytes, &from, &to, drop_package_parts))
}

/// A standalone HTML preview. Show it without scripts or network access.
#[uniffi::export]
pub fn document_html(bytes: Vec<u8>, from: String) -> Result<String> {
    core(api::document_html(&bytes, &from))
}

#[uniffi::export]
pub fn document_info(bytes: Vec<u8>, from: String) -> Result<DocumentInfo> {
    parse(&core(api::document_info(&bytes, &from))?)
}

// MARK: - Spreadsheets

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Cell {
    pub row: u32,
    pub col: u32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Sheet {
    pub name: String,
    /// Used size reported by the workbook (cells may be fewer).
    pub rows: u32,
    pub cols: u32,
    /// Cells beyond the requested limits were left out.
    pub truncated: bool,
    /// Displayed (calculated, formatted) text of every non-empty cell, 0-based.
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct Workbook {
    /// XLS is read-only: it can be saved as XLSX, ODS or CSV values only.
    pub read_only: bool,
    pub sheets: Vec<Sheet>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkbookJson {
    read_only: bool,
    sheets: Vec<SheetJson>,
}

#[derive(Deserialize)]
struct SheetJson {
    name: String,
    rows: u32,
    cols: u32,
    truncated: bool,
    cells: Vec<(u32, u32, String)>,
}

/// Opens CSV, XLSX, ODS or XLS and returns the displayed cells of each sheet, up to `max_rows` × `max_cols`.
#[uniffi::export]
pub fn spreadsheet_open(bytes: Vec<u8>, kind: String, max_rows: u32, max_cols: u32) -> Result<Workbook> {
    let book: WorkbookJson = parse(&core(api::spreadsheet_json(&bytes, &kind, max_rows as usize, max_cols as usize))?)?;
    Ok(Workbook {
        read_only: book.read_only,
        sheets: book
            .sheets
            .into_iter()
            .map(|s| Sheet {
                name: s.name,
                rows: s.rows,
                cols: s.cols,
                truncated: s.truncated,
                cells: s.cells.into_iter().map(|(row, col, text)| Cell { row, col, text }).collect(),
            })
            .collect(),
    })
}

/// Saves a workbook as xlsx, ods or csv (the writers refuse conversions that would lose content).
#[uniffi::export]
pub fn spreadsheet_convert(bytes: Vec<u8>, from: String, to: String) -> Result<Vec<u8>> {
    core(api::spreadsheet_convert(&bytes, &from, &to))
}

/// The displayed values of one sheet (0-based) as CSV.
#[uniffi::export]
pub fn spreadsheet_values_csv(bytes: Vec<u8>, from: String, sheet: u32) -> Result<Vec<u8>> {
    core(api::spreadsheet_values_csv(&bytes, &from, sheet as usize))
}

// MARK: - Presentations

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct SlideObject {
    /// "Text", "Picture", "Table", "Shape", ... (the ppt42 object kind).
    pub kind: String,
    /// "title", "body", "footer" or none (placeholder role of imported slides).
    pub role: Option<String>,
    pub text: String,
    pub cells: Vec<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct Slide {
    pub title: String,
    pub body: String,
    pub notes: String,
    pub hidden: bool,
    pub objects: Vec<SlideObject>,
}

#[derive(Deserialize)]
struct OutlineJson {
    slides: Vec<Slide>,
}

/// The text outline of a PPTX or ODP deck.
#[uniffi::export]
pub fn presentation_outline(bytes: Vec<u8>, from: String) -> Result<Vec<Slide>> {
    Ok(parse::<OutlineJson>(&core(api::presentation_outline(&bytes, &from))?)?.slides)
}

/// The title a slide shows: its own title, else its title placeholder, else (for slides without placeholders) its
/// first text box. Returns the index of the object used as the title, if any, so a viewer can skip it.
#[uniffi::export]
pub fn slide_title_object(slide: Slide) -> Option<u32> {
    if !slide.title.is_empty() {
        return None;
    }
    let by_role = slide.objects.iter().position(|o| o.role.as_deref() == Some("title"));
    let index = by_role.or_else(|| {
        slide.objects.iter().all(|o| o.role.is_none()).then(|| slide.objects.iter().position(|o| o.kind == "Text")).flatten()
    });
    index.map(|i| i as u32)
}

/// Converts between pptx and odp (same-format saves keep the parts of the package that are not edited).
#[uniffi::export]
pub fn presentation_convert(bytes: Vec<u8>, from: String, to: String) -> Result<Vec<u8>> {
    core(api::presentation_convert(&bytes, &from, &to))
}

// MARK: - PDF

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct FormField {
    pub name: String,
    pub kind: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PdfInfo {
    /// Password-protected: nothing else is read.
    pub encrypted: bool,
    pub pages: u32,
    /// Title, Author, Subject, Keywords, Creator, Producer when present.
    pub metadata: Vec<MetadataEntry>,
    pub bookmarks: Vec<String>,
    pub attachments: Vec<String>,
    pub form_fields: Vec<FormField>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PdfInfoJson {
    encrypted: bool,
    #[serde(default)]
    pages: u32,
    #[serde(default)]
    metadata: serde_json::Map<String, serde_json::Value>,
    #[serde(default)]
    bookmarks: Vec<String>,
    #[serde(default)]
    attachments: Vec<String>,
    #[serde(default)]
    form_fields: Vec<FormField>,
}

/// What pdf42 reads from a PDF: pages, metadata, bookmarks, attachments, form fields.
#[uniffi::export]
pub fn pdf_info(bytes: Vec<u8>) -> Result<PdfInfo> {
    let info: PdfInfoJson = parse(&core(api::pdf_info(&bytes))?)?;
    const ORDER: [&str; 6] = ["Title", "Author", "Subject", "Keywords", "Creator", "Producer"];
    let metadata = ORDER
        .iter()
        .filter_map(|key| {
            info.metadata.get(*key).and_then(|v| v.as_str()).map(|v| MetadataEntry { key: key.to_string(), value: v.into() })
        })
        .collect();
    Ok(PdfInfo {
        encrypted: info.encrypted,
        pages: info.pages,
        metadata,
        bookmarks: info.bookmarks,
        attachments: info.attachments,
        form_fields: info.form_fields,
    })
}

// MARK: - Codes, labels and product marks

/// QR ("qr"), Data Matrix ("datamatrix"), Code 128 ("code128") or Aztec ("aztec") as a 300 dpi PNG.
#[uniffi::export]
pub fn barcode_png(format: String, value: String, width_mm: f64) -> Result<Vec<u8>> {
    core(api::barcode_png(&format, &value, width_mm))
}

/// The same code as SVG in millimetres, optionally with the value printed underneath.
#[uniffi::export]
pub fn barcode_svg(format: String, value: String, width_mm: f64, caption: bool) -> Result<String> {
    core(api::barcode_svg(&format, &value, width_mm, caption))
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct LabelStock {
    pub id: String,
    pub name: String,
    /// Label size and layout in millimetres.
    pub width: f64,
    pub height: f64,
    pub columns: u32,
    pub rows: u32,
    pub margin_left: f64,
    pub margin_top: f64,
    pub gap_x: f64,
    pub gap_y: f64,
    pub source: String,
    pub note: String,
}

/// The built-in A4 label stocks.
#[uniffi::export]
pub fn label_stocks() -> Result<Vec<LabelStock>> {
    parse(&core(api::label_stocks())?)
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LabelEntry {
    pub text: String,
    pub code: Option<String>,
    pub copies: u32,
}

#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct LabelSheetRequest {
    /// A stock id from [`label_stocks`], or "custom" with the custom size and gaps below (mm).
    pub stock: String,
    pub width: f64,
    pub height: f64,
    pub gap_x: f64,
    pub gap_y: f64,
    /// 1-based label position to start at on the first page.
    pub first_slot: u32,
    /// "qr", "datamatrix", "code128", "aztec", or none for text-only labels.
    pub code_format: Option<String>,
    pub labels: Vec<LabelEntry>,
}

/// Parses label lines in the web app's format, one label per line: `text | code | copies`.
#[uniffi::export]
pub fn parse_label_lines(text: String) -> Vec<LabelEntry> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|line| {
            let mut parts = line.split('|').map(str::trim);
            let text = parts.next().unwrap_or("").to_string();
            let code = parts.next().filter(|c| !c.is_empty()).map(String::from);
            let copies = parts.next().and_then(|c| c.parse().ok()).unwrap_or(1);
            LabelEntry { text, code, copies }
        })
        .collect()
}

/// An A4 label sheet PDF.
#[uniffi::export]
pub fn label_sheet_pdf(request: LabelSheetRequest) -> Result<Vec<u8>> {
    let json = serde_json::json!({
        "stock": request.stock,
        "width": request.width,
        "height": request.height,
        "gapX": request.gap_x,
        "gapY": request.gap_y,
        "firstSlot": request.first_slot,
        "codeFormat": request.code_format,
        "labels": request.labels.iter().map(|l| serde_json::json!({
            "text": l.text,
            "code": l.code,
            "copies": l.copies,
        })).collect::<Vec<_>>(),
    });
    core(api::label_sheet_pdf(&json.to_string()))
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ProductMark {
    pub id: String,
    pub name: String,
    pub category: String,
    pub purpose: String,
    pub warning: Option<String>,
    pub raster: bool,
    /// Drawing of unverified licensing or provenance.
    pub unverified: bool,
    /// Needs a registration or certificate number next to it.
    pub needs_number: bool,
}

/// The product mark catalogue, names in `language` where label42 has them (English otherwise).
#[uniffi::export]
pub fn product_marks(language: String) -> Result<Vec<ProductMark>> {
    parse(&core(api::product_marks(&language))?)
}

#[uniffi::export]
pub fn product_mark_svg(id: String, width_mm: f64) -> Result<String> {
    core(api::product_mark_svg(&id, width_mm))
}

/// A product mark as a 600 dpi PNG.
#[uniffi::export]
pub fn product_mark_png(id: String, width_mm: f64) -> Result<Vec<u8>> {
    core(api::product_mark_png(&id, width_mm))
}

// MARK: - Signatures and stamps

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct NumberedChoice {
    pub value: u8,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct NamedChoice {
    pub value: String,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct SignatureOptions {
    /// Signature forms (raw values as in the macOS apps; titles in English).
    pub forms: Vec<NumberedChoice>,
    pub pens: Vec<NumberedChoice>,
    /// Ink names: blue, violet, black, red, green.
    pub inks: Vec<NamedChoice>,
}

#[uniffi::export]
pub fn signature_options() -> Result<SignatureOptions> {
    parse(&api::signature_options())
}

/// `count` (1–12) signature designs for a form, drawn from `seed` (supply randomness from a secure source).
#[uniffi::export]
pub fn signature_designs(count: u32, form: u8, seed: u64) -> Result<Vec<u64>> {
    let designs: Vec<String> = parse(&core(api::signature_designs(count as usize, form, &seed.to_string()))?)?;
    designs.iter().map(|d| d.parse().map_err(|_| invalid(format!("Invalid design seed: {d}")))).collect()
}

/// A handwritten signature as a transparent PNG `pixel_height` pixels high (20–the core's maximum).
/// The same name, design and impression always give the same image.
#[uniffi::export]
pub fn signature_png(name: String, design: u64, impression: u64, ink: String, pen: u8, pixel_height: f64) -> Result<Vec<u8>> {
    core(api::signature_png(&name, &design.to_string(), &impression.to_string(), &ink, pen, pixel_height))
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct StampField {
    pub key: String,
    /// English label.
    pub label: String,
    pub example: String,
    pub multiline: bool,
    /// A date field: pass the date (an empty value means today).
    pub date: bool,
}

#[derive(Debug, Clone, PartialEq, Deserialize, uniffi::Record)]
pub struct StampTemplate {
    pub id: String,
    /// In English, Russian or Turkish (English for other languages).
    pub title: String,
    /// English.
    pub category: String,
    pub round: bool,
    /// Default ink name.
    pub ink: String,
    pub fields: Vec<StampField>,
}

#[uniffi::export]
pub fn stamp_templates(language: String) -> Result<Vec<StampTemplate>> {
    parse(&api::stamp_templates(&language))
}

/// A stamp impression as a transparent PNG. Missing `values` use the template's examples; an empty `ink` uses the
/// template's. The impression seed fixes tilt, pressure and grain at every `pixel_scale` (0.25–4).
#[uniffi::export]
pub fn stamp_png(
    template: String,
    values: HashMap<String, String>,
    ink: String,
    impression: u64,
    pixel_scale: f64,
) -> Result<Vec<u8>> {
    let values = serde_json::to_string(&values).map_err(|e| invalid(e.to_string()))?;
    core(api::stamp_png(&template, &values, &ink, &impression.to_string(), pixel_scale))
}

#[cfg(test)]
mod tests;
