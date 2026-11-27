//! JavaScript exports (wasm32 only). Byte arrays are `Uint8Array`; JSON results are strings; errors throw.

use wasm_bindgen::prelude::*;

type JsResult<T> = std::result::Result<T, JsError>;

fn js<T>(result: crate::Result<T>) -> JsResult<T> {
    result.map_err(|e| JsError::new(&e))
}

#[wasm_bindgen(js_name = version)]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

#[wasm_bindgen(js_name = documentConvert)]
pub fn document_convert(bytes: &[u8], from: &str, to: &str, drop_package_parts: bool) -> JsResult<Vec<u8>> {
    js(crate::document_convert(bytes, from, to, drop_package_parts))
}

#[wasm_bindgen(js_name = documentHtml)]
pub fn document_html(bytes: &[u8], from: &str) -> JsResult<String> {
    js(crate::document_html(bytes, from))
}

#[wasm_bindgen(js_name = documentInfo)]
pub fn document_info(bytes: &[u8], from: &str) -> JsResult<String> {
    js(crate::document_info(bytes, from))
}

#[wasm_bindgen(js_name = spreadsheetJson)]
pub fn spreadsheet_json(bytes: &[u8], kind: &str, max_rows: usize, max_cols: usize) -> JsResult<String> {
    js(crate::spreadsheet_json(bytes, kind, max_rows, max_cols))
}

#[wasm_bindgen(js_name = spreadsheetConvert)]
pub fn spreadsheet_convert(bytes: &[u8], from: &str, to: &str) -> JsResult<Vec<u8>> {
    js(crate::spreadsheet_convert(bytes, from, to))
}

#[wasm_bindgen(js_name = spreadsheetValuesCsv)]
pub fn spreadsheet_values_csv(bytes: &[u8], from: &str, sheet: usize) -> JsResult<Vec<u8>> {
    js(crate::spreadsheet_values_csv(bytes, from, sheet))
}

#[wasm_bindgen(js_name = editNew)]
pub fn edit_new(kind: &str) -> JsResult<u32> {
    js(crate::edit_new(kind))
}

#[wasm_bindgen(js_name = editOpen)]
pub fn edit_open(bytes: &[u8], format: &str) -> JsResult<u32> {
    js(crate::edit_open(bytes, format))
}

#[wasm_bindgen(js_name = editApply)]
pub fn edit_apply(id: u32, request: &str) -> JsResult<String> {
    js(crate::edit_apply(id, request))
}

#[wasm_bindgen(js_name = editUndo)]
pub fn edit_undo(id: u32) -> JsResult<String> {
    js(crate::edit_undo(id))
}

#[wasm_bindgen(js_name = editRedo)]
pub fn edit_redo(id: u32) -> JsResult<String> {
    js(crate::edit_redo(id))
}

#[wasm_bindgen(js_name = editSnapshot)]
pub fn edit_snapshot(id: u32, options: &str) -> JsResult<String> {
    js(crate::edit_snapshot(id, options))
}

#[wasm_bindgen(js_name = editSave)]
pub fn edit_save(id: u32, format: &str, drop_package_parts: bool) -> JsResult<Vec<u8>> {
    js(crate::edit_save(id, format, drop_package_parts))
}

#[wasm_bindgen(js_name = editMarkSaved)]
pub fn edit_mark_saved(id: u32) -> JsResult<()> {
    js(crate::edit_mark_saved(id))
}

/// Image ids are passed as numbers (paragraph and image ids stay far below 2^53).
#[wasm_bindgen(js_name = editImage)]
pub fn edit_image(id: u32, image: f64) -> JsResult<Vec<u8>> {
    js(crate::edit_image(id, image as u64))
}

#[wasm_bindgen(js_name = editClose)]
pub fn edit_close(id: u32) {
    crate::edit_close(id)
}

#[wasm_bindgen(js_name = presentationOutline)]
pub fn presentation_outline(bytes: &[u8], from: &str) -> JsResult<String> {
    js(crate::presentation_outline(bytes, from))
}

#[wasm_bindgen(js_name = presentationConvert)]
pub fn presentation_convert(bytes: &[u8], from: &str, to: &str) -> JsResult<Vec<u8>> {
    js(crate::presentation_convert(bytes, from, to))
}

#[wasm_bindgen(js_name = pdfInfo)]
pub fn pdf_info(bytes: &[u8]) -> JsResult<String> {
    js(crate::pdf_info(bytes))
}

#[wasm_bindgen(js_name = barcodePng)]
pub fn barcode_png(format: &str, value: &str, width_mm: f64) -> JsResult<Vec<u8>> {
    js(crate::barcode_png(format, value, width_mm))
}

#[wasm_bindgen(js_name = barcodeSvg)]
pub fn barcode_svg(format: &str, value: &str, width_mm: f64, caption: bool) -> JsResult<String> {
    js(crate::barcode_svg(format, value, width_mm, caption))
}

#[wasm_bindgen(js_name = labelStocks)]
pub fn label_stocks() -> JsResult<String> {
    js(crate::label_stocks())
}

#[wasm_bindgen(js_name = labelSheetPdf)]
pub fn label_sheet_pdf(request: &str) -> JsResult<Vec<u8>> {
    js(crate::label_sheet_pdf(request))
}

#[wasm_bindgen(js_name = productMarks)]
pub fn product_marks(language: &str) -> JsResult<String> {
    js(crate::product_marks(language))
}

#[wasm_bindgen(js_name = productMarkSvg)]
pub fn product_mark_svg(id: &str, width_mm: f64) -> JsResult<String> {
    js(crate::product_mark_svg(id, width_mm))
}

#[wasm_bindgen(js_name = productMarkPng)]
pub fn product_mark_png(id: &str, width_mm: f64) -> JsResult<Vec<u8>> {
    js(crate::product_mark_png(id, width_mm))
}

#[wasm_bindgen(js_name = signatureOptions)]
pub fn signature_options() -> String {
    crate::signature_options()
}

#[wasm_bindgen(js_name = signatureDesigns)]
pub fn signature_designs(count: usize, form: u8, seed: &str) -> JsResult<String> {
    js(crate::signature_designs(count, form, seed))
}

#[wasm_bindgen(js_name = signaturePng)]
pub fn signature_png(name: &str, design: &str, impression: &str, ink: &str, pen: u8, pixel_height: f64) -> JsResult<Vec<u8>> {
    js(crate::signature_png(name, design, impression, ink, pen, pixel_height))
}

#[wasm_bindgen(js_name = stampTemplates)]
pub fn stamp_templates(language: &str) -> String {
    crate::stamp_templates(language)
}

#[wasm_bindgen(js_name = stampPng)]
pub fn stamp_png(template: &str, values: &str, ink: &str, impression: &str, pixel_scale: f64) -> JsResult<Vec<u8>> {
    js(crate::stamp_png(template, values, ink, impression, pixel_scale))
}

#[wasm_bindgen(js_name = presentationCreate)]
pub fn presentation_create(slides: &str, format: &str) -> JsResult<Vec<u8>> {
    js(crate::media::presentation_create(slides, format))
}
#[wasm_bindgen(js_name = imageOpen)]
pub fn image_open(bytes: &[u8]) -> JsResult<u32> { js(crate::media::image_open(bytes)) }
#[wasm_bindgen(js_name = imageEdit)]
pub fn image_edit(id: u32, action: &str) -> JsResult<String> { js(crate::media::image_edit(id, action)) }
#[wasm_bindgen(js_name = imageSave)]
pub fn image_save(id: u32, format: &str, mark: bool) -> JsResult<Vec<u8>> { js(crate::media::image_save(id, format, mark)) }
#[wasm_bindgen(js_name = imageClose)]
pub fn image_close(id: u32) { crate::media::image_close(id); }

#[wasm_bindgen(js_name = editFork)]
pub fn edit_fork(id: u32) -> JsResult<u32> { js(crate::edit_fork(id)) }
