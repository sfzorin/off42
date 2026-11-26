//! Services that portable code needs from a native engine or the operating system. Each shell
//! provides one implementation, chosen at build time (PDFium on Windows today).
use std::sync::atomic::{AtomicBool, AtomicUsize};

/// Text extraction from PDF bytes.
pub trait PdfText: Sync {
    /// Styled text of the zero-based `pages`, in the given order; each page starts a new page.
    fn rich_text(
        &self,
        bytes: &[u8],
        pages: &[usize],
        progress: &AtomicUsize,
        cancel: &AtomicBool,
    ) -> Result<doc42_core::rich::RichDocument, String>;
    /// Unicode text of every page with the box of each glyph, for comparison.
    fn positioned(
        &self,
        bytes: &[u8],
        page_count: usize,
        progress: &AtomicUsize,
        cancel: &AtomicBool,
    ) -> Result<Vec<crate::pdf::comparison::PageText>, String>;
}
