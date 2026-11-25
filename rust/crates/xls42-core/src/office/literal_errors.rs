use super::*;
#[test]
fn generated_error_inputs_are_standard_typed_cells_and_stay_distinct_from_literal_text() {
    let errors = [
        "#DIV/0!",
        "#N/A",
        "#NAME?",
        "#NULL!",
        "#NUM!",
        "#REF!",
        "#VALUE!",
        "#GETTING_DATA",
        "#SPILL!",
        "#CALC!",
        "#FIELD!",
        "#BLOCKED!",
        "#CONNECT!",
        "#UNKNOWN!",
        "#BUSY!",
        "#PYTHON!",
    ];
    let mut b = Workbook::new(8, 20).unwrap();
    for (c, &error) in errors.iter().enumerate() {
        b.set_content(0, c, CellContent::Error(TextValue::from_raw(error.into())))
            .unwrap();
        b.set_content(1, c, CellContent::Text(TextValue::from_raw(error.into())))
            .unwrap();
        b.set_cell(2, c, format!("=ISERROR({}1)", col_name(c)))
            .unwrap();
        b.set_cell(3, c, format!("=ISERROR({}2)", col_name(c)))
            .unwrap();
        b.set_cell(4,c,format!("={}1",col_name(c))).unwrap();
    }
    b.set_style(Range{top:0,bottom:0,left:0,right:0},CellStyle{bold:true,foreground:Some(0x123456),..Default::default()}).unwrap();
    for (kind, bytes) in [
        ("xlsx", b.xlsx_bytes().unwrap()),
        ("ods", b.ods_bytes().unwrap()),
    ] {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes.as_slice())).unwrap();
        let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            if entry.name() == EXTRA {
                continue;
            }
            if matches!(
                entry.name(),
                "[Content_Types].xml" | "META-INF/manifest.xml"
            ) {
                let mut declaration = Vec::new();
                entry.read_to_end(&mut declaration).unwrap();
                let name = entry.name().to_owned();
                out.start_file(name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                out.write_all(&project_declaration(&declaration, kind, false).unwrap())
                    .unwrap();
            } else {
                out.raw_copy_file(entry).unwrap();
            }
        }
        let independent = out.finish().unwrap().into_inner();
        let opened = Workbook::from_office_bytes(&independent, kind).unwrap();
        assert!(opened.style(0,0).unwrap().bold);assert_eq!(opened.style(0,0).unwrap().foreground,Some(0x123456));
        for (c, &error) in errors.iter().enumerate() {
            assert!(
                matches!(opened.content(0,c),Some(CellContent::Error(v))if v.raw()==error),
                "{kind} error {error}: {:?}",
                opened.content(0, c)
            );
            assert!(
                matches!(opened.content(1,c),Some(CellContent::Text(v))if v.raw()==error),
                "{kind} text {error}"
            );
            assert_eq!(opened.calculated_cells()[&(2, c)], "TRUE");
            assert_eq!(opened.calculated_cells()[&(3, c)], "FALSE");
            assert_eq!(opened.calculated_cells()[&(4,c)],error);
        }
    }
    let mut unsupported = Workbook::new(1, 1).unwrap();
    unsupported
        .set_content(
            0,
            0,
            CellContent::Error(TextValue::from_raw("not an Excel error".into())),
        )
        .unwrap();
    assert!(unsupported.xlsx_bytes().is_err());
}
