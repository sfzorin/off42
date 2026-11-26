use super::*;

const MARKDOWN: &[u8] = b"# Report\n\nHello **world** from off42.\n\n- one\n- two\n";

fn s(text: &str) -> String {
    text.to_string()
}

#[test]
fn files_are_classified_by_extension() {
    assert_eq!(file_kind(s("Report.DOCX")), Some(FileKind::Document));
    assert_eq!(file_kind(s("notes.markdown")), Some(FileKind::Document));
    assert_eq!(file_kind(s("budget.xls")), Some(FileKind::Spreadsheet));
    assert_eq!(file_kind(s("deck.odp")), Some(FileKind::Presentation));
    assert_eq!(file_kind(s("pdf")), Some(FileKind::Pdf));
    assert_eq!(file_kind(s("photo.jpeg")), None);
    assert_eq!(normalized_extension(s("page.HTM")), "html");
    assert_eq!(save_formats(FileKind::Spreadsheet), ["xlsx", "ods", "csv"]);
    assert!(save_formats(FileKind::Pdf).is_empty());
    assert_eq!(mime_type(s("csv")), "text/csv");
    assert_eq!(mime_type(s("x.unknown")), "application/octet-stream");
    assert!(!version().is_empty());
}

#[test]
fn documents_convert_preview_and_report_package_part_loss_as_its_own_error() {
    let docx = document_convert(MARKDOWN.to_vec(), s("md"), s("docx"), false).unwrap();
    assert!(docx.starts_with(b"PK"));
    let html = document_html(docx.clone(), s("docx")).unwrap();
    assert!(html.contains("Report") && html.contains("world"), "{html}");
    match document_convert(docx.clone(), s("docx"), s("odt"), false) {
        Err(Off42Error::PackagePartsWouldBeLost { message }) => assert!(message.contains("package parts")),
        other => panic!("expected PackagePartsWouldBeLost, got {other:?}"),
    }
    assert!(document_convert(docx, s("docx"), s("odt"), true).unwrap().starts_with(b"PK"));
    assert!(matches!(
        document_convert(MARKDOWN.to_vec(), s("md"), s("pdf"), false),
        Err(Off42Error::UnsupportedFormat { .. })
    ));
    let info = document_info(MARKDOWN.to_vec(), s("md")).unwrap();
    assert!(info.words >= 6 && info.paragraphs >= 2, "{info:?}");
    assert!(matches!(document_html(b"not a zip".to_vec(), s("docx")), Err(Off42Error::Failed { .. })));
}

#[test]
fn spreadsheets_give_typed_cells_and_convert() {
    let csv = b"Item,Price\nTea,2\nCake,3\nTotal,=SUM(B2:B3)\n".to_vec();
    let book = spreadsheet_open(csv.clone(), s("csv"), 1000, 100).unwrap();
    assert!(!book.read_only);
    let sheet = &book.sheets[0];
    assert!(sheet.cells.contains(&Cell { row: 3, col: 1, text: s("5") }), "{:?}", sheet.cells);
    let xlsx = spreadsheet_convert(csv.clone(), s("csv"), s("xlsx")).unwrap();
    let reopened = spreadsheet_open(xlsx.clone(), s("xlsx"), 1000, 100).unwrap();
    assert!(reopened.sheets[0].cells.contains(&Cell { row: 0, col: 0, text: s("Item") }));
    let values = String::from_utf8(spreadsheet_values_csv(xlsx, s("xlsx"), 0).unwrap()).unwrap();
    assert_eq!(values.lines().nth(3), Some("Total,5"));
    assert!(spreadsheet_open(csv.clone(), s("csv"), 2, 1).unwrap().sheets[0].truncated);
    assert!(matches!(spreadsheet_values_csv(csv.clone(), s("csv"), 5), Err(Off42Error::InvalidInput { .. })));
    assert!(matches!(spreadsheet_convert(csv, s("csv"), s("xls")), Err(Off42Error::UnsupportedFormat { .. })));

    // A legacy XLS workbook (written by LibreOffice) opens read-only; its values export as CSV (the apps offer only
    // that for read-only workbooks).
    let xls = include_bytes!("../../../../android/app/src/androidTest/assets/fixtures/sample.xls").to_vec();
    let book = spreadsheet_open(xls.clone(), s("xls"), 1000, 100).unwrap();
    assert!(book.read_only);
    assert!(book.sheets[0].cells.contains(&Cell { row: 0, col: 0, text: s("Item") }), "{:?}", book.sheets[0].cells);
    assert!(String::from_utf8(spreadsheet_values_csv(xls, s("xls"), 0).unwrap()).unwrap().starts_with("Item,"));
}

#[test]
fn presentations_give_outlines_and_convert() {
    let fixture = include_bytes!("../../../../web/test/fixtures/sample.pptx").to_vec();
    let slides = presentation_outline(fixture.clone(), s("pptx")).unwrap();
    assert!(!slides.is_empty());
    let odp = presentation_convert(fixture, s("pptx"), s("odp")).unwrap();
    let again = presentation_outline(odp.clone(), s("odp")).unwrap();
    assert_eq!(again.len(), slides.len());
    assert!(presentation_convert(odp, s("odp"), s("pptx")).unwrap().starts_with(b"PK"));

    let titled = Slide { title: s("T"), body: s(""), notes: s(""), hidden: false, objects: vec![] };
    assert_eq!(slide_title_object(titled), None);
    let text = |role: Option<&str>, text: &str| SlideObject { kind: s("Text"), role: role.map(s), text: s(text), cells: vec![] };
    let placeholders = Slide {
        title: s(""),
        body: s(""),
        notes: s(""),
        hidden: false,
        objects: vec![text(Some("body"), "b"), text(Some("title"), "Title")],
    };
    assert_eq!(slide_title_object(placeholders), Some(1));
    let plain = Slide { title: s(""), body: s(""), notes: s(""), hidden: false, objects: vec![text(None, "First")] };
    assert_eq!(slide_title_object(plain), Some(0));
}

#[test]
fn labels_codes_marks_and_pdf_info() {
    assert!(barcode_png(s("qr"), s("https://off42.cc"), 30.0).unwrap().starts_with(b"\x89PNG"));
    assert!(barcode_svg(s("code128"), s("OFF42-001"), 50.0, true).unwrap().starts_with("<svg"));
    assert!(barcode_png(s("bogus"), s("x"), 30.0).is_err());
    let stocks = label_stocks().unwrap();
    assert!(stocks.len() > 100 && stocks[0].columns > 0);
    let labels = parse_label_lines(s("Box 1 | BOX-1 | 3\n\n  Plain  \nOdd | | x"));
    assert_eq!(labels.len(), 3);
    assert_eq!(labels[0], LabelEntry { text: s("Box 1"), code: Some(s("BOX-1")), copies: 3 });
    assert_eq!(labels[1], LabelEntry { text: s("Plain"), code: None, copies: 1 });
    assert_eq!(labels[2].code, None);
    let request = LabelSheetRequest {
        stock: stocks[0].id.clone(),
        width: 0.0,
        height: 0.0,
        gap_x: 0.0,
        gap_y: 0.0,
        first_slot: 1,
        code_format: Some(s("qr")),
        labels,
    };
    let pdf = label_sheet_pdf(request.clone()).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let info = pdf_info(pdf).unwrap();
    assert!(!info.encrypted);
    assert_eq!(info.pages, 1);
    assert!(label_sheet_pdf(LabelSheetRequest { stock: s("nope"), ..request.clone() }).is_err());
    let plain = vec![LabelEntry { text: s("Hello"), code: None, copies: 1 }];
    let custom =
        LabelSheetRequest { stock: s("custom"), width: 70.0, height: 37.0, code_format: None, labels: plain, ..request };
    assert!(label_sheet_pdf(custom).unwrap().starts_with(b"%PDF"));
    assert!(pdf_info(b"%PDF-broken".to_vec()).is_err());

    let marks = product_marks(s("de")).unwrap();
    let ce = marks.iter().find(|m| m.id == "ce").expect("CE mark");
    assert!(ce.name.starts_with("CE"));
    assert!(product_mark_svg(s("ce"), 20.0).unwrap().starts_with("<svg"));
    assert!(product_mark_png(s("ce"), 20.0).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn signatures_and_stamps_are_deterministic() {
    let options = signature_options().unwrap();
    assert!(options.forms.len() >= 5 && options.pens.len() >= 4);
    assert!(options.inks.iter().any(|i| i.value == "blue"));
    let designs = signature_designs(3, 0, 12345).unwrap();
    assert_eq!(designs.len(), 3);
    assert_eq!(designs, signature_designs(3, 0, 12345).unwrap());
    let a = signature_png(s("Anna Smith"), designs[0], 7, s("blue"), 0, 130.0).unwrap();
    assert!(a.starts_with(b"\x89PNG"));
    assert_eq!(a, signature_png(s("Anna Smith"), designs[0], 7, s("blue"), 0, 130.0).unwrap());
    assert!(matches!(signature_png(s(" "), designs[0], 7, s("blue"), 0, 130.0), Err(Off42Error::InvalidInput { .. })));
    assert!(matches!(signature_png(s("A"), designs[0], 7, s("gold"), 0, 130.0), Err(Off42Error::InvalidInput { .. })));

    let templates = stamp_templates(s("en")).unwrap();
    assert!(templates.len() > 50);
    let template = &templates[0];
    let values: HashMap<String, String> =
        template.fields.iter().filter(|f| f.date).map(|f| (f.key.clone(), s("26/11/2026"))).collect();
    let stamp = stamp_png(template.id.clone(), values.clone(), s(""), 99, 1.0).unwrap();
    assert!(stamp.starts_with(b"\x89PNG"));
    assert_eq!(stamp, stamp_png(template.id.clone(), values, s(""), 99, 1.0).unwrap());
    assert!(matches!(stamp_png(s("nope"), HashMap::new(), s(""), 1, 1.0), Err(Off42Error::InvalidInput { .. })));
}

/// Writes the Android instrumented-test fixtures (one file per format the app opens, made by the engine from the
/// web app's fixtures). XLS cannot be written by the engine; android/README.md says how sample.xls was made.
/// `OFF42_FIXTURES=android/app/src/androidTest/assets/fixtures cargo test -p off42-mobile -- --ignored fixtures`
#[test]
#[ignore]
fn write_android_fixtures() {
    let dir = std::path::PathBuf::from(std::env::var("OFF42_FIXTURES").expect("OFF42_FIXTURES"));
    std::fs::create_dir_all(&dir).unwrap();
    let web = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../web/test/fixtures");
    let md = std::fs::read(web.join("sample.md")).unwrap();
    let csv = std::fs::read(web.join("sample.csv")).unwrap();
    let pptx = std::fs::read(web.join("sample.pptx")).unwrap();
    let write = |name: &str, bytes: Vec<u8>| std::fs::write(dir.join(name), bytes).unwrap();
    write("sample.md", md.clone());
    for to in ["docx", "odt", "rtf", "html"] {
        write(&format!("sample.{to}"), document_convert(md.clone(), s("md"), s(to), true).unwrap());
    }
    write("sample.txt", b"Plain text notes\n\nThe OFF42 engine opens plain text too.\n".to_vec());
    write("sample.csv", csv.clone());
    write("sample.xlsx", spreadsheet_convert(csv.clone(), s("csv"), s("xlsx")).unwrap());
    write("sample.ods", spreadsheet_convert(csv, s("csv"), s("ods")).unwrap());
    write("sample.pptx", pptx.clone());
    write("sample.odp", presentation_convert(pptx, s("pptx"), s("odp")).unwrap());
    let stock = label_stocks().unwrap()[0].id.clone();
    let labels = parse_label_lines(s("Box 1 | BOX-1 | 4\nBox 2 | BOX-2 | 4\nShelf A"));
    let request = LabelSheetRequest {
        stock,
        width: 0.0,
        height: 0.0,
        gap_x: 0.0,
        gap_y: 0.0,
        first_slot: 1,
        code_format: Some(s("qr")),
        labels,
    };
    write("sample.pdf", label_sheet_pdf(request).unwrap());
}

#[test]
fn errors_classify_core_messages() {
    assert!(matches!(Off42Error::classify(s("Unsupported document format: pdf")), Off42Error::UnsupportedFormat { .. }));
    assert!(matches!(Off42Error::classify(s("Enter a name.")), Off42Error::InvalidInput { .. }));
    assert!(matches!(Off42Error::classify(s("zip error")), Off42Error::Failed { .. }));
    assert!(matches!(Off42Error::classify(s("invalid document: bad zip")), Off42Error::Failed { .. }));
    assert!(matches!(Off42Error::classify(s("Label 1: Select a barcode format.")), Off42Error::InvalidInput { .. }));
    assert!(matches!(Off42Error::classify(s("Unknown ink: gold")), Off42Error::InvalidInput { .. }));
    assert_eq!(Off42Error::Failed { message: s("m") }.to_string(), "m");
}
