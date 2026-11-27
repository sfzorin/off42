//! Batches (one Undo step for several edits), style patches, currency formats and reading inactive sheets.
use super::*;
fn cell(r: usize, c: usize) -> Range {
    Range { top: r, left: c, bottom: r, right: c }
}
#[test]
fn batch_is_one_undo_step_and_a_failed_batch_changes_nothing() {
    let mut b = Workbook::new(4, 4).unwrap();
    b.set_history_limit(2);
    b.batch(|w| {
        for r in 0..4 {
            w.set_cell(r, 0, (r + 1).to_string())?;
        }
        w.set_cell(0, 1, "=SUM(A1:A4)".into())?;
        w.insert_rows(0, 1)
    })
    .unwrap();
    assert_eq!(b.cell(0, 1), Some(""));
    assert_eq!(b.cell(1, 1), Some("=SUM(A2:A5)"));
    assert_eq!(b.display_cells()[&(1, 1)], "10");
    assert!(b.undo());
    assert!(b.display_cells().is_empty(), "the whole batch is undone at once");
    assert!(!b.can_undo());
    assert!(b.redo());
    assert_eq!(b.display_cells()[&(1, 1)], "10");
    let before = b.display_cells();
    assert!(b
        .batch(|w| {
            w.set_cell(1, 0, "99".into())?;
            w.set_cell(40, 0, "out of bounds".into())
        })
        .is_err());
    assert_eq!(b.display_cells(), before);
    assert!(b.can_undo() && !b.can_redo());
    // History deeper than the default 8 steps.
    b.set_history_limit(100);
    for n in 0..30 {
        b.set_cell(3, 3, n.to_string()).unwrap();
    }
    let mut steps = 0;
    while b.undo() {
        steps += 1;
    }
    assert_eq!(steps, 31);
}
#[test]
fn a_batch_on_an_opened_office_workbook_keeps_its_source() {
    let mut b = Workbook::from_office_bytes(include_bytes!("../../../../web/test/fixtures/sheets.xlsx"), "xlsx").unwrap();
    let external = b.has_external_source();
    b.batch(|w| {
        w.set_cell(1, 0, "2".into())?;
        w.set_cell(1, 1, "=A2*3".into())
    })
    .unwrap();
    assert_eq!(b.has_external_source(), external, "the original package stays attached");
    let reopened = Workbook::from_office_bytes(&b.xlsx_bytes().unwrap(), "xlsx").unwrap();
    assert_eq!(reopened.display_cells()[&(1, 1)], "6");
    assert!(b.batch(|w| w.set_cell(9, 9, "x".into())).is_err());
    assert_eq!(b.has_external_source(), external, "kept after a failed batch too");
}
#[test]
fn update_style_patches_one_property_and_currency_formats_round_trip() {
    let mut b = Workbook::new(2, 2).unwrap();
    b.set_cell(0, 0, "1234.5".into()).unwrap();
    b.set_cell(1, 0, "-2".into()).unwrap();
    b.set_style(cell(0, 0), CellStyle { italic: true, ..Default::default() }).unwrap();
    let all = Range { top: 0, left: 0, bottom: 1, right: 0 };
    b.update_style(all, |s| CellStyle { bold: true, ..s.clone() }).unwrap();
    assert!(b.style(0, 0).unwrap().bold && b.style(0, 0).unwrap().italic);
    assert!(b.style(1, 0).unwrap().bold && !b.style(1, 0).unwrap().italic);
    for (code, top, bottom) in [
        ("\"$\"#,##0.00", "$1,234.50", "-$2.00"),
        ("$#,##0.00", "$1,234.50", "-$2.00"),
        ("#,##0.00 \"€\"", "1,234.50 €", "-2.00 €"),
        ("#,##0.00\\ \"₽\"", "1,234.50 ₽", "-2.00 ₽"),
        ("\"R$\" 0.0", "R$ 1234.5", "-R$ 2.0"),
    ] {
        b.update_style(all, |s| CellStyle { number_format: code.into(), ..s.clone() }).unwrap();
        assert_eq!(b.display_cells()[&(0, 0)], top, "{code}");
        assert_eq!(b.display_cells()[&(1, 0)], bottom, "{code}");
        for kind in ["xlsx", "ods"] {
            let bytes = if kind == "xlsx" { b.xlsx_bytes() } else { b.ods_bytes() }.unwrap();
            let reopened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(reopened.display_cells()[&(0, 0)], top, "{code} via {kind}");
            assert!(reopened.style(0, 0).unwrap().bold, "{kind}");
        }
    }
    assert!(display::NumberPattern::parse("\"€\"0%").is_none(), "no currency percentages");
    assert!(display::NumberPattern::parse("\"XYZ\"0.00").is_none(), "unknown symbols stay unsupported");
}
#[test]
fn with_sheet_reads_another_sheet_without_selecting_it() {
    let mut b = Workbook::new(2, 2).unwrap();
    b.set_cell(0, 0, "first".into()).unwrap();
    let second = b.add_sheet("Two").unwrap();
    b.select_sheet(second).unwrap();
    b.set_cell(1, 1, "second".into()).unwrap();
    let first = b.sheets()[0].0;
    b.select_sheet(first).unwrap();
    let undo_before = b.can_undo();
    let cells = b.with_sheet(second, |w| w.display_cells()).unwrap();
    assert_eq!(cells[&(1, 1)], "second");
    assert_eq!(b.active_sheet(), first);
    assert_eq!(b.can_undo(), undo_before);
    assert!(b.with_sheet(999, |_| ()).is_err());
}
