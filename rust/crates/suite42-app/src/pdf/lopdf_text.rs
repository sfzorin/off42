//! `PdfText` through lopdf, for shells without PDFium (Linux and macOS previews, the `off42` CLI).
//!
//! Limits compared with PDFium: plain text in lopdf's content-stream order, no character styles, and no glyph boxes,
//! so comparisons list changed words but cannot highlight them on the page.
use crate::pdf::{comparison::PageText, text_export::TextBuilder};
use crate::platform::PdfText;
use doc42_core::rich::{RichDocument, TextStyle};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

pub struct Lopdf;
pub static LOPDF: Lopdf = Lopdf;

fn open(bytes: &[u8]) -> Result<(lopdf::Document, usize), String> {
    let document = lopdf::Document::load_mem(bytes).map_err(|e| format!("Could not read the PDF: {e}"))?;
    let count = document.get_pages().len();
    Ok((document, count))
}

/// Text of the zero-based page `index`, without the trailing line break lopdf adds.
fn page_text(document: &lopdf::Document, index: usize, count: usize) -> Result<String, String> {
    if index >= count {
        return Err("Page numbers must be within the current document".into());
    }
    let mut text = document
        .extract_text(&[index as u32 + 1])
        .map_err(|e| format!("Could not read the text of page {}: {e}", index + 1))?;
    text.truncate(text.trim_end_matches(['\n', '\r']).len());
    Ok(text)
}

fn check(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("PDF text extraction cancelled".into())
    } else {
        Ok(())
    }
}

impl PdfText for Lopdf {
    fn rich_text(
        &self,
        bytes: &[u8],
        pages: &[usize],
        progress: &AtomicUsize,
        cancel: &AtomicBool,
    ) -> Result<RichDocument, String> {
        let (document, count) = open(bytes)?;
        let mut builder = TextBuilder::new();
        for (position, &index) in pages.iter().enumerate() {
            check(cancel)?;
            if position > 0 {
                builder.page_break()?;
            }
            for character in page_text(&document, index, count)?.chars() {
                builder.push(character, TextStyle::default())?;
            }
            progress.store(position + 1, Ordering::Relaxed);
        }
        builder.finish()
    }

    fn positioned(
        &self,
        bytes: &[u8],
        page_count: usize,
        progress: &AtomicUsize,
        cancel: &AtomicBool,
    ) -> Result<Vec<PageText>, String> {
        let (document, count) = open(bytes)?;
        (0..page_count.min(count))
            .map(|index| {
                check(cancel)?;
                let text = page_text(&document, index, count)?;
                progress.fetch_add(1, Ordering::Relaxed);
                Ok(PageText { text, glyphs: vec![] })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pdf::{comparison, text_export};

    fn pdf(pages: &[&str]) -> Vec<u8> {
        suite42_pdf_text::plain_text_pdf("test", &pages.join("\u{c}"), 11.0).unwrap()
    }

    #[test]
    fn exports_page_text_in_the_chosen_order_and_compares_words() {
        let bytes = pdf(&["First page words", "Second page words"]);
        let (progress, cancel) = (AtomicUsize::new(0), AtomicBool::new(false));
        let document = LOPDF.rich_text(&bytes, &[1, 0], &progress, &cancel).unwrap();
        let text = String::from_utf8(text_export::encode(&document, text_export::Output::Text).unwrap()).unwrap();
        assert_eq!(text, "Second page words\u{c}First page words");
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        assert!(LOPDF.rich_text(&bytes, &[2], &progress, &cancel).is_err());

        let changed = pdf(&["First page words", "Second sheet words"]);
        let a = LOPDF.positioned(&bytes, 2, &progress, &cancel).unwrap();
        let b = LOPDF.positioned(&changed, 2, &progress, &cancel).unwrap();
        let changes = comparison::compare(&a, &b, &cancel).unwrap();
        assert_eq!(changes.len(), 1);
        assert_eq!((changes[0].page, changes[0].removed_text(), changes[0].added_text()), (1, "page".into(), "sheet".into()));
        assert!(LOPDF.rich_text(&bytes, &[0], &progress, &AtomicBool::new(true)).is_err());
    }
}
