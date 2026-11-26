//! PDF42 services: text comparison and export (with a lopdf `PdfText` for shells without PDFium), OCR pipeline, signature binding, page export, merging, batch headers and label regions.
pub mod batch_headers;
pub mod comparison;
pub mod label_region;
pub mod lopdf_text;
pub mod merge;
pub mod ocr;
pub mod page_export;
pub mod signatures;
pub mod text_export;
