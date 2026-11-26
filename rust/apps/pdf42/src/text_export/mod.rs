//! PDF text export: the portable builder, encoders and worker live in
//! `suite42_app::pdf::text_export`; this module supplies the PDFium engine.
#[cfg_attr(not(windows), allow(unused_imports))]
pub use suite42_app::pdf::text_export::*;
#[cfg(windows)]
pub(super) mod pdfium;

/// PDFium text extraction for the shared export and comparison workers.
#[cfg(windows)]
pub struct Pdfium;
#[cfg(windows)]
pub static PDFIUM: Pdfium = Pdfium;
#[cfg(windows)]
impl suite42_app::platform::PdfText for Pdfium {
    fn rich_text(
        &self,
        bytes: &[u8],
        pages: &[usize],
        progress: &std::sync::atomic::AtomicUsize,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<doc42_core::rich::RichDocument, String> {
        pdfium::extract(bytes, pages, progress, cancel)
    }
    fn positioned(
        &self,
        bytes: &[u8],
        page_count: usize,
        progress: &std::sync::atomic::AtomicUsize,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<suite42_app::pdf::comparison::PageText>, String> {
        pdfium::positioned(bytes, page_count, progress, cancel)
    }
}

#[cfg(test)]
mod tests {
    pub(super) fn redacted_snapshot() -> Vec<u8> {
        let source = include_bytes!("../../fixtures/page-export.pdf");
        let mut document = pdf42_core::PdfDocument::from_edited_snapshot(source, 0).unwrap();
        // The stock fixture shares its font across pages. The core correctly
        // refuses partial raster redaction of shared resources.
        assert!(document
            .redact_raster(0, 20, 20, &vec![255; 1600], [0, 0, 10, 10])
            .is_err());
        let mut source = lopdf::Document::load_mem(source).unwrap();
        let first = source.get_pages()[&1];
        let mut resources = source
            .get_dictionary(first)
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        let mut fonts = resources.get(b"Font").unwrap().as_dict().unwrap().clone();
        let font = fonts.get(b"F1").unwrap().as_reference().unwrap();
        let independent_font = source.add_object(source.get_object(font).unwrap().clone());
        fonts.set("F1", independent_font);
        resources.set("Font", fonts);
        source
            .get_dictionary_mut(first)
            .unwrap()
            .set("Resources", resources);
        let mut bytes = vec![];
        source.save_to(&mut bytes).unwrap();
        let mut document = pdf42_core::PdfDocument::from_edited_snapshot(&bytes, 0).unwrap();
        document
            .redact_raster(0, 20, 20, &vec![255; 1600], [0, 0, 10, 10])
            .unwrap();
        document.delete(1).unwrap();
        document.save_snapshot().unwrap().0
    }
    #[test]
    fn current_snapshot_removes_redacted_and_deleted_page_text() {
        let snapshot = redacted_snapshot();
        let document = lopdf::Document::load_mem(&snapshot).unwrap();
        let text = document.extract_text(&[1, 2]).unwrap();
        assert!(text.contains("PAGE-THREE"));
        assert!(!text.contains("PAGE-ONE") && !text.contains("PAGE-TWO"));
    }
    #[test]
    fn settings_resolve_the_picker_filter_without_changing_the_document() {
        use win42::App;
        let app = crate::Pdf {
            doc: Some(
                pdf42_core::PdfDocument::from_edited_snapshot(
                    include_bytes!("../../fixtures/page-export.pdf"),
                    0,
                )
                .unwrap(),
            ),
            ..Default::default()
        };
        let before = app.doc.as_ref().unwrap().save_snapshot().unwrap().0;
        for (name, extension) in [
            ("Plain text (TXT)", "txt"),
            ("Rich text (RTF)", "rtf"),
            ("Web page (HTML)", "html"),
            ("Word document (DOCX)", "docx"),
        ] {
            let action = app
                .configured_file_action(86, &["3,1".into(), name.into()])
                .unwrap();
            assert_eq!(action.extension, extension);
            assert!(action.save && action.pattern.ends_with(extension));
        }
        for values in [
            vec!["0".into(), "Plain text (TXT)".into()],
            vec!["1".into(), "unknown".into()],
            vec![],
        ] {
            assert!(app.configured_file_action(86, &values).is_err());
        }
        assert_eq!(app.doc.as_ref().unwrap().save_snapshot().unwrap().0, before);
    }
}
