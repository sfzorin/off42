//! Standard-file and independent-application fixtures for dynamic arrays and scalar array expressions.
use std::{fs, io::Write, path::PathBuf};
use xls42_core::Workbook;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    let mut b = Workbook::new(60, 24)?;
    for (r, (n, text)) in [("3", "apple"), ("1", "Banana"), ("2", "apple")]
        .into_iter()
        .enumerate()
    {
        b.set_cell(r, 0, n.into())?;
        b.set_cell(r, 1, text.into())?;
    }
    for (r, c, f) in [
        (0, 2, "=SORT(A1:A3)"),
        (0, 3, "=SEQUENCE(2,3)"),
        (4, 3, "=C2*10"),
        (0, 7, "=TRANSPOSE(A1:A3)"),
        (2, 7, "x"),
        (1, 7, "=SEQUENCE(2)"),
        (1, 9, "=IF(A1:A3>1,\"y\",\"n\")"),
        (1, 10, "=IFERROR(1/(A1:A3-1),\"e\")"),
        (1, 11, "=LEN(A1:A3)&\"!\""),
        (0, 12, "=FILTER(A1:A3,A1:A3>1)"),
        (0, 14, "=UNIQUE(B1:B3)"),
        (0, 15, "=SORTBY(B1:B3,A1:A3,-1)"),
        (0, 16, "=WRAPROWS(SEQUENCE(5),3)"),
        (0, 20, "=SUM(MMULT({1,2;3,4},{5;6}))"),
        (1, 20, "=MDETERM({1,2;3,4})"),
        (2, 20, "=INDEX(MINVERSE({4,7;2,6}),1,1)"),
        (3, 20, "=SUM(MUNIT(3))"),
        (4, 20, "=SUMPRODUCT((A1:A3>1)*A1:A3)"),
        (5, 20, "=SUM(IFERROR(1/(A1:A3-1),0))"),
        (6, 20, "=SUM(LEN(B1:B3))"),
        (7, 20, "=LET(x,5,y,x*2,x+y)"),
        (8, 20, "=TEXTJOIN(\",\",TRUE,SORT(UNIQUE(B1:B3)))"),
        (
            24,
            0,
            "=LINEST({10;20;18;25;32;40},{1,3;2,5;3,2;4,4;5,3;6,6},TRUE,TRUE)",
        ),
        (34, 0, "=LINEST({3,5,8,10,13},{1,2,3,4,5},FALSE,TRUE)"),
        (41, 0, "=GROWTH({2;4;8},{1;2;3},{4;5})"),
        (45, 0, "=TREND({3;5;8;10;13},{1;2;3;4;5},{6;7})"),
        (24, 6, r#"=TEXTSPLIT("αXβxγ;δ",{"x","XX"},";",FALSE,1,99)"#),
        (29, 6, r#"=REGEXEXTRACT("a12b34","[0-9]+",1)"#),
        (29, 9, r#"=REGEXEXTRACT("Ж12","(Ж)([0-9]+)",2)"#),
        (10, 20, r#"=REGEX("abc123def","[0-9]+")"#),
        (11, 20, r##"=REGEX("a1b2c3","[0-9]","#","g")"##),
        (12, 20, r#"=REGEX("a1b2c3","[0-9]",,2)"#),
        (13, 20, r#"=REGEX("abc","z")"#),
        (14, 20, r#"=REGEXTEST("Жук","ж",1)"#),
        (15, 20, r#"=REGEXREPLACE("a1b2","([a-z])([0-9])","$2$1")"#),
        (16, 20, r#"=REGEXREPLACE("a1b2c3","[0-9]","X",-2)"#),
        (17, 20, r#"=REGEXREPLACE("αβ","","!")"#),
        (18, 20, r#"=FIXED(1234.567,-2,TRUE)"#),
        (19, 20, r#"=LEN(FIXED(1.005,2,TRUE))"#),
        (20, 20, r#"=ROMAN(1999)"#),
        (21, 20, r#"=ARABIC("-iv")"#),
    ] {
        b.set_cell(r, c, f.into())?;
    }
    let grid = b.calculated_cells();
    let mut cases = Vec::new();
    for (r, c) in [
        (0, 2),
        (1, 2),
        (2, 2),
        (0, 3),
        (0, 4),
        (0, 5),
        (1, 3),
        (1, 4),
        (1, 5),
        (4, 3),
        (0, 7),
        (0, 8),
        (0, 9),
        (1, 7),
        (1, 9),
        (1, 10),
        (1, 11),
        (0, 12),
        (1, 12),
        (0, 14),
        (1, 14),
        (0, 15),
        (1, 15),
        (2, 15),
        (0, 16),
        (0, 17),
        (0, 18),
        (1, 16),
        (1, 17),
        (1, 18),
    ]
    .into_iter()
    .chain((0..9).map(|r| (r, 20)))
    .chain((24..29).flat_map(|r| (0..3).map(move |c| (r, c))))
    .chain((34..39).flat_map(|r| (0..2).map(move |c| (r, c))))
    .chain([(41, 0), (42, 0), (45, 0), (46, 0)])
    .chain((24..26).flat_map(|r| (6..9).map(move |c| (r, c))))
    .chain([(29, 6), (30, 6), (29, 9), (29, 10)])
    .chain((10..22).map(|r| (r, 20)))
    {
        let mut col = c + 1;
        let mut label = String::new();
        while col > 0 {
            col -= 1;
            label.insert(0, (b'A' + (col % 26) as u8) as char);
            col /= 26;
        }
        cases.push(serde_json::json!({"cell":format!("{label}{}",r+1),"expected":grid.get(&(r,c)).map_or("",String::as_str)}));
    }
    let mut csv = csv::Writer::from_writer(Vec::new());
    for r in 0..b.rows() {
        csv.write_record((0..b.cols()).map(|c| b.cell(r, c).unwrap_or("")))?;
    }
    let csv = csv.into_inner()?;
    for (name, bytes) in [
        ("arrays.csv", csv),
        ("arrays.xlsx", b.xlsx_bytes()?),
        ("arrays.ods", b.ods_bytes()?),
        ("cases.json", serde_json::to_vec_pretty(&cases)?),
    ] {
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output.join(name))?
            .write_all(&bytes)?;
    }
    Ok(())
}
