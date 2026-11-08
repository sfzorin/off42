use super::*;
fn source(rotation: i64) -> (PdfDocument, Vec<u8>) {
    let mut d = Document::with_version("1.7");
    let root = d.new_object_id();
    let font =
        d.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let resources = d.add_object(dictionary! {"Font"=>dictionary!{"F1"=>font}});
    let note=d.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Text","Rect"=>vec![150.into(),250.into(),170.into(),270.into()],"Contents"=>Object::string_literal("kept note")});
    let content = d.add_object(Stream::new(
        dictionary! {},
        b"q 0.2 0.4 0.8 rg 20 40 30 20 re f Q BT /F1 12 Tf 30 200 Td (VISIBLE) Tj ET".to_vec(),
    ));
    let page=d.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"Contents"=>content,"Annots"=>vec![Object::Reference(note)]});
    d.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>1,"Kids"=>vec![Object::Reference(page)],"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Rotate"=>rotation,"Resources"=>resources}));
    let catalog = d.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    d.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    d.save_to(&mut bytes).unwrap();
    (PdfDocument::from_edited_snapshot(&bytes, 0).unwrap(), bytes)
}
fn page(text: &str) -> Page {
    Page {
        index: 0,
        language: "en-US".into(),
        words: vec![Text {
            text: text.into(),
            rect: [30., 100., 160., 120.],
        }],
    }
}
#[test]
fn capture_and_apply_preserve_geometry_annotations_content_and_complete_history() {
    for rotation in [0, 90, 180, 270] {
        let (mut doc, _) = source(rotation);
        let (before, _) = doc.save_snapshot().unwrap();
        let capture = doc.capture_ocr(&[0, 0]).unwrap();
        assert_eq!(capture.pages, [(0, [10., 20., 210., 320.])]);
        let rendering = Document::load_mem(&capture.bytes).unwrap();
        let id = rendering.get_pages()[&1];
        let p = rendering.get_dictionary(id).unwrap();
        assert_eq!(p.get(b"Rotate").unwrap().as_i64().unwrap(), 0);
        assert!(p.get(b"Annots").is_err());
        assert_eq!(media_box(&rendering, id).unwrap(), [10., 20., 210., 320.]);
        assert_eq!(doc.save_snapshot().unwrap().0, before);
        assert_eq!(
            doc.apply_ocr(&capture.source, vec![page("recognized 中文 ไทย 😀")])
                .unwrap(),
            1
        );
        assert!(doc.is_dirty());
        let (saved, token) = doc.save_snapshot().unwrap();
        let output = Document::load_mem(&saved).unwrap();
        let id = output.get_pages()[&1];
        let p = output.get_dictionary(id).unwrap();
        assert_eq!(
            PdfDocument::from_edited_snapshot(&saved, 0)
                .unwrap()
                .rotation(0)
                .unwrap(),
            rotation
        );
        assert!(p.get(b"Annots").is_ok());
        assert_eq!(doc.page_box(0).unwrap(), [20., 30., 200., 310.]);
        assert_eq!(media_box(&output, id).unwrap(), [10., 20., 210., 320.]);
        let text = extracted(&output);
        assert!(text.contains("VISIBLE"));
        assert!(text.contains("recognized"));
        assert!(text.contains("中文"));
        assert!(text.contains("ไทย"));
        assert!(text.contains("😀"));
        doc.mark_saved_snapshot(token);
        assert!(!doc.is_dirty());
        doc.undo();
        assert_eq!(doc.save_snapshot().unwrap().0, before);
        doc.redo();
        assert_eq!(doc.save_snapshot().unwrap().0, saved);
    }
}
#[test]
fn stale_foreign_empty_and_invalid_results_leave_the_document_untouched() {
    let (mut doc, _) = source(0);
    let capture = doc.capture_ocr(&[0]).unwrap();
    let (before, _) = doc.save_snapshot().unwrap();
    assert_eq!(
        doc.apply_ocr(
            &capture.source,
            vec![Page {
                index: 0,
                language: "en-US".into(),
                words: Vec::new()
            }]
        )
        .unwrap(),
        0
    );
    assert_eq!(doc.save_snapshot().unwrap().0, before);
    let (mut other, _) = source(0);
    assert!(other
        .apply_ocr(&capture.source, vec![page("foreign")])
        .is_err());
    assert!(doc
        .apply_ocr(&capture.source, vec![page("duplicate"), page("duplicate")])
        .is_err());
    let mut invalid = page("bad");
    invalid.words[0].rect[2] = f64::NAN;
    assert!(doc.apply_ocr(&capture.source, vec![invalid]).is_err());
    assert_eq!(doc.save_snapshot().unwrap().0, before);
    doc.rotate(0, 90).unwrap();
    let after = doc.save_snapshot().unwrap().0;
    assert!(doc.apply_ocr(&capture.source, vec![page("stale")]).is_err());
    assert_eq!(doc.save_snapshot().unwrap().0, after);
}
#[test]
fn replacing_own_saved_layer_keeps_one_reachable_layer_and_foreign_visible_content() {
    let (mut doc, _) = source(0);
    let capture = doc.capture_ocr(&[0]).unwrap();
    doc.apply_ocr(&capture.source, vec![page("OCR-old")])
        .unwrap();
    let first = doc.save_snapshot().unwrap().0;
    let mut reopened = PdfDocument::from_edited_snapshot(&first, 0).unwrap();
    let capture = reopened.capture_ocr(&[0]).unwrap();
    reopened
        .apply_ocr(&capture.source, vec![page("OCR-new")])
        .unwrap();
    let saved = reopened.save_snapshot().unwrap().0;
    let output = Document::load_mem(&saved).unwrap();
    let text = extracted(&output);
    assert!(text.contains("OCR-new"));
    assert!(!text.contains("OCR-old"));
    assert!(text.contains("VISIBLE"));
    let resources = decorations::inherited_resources(&output, output.get_pages()[&1]).unwrap();
    let objects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    assert_eq!(
        objects
            .iter()
            .filter(|(_, v)| output
                .dereference(v)
                .ok()
                .and_then(|(_, v)| v.as_stream().ok())
                .is_some_and(
                    |s| s.dict.get(b"Off42OCR").ok().and_then(|v| v.as_bool().ok()) == Some(true)
                ))
            .count(),
        1
    );
}

fn extracted(doc: &Document) -> String {
    let id = doc.get_pages()[&1];
    let mut text = doc.extract_text(&[1]).unwrap();
    let resources = decorations::inherited_resources(doc, id).unwrap();
    if let Ok(objects) = resources.get(b"XObject") {
        let objects = doc.dereference(objects).unwrap().1.as_dict().unwrap();
        for (_, value) in objects.iter() {
            let (id, object) = doc.dereference(value).unwrap();
            let Ok(form) = object.as_stream() else {
                continue;
            };
            if form
                .dict
                .get(b"Off42OCR")
                .ok()
                .and_then(|v| v.as_bool().ok())
                != Some(true)
            {
                continue;
            }
            let mut projected = doc.clone();
            let page = projected.get_dictionary_mut(doc.get_pages()[&1]).unwrap();
            page.set("Contents", id.unwrap());
            page.set("Resources", form.dict.get(b"Resources").unwrap().clone());
            text.push_str(&projected.extract_text(&[1]).unwrap());
        }
    }
    text
}
