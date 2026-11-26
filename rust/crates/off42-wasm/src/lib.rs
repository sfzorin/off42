//! Bytes-in/bytes-out API over the off42 cores for the web version (ARCHITECTURE.md, D4/D5).
//!
//! Everything here takes and returns bytes, strings and JSON; there are no file paths, no network and no clock
//! reads that a browser cannot answer. The plain Rust functions are the API and are tested natively; on
//! `wasm32` the [`bindings`] module exports them to JavaScript with wasm-bindgen.
//!
//! Only what the cores really support is wired: rich-text documents convert between the doc42 formats (there is
//! no PDF writer for documents in the cores), workbooks are read for display and saved in the formats xls42
//! allows, presentations give their outline and convert between PPTX and ODP, PDFs report what pdf42 reads, and
//! label42/marks42 generate codes, label sheets, product marks, signatures and stamps.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[cfg(target_arch = "wasm32")]
pub mod bindings;

pub type Result<T> = std::result::Result<T, String>;

fn json_string(value: &Value) -> String {
    value.to_string()
}

// MARK: - Documents (doc42-core)

use doc42_core::rich::{Format as DocFormat, RichDocument};

/// Document formats by file extension: docx, odt, rtf, md/markdown, html/htm, txt.
pub fn document_format(name: &str) -> Result<DocFormat> {
    Ok(match name.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
        "docx" => DocFormat::Docx,
        "odt" => DocFormat::Odt,
        "rtf" => DocFormat::Rtf,
        "md" | "markdown" => DocFormat::Markdown,
        "html" | "htm" => DocFormat::Html,
        "txt" | "text" => DocFormat::Text,
        other => return Err(format!("Unsupported document format: {other}")),
    })
}

/// Converts a document between doc42 formats. The core refuses conversions that would lose content the target
/// cannot carry (bookmarks, tables of contents, plain text from formatted documents, and the opaque Word or
/// OpenDocument package parts it keeps for same-format saves). With `drop_package_parts` the caller accepts
/// losing only those opaque package parts (unknown styles, settings, embedded extras); every other check stays.
pub fn document_convert(bytes: &[u8], from: &str, to: &str, drop_package_parts: bool) -> Result<Vec<u8>> {
    let document = RichDocument::decode(bytes, document_format(from)?).map_err(|e| e.to_string())?;
    let to = document_format(to)?;
    if !drop_package_parts {
        return document.encode(to).map_err(|e| e.to_string());
    }
    let mut state = document.snapshot();
    if to != DocFormat::Docx {
        state.docx_parts.clear();
    }
    if to != DocFormat::Odt {
        state.odt_parts.clear();
    }
    RichDocument::encode_snapshot(state, to).map_err(|e| e.to_string())
}

/// HTML preview of a document (the doc42 HTML encoder, without the retained package parts that only a
/// same-format save could keep). Hosts should show it without scripts.
pub fn document_html(bytes: &[u8], from: &str) -> Result<String> {
    String::from_utf8(document_convert(bytes, from, "html", true)?).map_err(|e| e.to_string())
}

/// Word, character and paragraph counts as JSON (doc42 statistics).
pub fn document_info(bytes: &[u8], from: &str) -> Result<String> {
    let document = RichDocument::decode(bytes, document_format(from)?).map_err(|e| e.to_string())?;
    let s = document.statistics();
    Ok(json_string(&json!({
        "words": s.words,
        "characters": s.characters,
        "charactersWithoutSpaces": s.characters_without_spaces,
        "paragraphs": s.paragraphs,
    })))
}

// MARK: - Spreadsheets (xls42-core)

use xls42_core::Workbook;

fn spreadsheet_kind(name: &str) -> Result<String> {
    let kind = name.trim().trim_start_matches('.').to_ascii_lowercase();
    match kind.as_str() {
        "xlsx" | "ods" | "xls" | "csv" => Ok(kind),
        other => Err(format!("Unsupported spreadsheet format: {other}")),
    }
}

/// Opens CSV, XLSX, ODS or XLS (read-only) from memory.
pub fn spreadsheet_open(bytes: &[u8], kind: &str) -> Result<Workbook> {
    let kind = spreadsheet_kind(kind)?;
    if kind == "csv" {
        Workbook::from_csv_bytes(bytes).map_err(|e| e.to_string())
    } else {
        Workbook::from_office_bytes(bytes, &kind).map_err(|e| e.to_string())
    }
}

/// Displayed (calculated and formatted) text of each sheet as JSON:
/// `{readOnly, sheets: [{name, rows, cols, truncated, cells: [[row, col, text], ...]}]}`.
/// Cells beyond `max_rows` × `max_cols` are left out and the sheet is marked `truncated`.
pub fn spreadsheet_json(bytes: &[u8], kind: &str, max_rows: usize, max_cols: usize) -> Result<String> {
    let mut book = spreadsheet_open(bytes, kind)?;
    let mut sheets = Vec::new();
    for (id, name) in book.sheets() {
        book.select_sheet(id).map_err(|e| e.to_string())?;
        let mut truncated = false;
        let cells: Vec<Value> = book
            .display_cells()
            .into_iter()
            .filter(|((row, col), _)| {
                let inside = *row < max_rows && *col < max_cols;
                truncated |= !inside;
                inside
            })
            .map(|((row, col), text)| json!([row, col, text]))
            .collect();
        sheets.push(json!({
            "name": name,
            "rows": book.rows(),
            "cols": book.cols(),
            "truncated": truncated,
            "cells": cells,
        }));
    }
    Ok(json_string(&json!({ "readOnly": book.is_read_only(), "sheets": sheets })))
}

/// Saves a workbook as XLSX, ODS or CSV through the xls42 writers (which refuse conversions that would lose
/// content they cannot carry, e.g. CSV from a formatted workbook).
pub fn spreadsheet_convert(bytes: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    let book = spreadsheet_open(bytes, from)?;
    match spreadsheet_kind(to)?.as_str() {
        "xlsx" => book.xlsx_bytes(),
        "ods" => book.ods_bytes(),
        "csv" => book.csv_bytes(),
        other => return Err(format!("Cannot save spreadsheets as {other}")),
    }
    .map_err(|e| e.to_string())
}

/// The displayed values of one sheet (0-based index) as CSV, for any workbook xls42 can open.
pub fn spreadsheet_values_csv(bytes: &[u8], from: &str, sheet: usize) -> Result<Vec<u8>> {
    let mut book = spreadsheet_open(bytes, from)?;
    let (id, _) = *book.sheets().get(sheet).ok_or("No such sheet")?;
    book.select_sheet(id).map_err(|e| e.to_string())?;
    let cells = book.display_cells();
    let rows = cells.keys().map(|(r, _)| r + 1).max().unwrap_or(0);
    let cols = cells.keys().map(|(_, c)| c + 1).max().unwrap_or(0);
    let mut out = String::new();
    for row in 0..rows {
        for col in 0..cols {
            if col > 0 {
                out.push(',');
            }
            let text = cells.get(&(row, col)).map(String::as_str).unwrap_or("");
            if text.contains([',', '"', '\n', '\r']) {
                out.push('"');
                out.push_str(&text.replace('"', "\"\""));
                out.push('"');
            } else {
                out.push_str(text);
            }
        }
        out.push_str("\r\n");
    }
    Ok(out.into_bytes())
}

// MARK: - Presentations (ppt42-core)

use ppt42_core::{odp, pptx, ObjectKind, OutlineRole, Presentation};

enum DeckSource {
    Pptx(pptx::Source),
    Odp(odp::Source),
}

fn presentation_open(bytes: &[u8], from: &str) -> Result<(Presentation, DeckSource)> {
    match from.trim().trim_start_matches('.').to_ascii_lowercase().as_str() {
        "pptx" => pptx::import(bytes).map(|(deck, source)| (deck, DeckSource::Pptx(source))),
        "odp" => odp::import_with_source(bytes).map(|(deck, source)| (deck, DeckSource::Odp(source))),
        other => Err(format!("Unsupported presentation format: {other}")),
    }
}

/// Slide outline as JSON: `{slides: [{title, body, notes, hidden, objects: [{kind, role, text, cells}]}]}`.
/// Imported slides keep their text in objects; `role` is "title", "body", "footer" or null (placeholder role).
pub fn presentation_outline(bytes: &[u8], from: &str) -> Result<String> {
    let (deck, source) = presentation_open(bytes, from)?;
    let role = |slide: u64, object: u64| match &source {
        DeckSource::Pptx(s) => s.outline_role(slide, object),
        DeckSource::Odp(s) => s.outline_role(slide, object),
    };
    let slides: Vec<Value> = deck
        .slides()
        .iter()
        .map(|slide| {
            let objects: Vec<Value> = slide
                .objects
                .iter()
                .filter(|o| !o.text.trim().is_empty() || !o.cells.is_empty() || o.kind == ObjectKind::Picture)
                .map(|o| {
                    let role = role(slide.id, o.id).map(|r| match r {
                        OutlineRole::Title => "title",
                        OutlineRole::Body => "body",
                        OutlineRole::Footer => "footer",
                    });
                    json!({
                        "kind": format!("{:?}", o.kind),
                        "role": role,
                        "text": o.text,
                        "cells": o.cells,
                    })
                })
                .collect();
            json!({
                "title": slide.title,
                "body": slide.body,
                "notes": slide.notes,
                "hidden": slide.hidden,
                "objects": objects,
            })
        })
        .collect();
    Ok(json_string(&json!({ "slides": slides })))
}

/// Converts between PPTX and ODP; saving in the same format goes through the core's overlay writer, which keeps
/// the parts of the original package it does not edit.
pub fn presentation_convert(bytes: &[u8], from: &str, to: &str) -> Result<Vec<u8>> {
    let (deck, source) = presentation_open(bytes, from)?;
    match (to.trim().trim_start_matches('.').to_ascii_lowercase().as_str(), source) {
        ("pptx", DeckSource::Pptx(source)) => pptx::export(&deck, &source),
        ("pptx", DeckSource::Odp(_)) => pptx::export_new(&deck),
        ("odp", DeckSource::Odp(source)) => odp::export_overlay(&deck, &source),
        ("odp", DeckSource::Pptx(_)) => odp::export(&deck),
        (other, _) => Err(format!("Cannot save presentations as {other}")),
    }
}

// MARK: - PDF (pdf42-core)

use pdf42_core::{PdfDocument, PdfOpenProbe};

/// What pdf42 reads from a PDF, as JSON: page count, metadata, bookmarks, attachments and form fields.
/// Encrypted PDFs report `{encrypted: true}` only.
pub fn pdf_info(bytes: &[u8]) -> Result<String> {
    let doc: PdfDocument = match PdfDocument::probe_open_bytes(bytes)? {
        PdfOpenProbe::Ready(doc) => doc,
        PdfOpenProbe::PasswordRequired => return Ok(json_string(&json!({ "encrypted": true }))),
    };
    let mut metadata = serde_json::Map::new();
    for key in ["Title", "Author", "Subject", "Keywords", "Creator", "Producer"] {
        if let Some(value) = doc.metadata(key)? {
            metadata.insert(key.into(), value.into());
        }
    }
    let fields: Vec<Value> =
        doc.form_fields().into_iter().map(|(name, kind, value)| json!({"name": name, "kind": kind, "value": value})).collect();
    Ok(json_string(&json!({
        "encrypted": false,
        "pages": doc.page_count(),
        "metadata": metadata,
        "bookmarks": doc.bookmarks().unwrap_or_default(),
        "attachments": doc.attachments().unwrap_or_default(),
        "formFields": fields,
    })))
}

// MARK: - Codes, labels and product marks (label42)

use label42::{Format as CodeFormat, Label, Stock, TechnicalElement};

fn code_element(format: &str, value: &str, width_mm: f64, caption: bool) -> Result<TechnicalElement> {
    Ok(TechnicalElement {
        symbol_id: None,
        code_type: CodeFormat::parse(format)?,
        value: value.to_owned(),
        width_mm,
        caption,
    })
}

/// QR, Data Matrix, Code 128 or Aztec as a 300 dpi PNG.
pub fn barcode_png(format: &str, value: &str, width_mm: f64) -> Result<Vec<u8>> {
    label42::barcode_png(CodeFormat::parse(format)?, value, width_mm)
}

/// The same code as SVG (millimetre size), optionally with the value printed underneath.
pub fn barcode_svg(format: &str, value: &str, width_mm: f64, caption: bool) -> Result<String> {
    code_element(format, value, width_mm, caption)?.svg()
}

/// The built-in A4 label stocks as JSON.
pub fn label_stocks() -> Result<String> {
    serde_json::to_string(Stock::catalog()?).map_err(|e| e.to_string())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SheetRequest {
    /// A catalogue stock id, or "custom" with the custom dimensions below (mm).
    stock: String,
    #[serde(default)]
    width: f64,
    #[serde(default)]
    height: f64,
    #[serde(default)]
    gap_x: f64,
    #[serde(default)]
    gap_y: f64,
    #[serde(default)]
    first_slot: Option<usize>,
    /// "qr", "datamatrix", "code128", "aztec", or absent for text-only labels.
    #[serde(default)]
    code_format: Option<String>,
    labels: Vec<LabelRequest>,
}

#[derive(Deserialize)]
struct LabelRequest {
    text: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default = "one")]
    copies: usize,
}

fn one() -> usize {
    1
}

/// An A4 label sheet PDF. `request` is JSON: `{stock, width?, height?, gapX?, gapY?, firstSlot? (1-based), codeFormat?,
/// labels: [{text, code?, copies?}]}`.
pub fn label_sheet_pdf(request: &str) -> Result<Vec<u8>> {
    let request: SheetRequest = serde_json::from_str(request).map_err(|e| e.to_string())?;
    let stock = if request.stock == "custom" {
        Stock::custom(request.width, request.height, request.gap_x, request.gap_y)?
    } else {
        Stock::catalog()?
            .iter()
            .find(|s| s.id == request.stock)
            .cloned()
            .ok_or_else(|| format!("Unknown label stock: {}", request.stock))?
    };
    let format = request.code_format.as_deref().filter(|f| !f.is_empty()).map(CodeFormat::parse).transpose()?;
    let labels: Vec<Label> = request
        .labels
        .into_iter()
        .map(|l| Label { text: l.text, code: l.code.filter(|c| !c.is_empty()), copies: l.copies })
        .collect();
    label42::sheet_pdf_stock(&stock, request.first_slot.unwrap_or(1), format, &labels)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct MarkInfo {
    id: &'static str,
    name: String,
    category: String,
    purpose: String,
    warning: Option<String>,
    raster: bool,
    unverified: bool,
    needs_number: bool,
}

/// The product mark catalogue (conformity, safety, environmental, ... marks) in a language (English fallback).
pub fn product_marks(language: &str) -> Result<String> {
    let marks: Vec<MarkInfo> = label42::symbols()
        .iter()
        .map(|s| MarkInfo {
            id: s.id,
            name: s.name_in(language),
            category: s.category_in(language),
            purpose: s.purpose_in(language),
            warning: s.warning_in(language),
            raster: s.raster,
            unverified: s.unverified,
            needs_number: s.needs_number,
        })
        .collect();
    serde_json::to_string(&marks).map_err(|e| e.to_string())
}

fn mark_element(id: &str, width_mm: f64) -> TechnicalElement {
    TechnicalElement { symbol_id: Some(id.into()), code_type: CodeFormat::Qr, value: String::new(), width_mm, caption: false }
}

/// A product mark as SVG at the given width.
pub fn product_mark_svg(id: &str, width_mm: f64) -> Result<String> {
    mark_element(id, width_mm).svg()
}

/// A product mark as a 600 dpi PNG at the given width.
pub fn product_mark_png(id: &str, width_mm: f64) -> Result<Vec<u8>> {
    mark_element(id, width_mm).png()
}

// MARK: - Signatures and stamps (marks42)

use marks42::seal::{self, SealSpec};
use marks42::signature::{self, Pen, SignatureForm};
use marks42::{InkColor, SeededRandom};

fn ink_color(name: &str) -> Result<InkColor> {
    InkColor::ALL
        .into_iter()
        .find(|c| format!("{c:?}").eq_ignore_ascii_case(name.trim()))
        .ok_or_else(|| format!("Unknown ink: {name}"))
}

fn parse_seed(text: &str) -> Result<u64> {
    text.trim().parse().map_err(|_| format!("Invalid seed: {text}"))
}

/// Signature forms, pens and inks as JSON (raw values match the macOS apps).
pub fn signature_options() -> String {
    json_string(&json!({
        "forms": SignatureForm::ALL.iter().map(|f| json!({"value": f.raw_value(), "title": f.title()})).collect::<Vec<_>>(),
        "pens": Pen::ALL.iter().map(|p| json!({"value": p.raw_value(), "title": p.title()})).collect::<Vec<_>>(),
        "inks": InkColor::ALL.iter().map(|c| json!({"value": format!("{c:?}").to_lowercase(), "title": c.title()})).collect::<Vec<_>>(),
    }))
}

/// `count` signature designs for a form, drawn from `seed` (the host supplies randomness, e.g. Web Crypto).
/// Design seeds are returned as decimal strings because they exceed JavaScript's safe integers.
pub fn signature_designs(count: usize, form: u8, seed: &str) -> Result<String> {
    let form = SignatureForm::from_raw_value(form).ok_or("Unknown signature form")?;
    let mut rng = SeededRandom::new(parse_seed(seed)?);
    let designs: Vec<String> =
        signature::design_seeds_with(count.clamp(1, 12), form, &mut rng).into_iter().map(|d| d.to_string()).collect();
    serde_json::to_string(&designs).map_err(|e| e.to_string())
}

/// A handwritten signature as a transparent PNG.
pub fn signature_png(name: &str, design: &str, impression: &str, ink: &str, pen: u8, pixel_height: f64) -> Result<Vec<u8>> {
    if name.trim().is_empty() {
        return Err("Enter a name.".into());
    }
    let pen = Pen::from_raw_value(pen).ok_or("Unknown pen")?;
    let height = pixel_height.clamp(20.0, signature::MAXIMUM_PIXEL_HEIGHT);
    let image = signature::render(name, parse_seed(design)?, parse_seed(impression)?, ink_color(ink)?.pen(), pen, height)
        .ok_or("This name cannot be drawn as a signature.")?;
    marks42::png(&image)
}

/// Stamp templates as JSON: `[{id, title, category, round, ink, fields: [{key, label, example, multiline, date}]}]`.
/// marks42 has template titles in English, Russian and Turkish; other languages get English. Categories and field
/// labels are English (the desktop apps translate them in their own string tables).
pub fn stamp_templates(language: &str) -> String {
    let index = match language.split(['-', '_']).next().unwrap_or("") {
        "ru" => 1,
        "tr" => 2,
        _ => 0,
    };
    let templates: Vec<Value> = seal::templates()
        .iter()
        .map(|t| {
            json!({
                "id": t.id,
                "title": t.titles[index],
                "category": t.category.title(),
                "round": t.is_round(),
                "ink": format!("{:?}", t.ink).to_lowercase(),
                "fields": t.fields().iter().map(|f| json!({
                    "key": f.key,
                    "label": f.label,
                    "example": f.example,
                    "multiline": f.multiline,
                    "date": f.label.starts_with("Date"),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json_string(&Value::Array(templates))
}

/// A stamp impression as a transparent PNG. `values` is a JSON object of field texts; missing fields use the
/// template's examples. Date fields must be supplied by the host (the browser knows the local date).
pub fn stamp_png(template: &str, values: &str, ink: &str, impression: &str, pixel_scale: f64) -> Result<Vec<u8>> {
    let t = seal::named(template).ok_or_else(|| format!("Unknown stamp template: {template}"))?;
    let values: std::collections::HashMap<String, String> = serde_json::from_str(values).map_err(|e| e.to_string())?;
    // An empty date field means "today" to the core, which reads the system clock; wasm32-unknown-unknown has
    // none, so the browser must pass the date itself.
    if cfg!(target_arch = "wasm32") {
        for field in t.fields().iter().filter(|f| f.label.starts_with("Date")) {
            let given = values.get(field.key).map(String::as_str);
            if given.unwrap_or(field.example).trim().is_empty() && given != Some(" ") && field.example != " " {
                return Err(format!("Enter a value for “{}”.", field.label));
            }
        }
    }
    let ink = if ink.trim().is_empty() { t.ink } else { ink_color(ink)? };
    let spec = SealSpec { template_id: t.id.into(), values, ink: ink.stamp_pad(), design_seed: 42 };
    let image = seal::render(&spec, parse_seed(impression)?, pixel_scale.clamp(0.25, 4.0)).ok_or("Could not draw this stamp.")?;
    marks42::png(&image)
}

#[cfg(test)]
mod tests;
