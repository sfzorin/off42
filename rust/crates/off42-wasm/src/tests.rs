use super::*;

const MARKDOWN: &[u8] = b"# Report\n\nHello **world** from off42.\n\n- one\n- two\n";

#[test]
fn documents_convert_between_formats_and_preview_as_html() {
    let docx = document_convert(MARKDOWN, "md", "docx", false).unwrap();
    assert!(docx.starts_with(b"PK"));
    let html = document_html(&docx, "docx").unwrap();
    assert!(html.contains("Report") && html.contains("world"), "{html}");
    assert!(document_convert(&docx, "docx", "odt", false).is_err(), "the core keeps Word parts it cannot convert");
    assert!(document_convert(&docx, "docx", "odt", true).unwrap().starts_with(b"PK"));
    let odt = document_convert(MARKDOWN, "md", "odt", false).unwrap();
    let rtf = document_convert(&odt, "odt", "rtf", true).unwrap();
    assert!(rtf.starts_with(b"{\\rtf"));
    let text = String::from_utf8(document_convert(b"Hello world from off42.", "txt", "txt", false).unwrap()).unwrap();
    assert!(text.contains("Hello world from off42."), "{text}");
    let info: Value = serde_json::from_str(&document_info(MARKDOWN, "md").unwrap()).unwrap();
    assert!(info["words"].as_u64().unwrap() >= 6);
    assert!(document_convert(MARKDOWN, "md", "pdf", false).is_err());
}

#[test]
fn spreadsheets_show_calculated_values_and_convert() {
    let csv = b"Item,Price\nTea,2\nCake,3\nTotal,=SUM(B2:B3)\n";
    let shown: Value = serde_json::from_str(&spreadsheet_json(csv, "csv", 1000, 100).unwrap()).unwrap();
    let cells = shown["sheets"][0]["cells"].as_array().unwrap();
    assert!(cells.iter().any(|c| c[0] == 3 && c[1] == 1 && c[2] == "5"), "{cells:?}");
    let xlsx = spreadsheet_convert(csv, "csv", "xlsx").unwrap();
    let reopened: Value = serde_json::from_str(&spreadsheet_json(&xlsx, "xlsx", 1000, 100).unwrap()).unwrap();
    let cells = reopened["sheets"][0]["cells"].as_array().unwrap();
    assert!(cells.iter().any(|c| c[0] == 3 && c[1] == 1 && c[2] == "5"), "{cells:?}");
    let values = String::from_utf8(spreadsheet_values_csv(&xlsx, "xlsx", 0).unwrap()).unwrap();
    assert_eq!(values.lines().nth(3), Some("Total,5"));
    let ods = spreadsheet_convert(csv, "csv", "ods").unwrap();
    assert!(ods.starts_with(b"PK"));
    assert_eq!(spreadsheet_convert(csv, "csv", "csv").unwrap(), b"Item,Price\nTea,2\nCake,3\nTotal,=SUM(B2:B3)\n");
    let small: Value = serde_json::from_str(&spreadsheet_json(csv, "csv", 2, 1).unwrap()).unwrap();
    assert_eq!(small["sheets"][0]["truncated"], true);
}

#[test]
fn presentations_give_outlines_and_convert() {
    let mut deck = Presentation::new();
    deck.edit("Quarterly review".into(), "Revenue up\nCosts down".into()).unwrap();
    let pptx_bytes = pptx::export_new(&deck).unwrap();
    let outline: Value = serde_json::from_str(&presentation_outline(&pptx_bytes, "pptx").unwrap()).unwrap();
    assert!(outline.to_string().contains("Quarterly review"), "{outline}");
    let odp_bytes = presentation_convert(&pptx_bytes, "pptx", "odp").unwrap();
    let outline: Value = serde_json::from_str(&presentation_outline(&odp_bytes, "odp").unwrap()).unwrap();
    assert!(outline.to_string().contains("Quarterly review"), "{outline}");
    assert!(presentation_convert(&odp_bytes, "odp", "pptx").unwrap().starts_with(b"PK"));
}

#[test]
fn codes_labels_and_marks() {
    assert!(barcode_png("qr", "https://off42.cc", 30.0).unwrap().starts_with(b"\x89PNG"));
    assert!(barcode_svg("code128", "OFF42-001", 50.0, true).unwrap().starts_with("<svg"));
    let stocks: Value = serde_json::from_str(&label_stocks().unwrap()).unwrap();
    let id = stocks[0]["id"].as_str().unwrap();
    assert!(label_sheet_pdf(&json!({"stock": id, "firstSlot": 0, "labels": [{"text": "x"}]}).to_string()).is_err());
    let request = json!({"stock": id, "codeFormat": "qr", "labels": [{"text": "Box 1", "code": "BOX-1", "copies": 3}]});
    let pdf = label_sheet_pdf(&request.to_string()).unwrap();
    assert!(pdf.starts_with(b"%PDF"));
    let info: Value = serde_json::from_str(&pdf_info(&pdf).unwrap()).unwrap();
    assert_eq!(info["pages"], 1);
    let custom = json!({"stock": "custom", "width": 70, "height": 37, "labels": [{"text": "Hello"}]});
    assert!(label_sheet_pdf(&custom.to_string()).unwrap().starts_with(b"%PDF"));
    let marks: Value = serde_json::from_str(&product_marks("en").unwrap()).unwrap();
    let ce = marks.as_array().unwrap().iter().find(|m| m["id"] == "ce").expect("CE mark");
    assert!(ce["name"].as_str().unwrap().starts_with("CE"));
    assert!(product_mark_svg("ce", 20.0).unwrap().starts_with("<svg"));
    assert!(product_mark_png("ce", 20.0).unwrap().starts_with(b"\x89PNG"));
}

#[test]
fn signatures_and_stamps_render_deterministically() {
    let options: Value = serde_json::from_str(&signature_options()).unwrap();
    assert_eq!(options["forms"].as_array().unwrap().len(), SignatureForm::ALL.len());
    let designs: Vec<String> = serde_json::from_str(&signature_designs(3, 0, "12345").unwrap()).unwrap();
    assert_eq!(designs.len(), 3);
    let a = signature_png("Anna Smith", &designs[0], "7", "blue", 0, 130.0).unwrap();
    let b = signature_png("Anna Smith", &designs[0], "7", "blue", 0, 130.0).unwrap();
    assert!(a.starts_with(b"\x89PNG"));
    assert_eq!(a, b);
    let templates: Value = serde_json::from_str(&stamp_templates("en")).unwrap();
    let russian: Value = serde_json::from_str(&stamp_templates("ru")).unwrap();
    assert_ne!(templates[0]["title"], russian[0]["title"], "template titles follow the language");
    let fallback: Value = serde_json::from_str(&stamp_templates("ja")).unwrap();
    assert_eq!(templates[0]["title"], fallback[0]["title"], "English for languages marks42 has no titles in");
    let template = &templates[0];
    let mut values = serde_json::Map::new();
    for field in template["fields"].as_array().unwrap() {
        if field["date"] == true {
            values.insert(field["key"].as_str().unwrap().into(), "26/11/2026".into());
        }
    }
    let id = template["id"].as_str().unwrap();
    let stamp = stamp_png(id, &Value::Object(values).to_string(), "", "99", 1.0).unwrap();
    assert!(stamp.starts_with(b"\x89PNG"));
}
