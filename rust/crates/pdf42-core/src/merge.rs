//! Bounded batch merge over stable in-memory inputs; source files are read only.
use super::*;
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
pub fn merge_files(
    paths: &[PathBuf],
    progress: &AtomicUsize,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    if paths.is_empty() || paths.len() > 128 {
        return Err("Choose between 1 and 128 PDF files".into());
    }
    let check = || {
        if cancel.load(Ordering::Relaxed) {
            Err("PDF merge cancelled".to_string())
        } else {
            Ok(())
        }
    };
    let mut remaining = MAX_FILE as usize;
    let mut merged: Option<PdfDocument> = None;
    for (position, path) in paths.iter().enumerate() {
        check()?;
        let mut bytes = Vec::new();
        let file = fs::File::open(suite42_core::win32_path(path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        file.take(remaining as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > remaining {
            return Err("Merge inputs exceed 256 MB in total".into());
        }
        remaining -= bytes.len();
        check()?;
        let source =
            PdfDocument::open_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        drop(bytes);
        if let Some(doc) = &mut merged {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "PDF".into());
            doc.insert_document_after(doc.page_count() - 1, source, &name)
                .map_err(|e| format!("{}: {e}", path.display()))?;
        } else {
            let state = source.history.snapshot();
            let mut source = source;
            source.history = SnapshotHistory::new(state, 0);
            merged = Some(source);
        }
        progress.store(position + 1, Ordering::Relaxed);
    }
    check()?;
    let mut merged = merged.ok_or("No PDFs selected")?;
    merged.select(0)?;
    let (bytes, _) = merged.save_snapshot()?;
    check()?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let mut bytes = [0; 8];
            getrandom::fill(&mut bytes).unwrap();
            let path =
                std::env::temp_dir().join(format!("off42-merge-{}", u64::from_le_bytes(bytes)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture(path: &Path, labels: &[&str], prefix: &str, indirect: bool) {
        let mut d = Document::with_version("1.7");
        let root = d.new_object_id();
        let font =
            d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
        let mut pages = Vec::new();
        for (i, text) in labels.iter().enumerate() {
            let content = d.add_object(lopdf::Stream::new(
                dictionary! {},
                format!("BT /F1 18 Tf 20 240 Td ({text}) Tj ET").into_bytes(),
            ));
            let annotation=d.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Square","Rect"=>vec![20.into(),20.into(),50.into(),50.into()],"C"=>vec![1.into(),0.into(),0.into()]});
            let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![0.into(),0.into(),200.into(),300.into()],"CropBox"=>vec![10.into(),10.into(),190.into(),290.into()],"Rotate"=>if i==1{90}else{0},"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}},"Contents"=>content,"Annots"=>vec![annotation.into()]});
            pages.push(page);
        }
        d.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Kids"=>pages.iter().copied().map(Object::Reference).collect::<Vec<_>>(),"Count"=>pages.len()as i64}));
        let p = lopdf::text_string(prefix);
        let p = if indirect {
            Object::Reference(d.add_object(p))
        } else {
            p
        };
        let catalog=d.add_object(dictionary!{"Type"=>"Catalog","Pages"=>root,"PageLabels"=>dictionary!{"Nums"=>vec![0.into(),Object::Dictionary(dictionary!{"S"=>"r","St"=>4,"P"=>p})]}});
        d.trailer.set("Root", catalog);
        d.save(path).unwrap();
        let mut pdf = PdfDocument::open(path).unwrap();
        assert_eq!(
            pdf.page_labels().unwrap().unwrap()[0],
            format!("{prefix}iv")
        );
        pdf.add_bookmark(&format!("Start {prefix}")).unwrap();
        let bytes = pdf.save_snapshot().unwrap().0;
        fs::write(path, bytes).unwrap();
    }
    #[test]
    fn batch_merge_preserves_file_order_labels_geometry_annotations_and_bookmarks() {
        let dir = Temp::new();
        let a = dir.0.join("a.pdf");
        let b = dir.0.join("b.pdf");
        fixture(&a, &["A1", "A2"], "A-", false);
        fixture(&b, &["B1", "B2"], "B-", true);
        let original = (fs::read(&a).unwrap(), fs::read(&b).unwrap());
        let progress = AtomicUsize::new(0);
        let bytes =
            merge_files(&[b.clone(), a.clone()], &progress, &AtomicBool::new(false)).unwrap();
        assert_eq!(progress.load(Ordering::Relaxed), 2);
        let pdf = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            pdf.extract_text(&[1, 2, 3, 4])
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>(),
            vec!["B1", "B2", "A1", "A2"]
        );
        let out = PdfDocument::open_bytes(&bytes).unwrap();
        assert_eq!(
            out.page_labels().unwrap().unwrap(),
            vec!["B-iv", "B-v", "A-iv", "A-v"]
        );
        assert_eq!(out.bookmarks().unwrap(), vec!["Start B-", "Start A-"]);
        assert_eq!(out.rotation(1).unwrap(), 90);
        assert_eq!(out.rotation(3).unwrap(), 90);
        assert_eq!(out.page_box(2).unwrap(), [10.0, 10.0, 190.0, 290.0]);
        for page in pdf.get_pages().values() {
            assert_eq!(
                pdf.get_dictionary(*page)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .len(),
                1
            );
        }
        assert_eq!(fs::read(a).unwrap(), original.0);
        assert_eq!(fs::read(b).unwrap(), original.1);
    }
    #[test]
    fn merge_cancel_corrupt_source_and_input_count_do_not_return_partial_output() {
        let dir = Temp::new();
        let a = dir.0.join("a.pdf");
        fixture(&a, &["A1"], "A-", false);
        let bad = dir.0.join("bad.pdf");
        fs::write(&bad, b"not a PDF").unwrap();
        let progress = AtomicUsize::new(0);
        assert!(merge_files(&[a.clone()], &progress, &AtomicBool::new(true)).is_err());
        assert_eq!(progress.load(Ordering::Relaxed), 0);
        assert!(merge_files(&[a.clone(), bad], &progress, &AtomicBool::new(false)).is_err());
        assert_eq!(progress.load(Ordering::Relaxed), 1);
        assert!(merge_files(&vec![a; 129], &progress, &AtomicBool::new(false)).is_err());
    }
}
