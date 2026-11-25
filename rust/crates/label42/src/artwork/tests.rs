use super::*;
fn source(rotation: i64) -> Vec<u8> {
    let mut doc = Document::with_version("1.4");
    let root = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let body = doc.add_object(Stream::new(
        dictionary! {},
        b"q 0.2 0.4 0.8 rg 30 45 100 70 re f Q BT /F1 12 Tf 30 200 Td (VECTOR-ART) Tj ET".to_vec(),
    ));
    let page = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>root,"Contents"=>body});
    doc.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>1,"Kids"=>vec![Object::Reference(page)],"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Rotate"=>rotation,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}}));
    let metadata =
        doc.add_object(dictionary! {"Title"=>Object::string_literal("PRIVATE-METADATA")});
    let unused = doc.add_object(Stream::new(
        dictionary! {},
        b"PRIVATE-UNUSED-ATTACHMENT".to_vec(),
    ));
    let _ = unused;
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", metadata);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
#[test]
fn vector_resources_rotation_crop_copies_and_first_slot_are_preserved_without_source_payloads() {
    let stock = Stock::custom(70., 37., 0., 0.).unwrap();
    let capacity = stock.columns * stock.rows;
    for rotation in [0, 90, 180, 270] {
        let bytes = source(rotation);
        let output =
            sheet_pdf_artwork_stock(&stock, capacity, 2, &bytes, Some([30., 40., 130., 240.]))
                .unwrap();
        let doc = Document::load_mem(&output).unwrap();
        assert_eq!(doc.get_pages().len(), 2);
        assert!(doc.trailer.get(b"Info").is_err());
        let forms = doc
            .objects
            .values()
            .filter_map(|v| v.as_stream().ok())
            .filter(|v| v.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Form"))
            .collect::<Vec<_>>();
        assert_eq!(forms.len(), 2);
        assert!(!doc
            .objects
            .values()
            .any(|v| v.as_stream().ok().is_some_and(|v| v
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|v| v.as_name().ok())
                == Some(b"Image"))));
        let artwork = forms
            .iter()
            .find(|v| v.dict.get(b"Matrix").is_ok())
            .unwrap();
        let expected = match rotation {
            0 => [1., 0., 0., 1., -30., -40.],
            90 => [0., -1., 1., 0., -40., 130.],
            180 => [-1., 0., 0., -1., 130., 240.],
            _ => [0., 1., -1., 0., 240., -30.],
        };
        assert_eq!(
            artwork.dict.get(b"Matrix").unwrap().as_array().unwrap(),
            &expected.into_iter().map(pdf_num).collect::<Vec<_>>()
        );
        let body = artwork.decompressed_content().unwrap();
        assert!(String::from_utf8(body).unwrap().contains("VECTOR-ART"));
        assert!(!String::from_utf8_lossy(&output).contains("PRIVATE"));
        for page in doc.get_pages().into_values() {
            assert_eq!(
                String::from_utf8(doc.get_page_content(page))
                    .unwrap()
                    .matches("/Label Do")
                    .count(),
                1
            );
        }
        assert_eq!(Document::load_mem(&bytes).unwrap().get_pages().len(), 1);
    }
}
#[test]
fn invalid_regions_page_counts_and_unflattened_visible_annotations_are_refused() {
    let stock = Stock::custom(70., 37., 0., 0.).unwrap();
    let bytes = source(0);
    for region in [
        [0., 0., 1., 1.],
        [0., 0., f64::NAN, 100.],
        [10., 10., 5., 5.],
        [500., 500., 600., 600.],
    ] {
        assert!(sheet_pdf_artwork_stock(&stock, 1, 1, &bytes, Some(region)).is_err());
    }
    assert!(sheet_pdf_artwork_stock(&stock, 0, 1, &bytes, None).is_err());
    assert!(sheet_pdf_artwork_stock(&stock, 1, 5001, &bytes, None).is_err());
    let mut doc = Document::load_mem(&bytes).unwrap();
    let page = doc.get_pages()[&1];
    let note=doc.add_object(dictionary!{"Type"=>"Annot","Subtype"=>"Text","Rect"=>vec![30.into(),40.into(),60.into(),60.into()]});
    doc.get_dictionary_mut(page)
        .unwrap()
        .set("Annots", vec![Object::Reference(note)]);
    let mut annotated = Vec::new();
    doc.save_to(&mut annotated).unwrap();
    assert!(sheet_pdf_artwork_stock(&stock, 1, 1, &annotated, None)
        .unwrap_err()
        .contains("Flatten"));
    doc.get_dictionary_mut(note).unwrap().set("Subtype", "Link");
    let mut link = Vec::new();
    doc.save_to(&mut link).unwrap();
    assert!(sheet_pdf_artwork_stock(&stock, 1, 1, &link, None).is_ok());
}

#[test]
fn bitmap_source_keeps_pixels_alpha_and_displayed_aspect_without_private_document_content() {
    let rgba = [255, 0, 0, 255, 0, 0, 255, 128];
    let bytes = bitmap_artwork_pdf(2, 1, &rgba, (120., 60.)).unwrap();
    let doc = Document::load_mem(&bytes).unwrap();
    let image = doc
        .objects
        .values()
        .filter_map(|v| v.as_stream().ok())
        .find(|v| v.dict.get(b"Subtype").ok().and_then(|v| v.as_name().ok()) == Some(b"Image"))
        .unwrap();
    assert_eq!(
        image.decompressed_content().unwrap(),
        [255, 0, 0, 127, 127, 255]
    );
    assert_eq!(
        bounds(
            &doc,
            &inherited(&doc, doc.get_pages()[&1], b"MediaBox")
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        [0., 0., 120., 60.]
    );
    let stock = Stock::custom(70., 37., 0., 0.).unwrap();
    assert!(sheet_pdf_artwork_stock(&stock, 1, 3, &bytes, None).is_ok());
    assert!(bitmap_artwork_pdf(1, 1, &[0, 0, 0, 255], (0.05, 0.05)).is_ok());
    assert!(bitmap_artwork_pdf(2, 1, &rgba[..7], (120., 60.)).is_err());
    assert!(bitmap_artwork_pdf(u32::MAX, 2, &[], (120., 60.)).is_err());
}
