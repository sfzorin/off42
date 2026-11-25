//! Render/extract these fixtures with an independent PDF engine.
use lopdf::{dictionary, Document, Object, Stream};
use pdf42_core::{
    ocr::{Page, Text},
    PdfDocument,
};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    if output.join("source.pdf").exists() {
        return Err("fixture directory already used".into());
    }
    let mut source = Document::with_version("1.4");
    let root = source.new_object_id();
    let font =
        source.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for rotation in [0, 90, 180, 270] {
        let note=source.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Text","Rect"=>vec![150.into(),250.into(),170.into(),270.into()],"Contents"=>Object::string_literal("Preserved note")});
        let content=source.add_object(Stream::new(dictionary!{},format!("q 0.2 0.4 0.8 rg 20 40 30 20 re f Q BT /F1 12 Tf 30 210 Td (VISIBLE-{rotation}) Tj ET\n2 0 0 2 0 0 cm\n").into_bytes()));
        let page=source.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"Contents"=>content,"Rotate"=>rotation,"Annots"=>vec![Object::Reference(note)]});
        kids.push(Object::Reference(page));
    }
    source.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>4,"Kids"=>kids,"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}}));
    let catalog = source.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    source.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    source.save_to(&mut bytes)?;
    fs::write(output.join("source.pdf"), &bytes)?;
    let mut doc = PdfDocument::from_edited_snapshot(&bytes, 0)?;
    let capture = doc.capture_ocr(&[0, 1, 2, 3])?;
    fs::write(output.join("capture.pdf"), &capture.bytes)?;
    let recognized = |prefix: &str| {
        (0..4)
            .map(|index| Page {
                index,
                language: "eng+rus+tur".into(),
                words: [prefix, "Привет", "中文", "ไทย", "😀"]
                    .iter()
                    .enumerate()
                    .map(|(i, text)| Text {
                        text: text.to_string(),
                        rect: [30., 180. - i as f64 * 25., 170., 198. - i as f64 * 25.],
                    })
                    .collect(),
            })
            .collect()
    };
    assert_eq!(
        doc.apply_ocr(&capture.source, recognized("OCR-original"))?,
        4
    );
    let (saved, token) = doc.save_snapshot()?;
    fs::write(output.join("recognized.pdf"), &saved)?;
    doc.mark_saved_snapshot(token);
    assert!(!doc.is_dirty());
    assert!(doc.undo());
    fs::write(output.join("undo.pdf"), doc.save_snapshot()?.0)?;
    assert!(doc.redo());
    assert_eq!(doc.save_snapshot()?.0, saved);
    let mut reopened = PdfDocument::from_edited_snapshot(&saved, 0)?;
    let capture = reopened.capture_ocr(&[0, 1, 2, 3])?;
    assert_eq!(
        reopened.apply_ocr(&capture.source, recognized("OCR-replaced"))?,
        4
    );
    fs::write(output.join("replaced.pdf"), reopened.save_snapshot()?.0)?;
    println!("{}", output.display());
    Ok(())
}
