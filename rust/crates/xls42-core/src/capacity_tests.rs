use super::*;
fn full() -> Range {
    Range {
        top: 0,
        left: 0,
        bottom: MAX_ROWS - 1,
        right: MAX_COLS - 1,
    }
}
#[test]
fn last_excel_cell_and_formula_roundtrip_all_native_formats_without_dense_area() {
    let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
    assert!(b.display_cells().is_empty());
    b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "7".into()).unwrap();
    b.set_cell(0, 0, "=XFD1048576*3".into()).unwrap();
    assert_eq!(
        b.calculated_cells(),
        BTreeMap::from([
            ((0, 0), "21".into()),
            ((MAX_ROWS - 1, MAX_COLS - 1), "7".into())
        ])
    );
    assert_eq!(
        b.copy_tsv(Range {
            top: MAX_ROWS - 1,
            bottom: MAX_ROWS - 1,
            left: MAX_COLS - 1,
            right: MAX_COLS - 1
        })
        .unwrap(),
        "7"
    );
    for (kind, bytes) in [
        ("xls42", b.project_bytes().unwrap()),
        ("xlsx", b.xlsx_bytes().unwrap()),
        ("ods", b.ods_bytes().unwrap()),
    ] {
        assert!(
            bytes.len() < 4 * 1024 * 1024,
            "Sparse {kind} unnecessarily expanded"
        );
        let opened = if kind == "xls42" {
            Workbook::from_project_bytes(&bytes)
        } else {
            Workbook::from_office_bytes(&bytes, kind)
        }
        .unwrap();
        assert_eq!((opened.rows(), opened.cols()), (MAX_ROWS, MAX_COLS));
        assert_eq!(opened.calculated_cells(), b.calculated_cells());
    }
    b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "11".into()).unwrap();
    assert_eq!(b.calculated_cells()[&(0, 0)], "33");
    assert!(b.undo());
    assert_eq!(b.calculated_cells()[&(0, 0)], "21");
    assert!(b.redo());
    assert_eq!(b.calculated_cells()[&(0, 0)], "33");
    let prior = b.project_bytes().unwrap();
    assert!(matches!(
        b.set_cell(MAX_ROWS, 0, "bad".into()),
        Err(Error::Bounds)
    ));
    assert!(matches!(
        b.set_cell(0, MAX_COLS, "bad".into()),
        Err(Error::Bounds)
    ));
    assert_eq!(b.project_bytes().unwrap(), prior);
}
#[test]
fn oversized_rectangles_refuse_atomically_while_far_sparse_edits_remain_available() {
    let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
    b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "7".into()).unwrap();
    let before = b.project_bytes().unwrap();
    assert!(matches!(b.copy_tsv(full()), Err(Error::Limit)));
    assert!(matches!(b.csv_bytes(), Err(Error::Limit)));
    assert!(matches!(
        b.set_style(full(), CellStyle::default()),
        Err(Error::Limit)
    ));
    assert!(matches!(b.copy_range(full(), 0, 0), Err(Error::Limit)));
    assert!(matches!(
        b.fill_range(
            Range {
                top: 0,
                bottom: 0,
                left: 0,
                right: 0
            },
            full()
        ),
        Err(Error::Limit)
    ));
    // Sorting an all-blank key is a sparse no-op, independently of rectangular export/fill limits.
    assert!(!b.sort_range(full(), 0, false).unwrap());
    let text = ("\t".repeat(199) + "\n").repeat(501);
    assert!(matches!(b.paste_tsv(0, 0, &text), Err(Error::Limit)));
    assert_eq!(b.project_bytes().unwrap(), before);
    b.paste_tsv(MAX_ROWS - 1, MAX_COLS - 1, "12").unwrap();
    assert_eq!(b.cell(MAX_ROWS - 1, MAX_COLS - 1), Some("12"));
    assert!(b.undo());
    assert_eq!(b.project_bytes().unwrap(), before);
}
#[test]
fn full_sheet_validation_visits_sparse_values_and_preserves_implicit_blank_rules() {
    let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
    b.add_validation(DataValidation {
        range: full(),
        allow_blank: true,
        kind: ValidationKind::Number {
            minimum: 0.,
            maximum: 10.,
        },
    })
    .unwrap();
    b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "10".into()).unwrap();
    let before = b.project_bytes().unwrap();
    assert!(b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "11".into()).is_err());
    assert_eq!(b.project_bytes().unwrap(), before);
    b.undo();
    assert_eq!(b.cell(MAX_ROWS - 1, MAX_COLS - 1), Some(""));
    b.redo();
    assert_eq!(b.cell(MAX_ROWS - 1, MAX_COLS - 1), Some("10"));
    let mut b = Workbook::new(MAX_ROWS, MAX_COLS).unwrap();
    b.add_validation(DataValidation {
        range: full(),
        allow_blank: false,
        kind: ValidationKind::List {
            values: vec![String::new(), "OK".into()],
        },
    })
    .unwrap();
    b.set_cell(MAX_ROWS - 1, MAX_COLS - 1, "OK".into()).unwrap();
    let mut b=Workbook::new(MAX_ROWS,MAX_COLS).unwrap();
    // Creating validation allows preexisting invalid values, as before; future edits must obey it.
    b.add_validation(DataValidation{range:full(),allow_blank:false,kind:ValidationKind::Number{minimum:0.,maximum:10.}}).unwrap();
    let before=b.project_bytes().unwrap();
    assert!(rules::validate_range(b.history.current(),full()).is_err());
    assert!(b.set_cell(MAX_ROWS-1,MAX_COLS-1,String::new()).is_err());
    assert_eq!(b.project_bytes().unwrap(),before);
}
