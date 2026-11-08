//! Independent Office recalculation fixtures; the companion JSON contains reference outputs.
use std::{fs, io::Write, path::PathBuf};
use xls42_core::Workbook;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    let mut book = Workbook::new(120, 8)?;
    for (i, v) in ["1", "2", "text", "TRUE", ""].into_iter().enumerate() {
        book.set_cell(i, 0, v.into())?;
        book.set_cell(i, 1, (2 * (i + 1)).to_string())?;
    }
    for (i, value) in [
        "2024-12-25",
        "2025-01-01",
        "-100",
        "0",
        "121",
        "2021-01-01",
        "2022-01-01",
        "2023-01-01",
    ]
    .into_iter()
    .enumerate()
    {
        book.set_cell(i, 3, value.into())?;
    }
    for (r, (tag, n)) in [
        ("Tag", 0),
        ("keep", 1),
        ("keep", 2),
        ("skip", 3),
        ("keep", 4),
    ]
    .into_iter()
    .enumerate()
    {
        book.set_cell(r, 5, tag.into())?;
        book.set_cell(r, 6, n.to_string())?;
    }
    let mut view = book.sheet_view().clone();
    view.hidden_rows.insert(2);
    book.set_sheet_view(view)?;
    book.set_filter(Some(xls42_core::SheetFilter {
        range: xls42_core::Range {
            top: 0,
            left: 5,
            bottom: 4,
            right: 6,
        },
        column: 5,
        contains: "keep".into(),
    }))?;
    let formulas = [
        "=STDEV.S(A1:A5)",
        "=AVERAGEA(A1:A5)",
        "=COUNTBLANK(A1:A5)",
        "=CORREL(A1:A5,B1:B5)",
        "=NORM.DIST(A1,0,1,TRUE)",
        "=BETA.INV(0.5,2,2)",
        "=T.DIST.2T(A1,10)",
        "=VAR.S(A1:A5)",
        "=NORM.S.INV(1e-100)",
        "=T.DIST.RT(20,100)",
        "=CHISQ.INV.RT(1e-20,2)",
        "=ERF(8,9)",
        "=F.INV.RT(1e-20,5,8)",
        "=PERCENTILE.EXC(A1:A2,0.5)",
        "=PERCENTILE.EXC(A1:A2,0)",
        "=F.DIST(1,5,8,TRUE)",
        "=FDIST(1,5,8)",
        "=F.INV(0.75,5,8)",
        "=FINV(0.75,5,8)",
        "=SKEW.P(A1:A2)",
        "=PV(.1/12,12,-100*A1)",
        "=FV(.06/12,10,-200,-500,1)",
        "=NPER(.12/12,-100,-1000,10000,1)",
        "=RATE(12,-100*A1,1000)",
        "=IPMT(.1/12,1,36,8000)",
        "=PPMT(.1/12,1,24,2000)",
        "=CUMIPMT(.1/12,36,1000*A1,1,12,0)",
        "=CUMPRINC(.1/12,36,1000,1,12,0)",
        "=IRR(D3:D5)",
        "=XIRR(D3:D5,D6:D8)",
        "=XNPV(0,D3:D5,D6:D8)",
        "=MIRR(D3:D5,.1,.12)",
        "=DDB(2400,300,10,10)",
        "=DB(1000000,100000,6,7,7)",
        "=EFFECT(.0525,4)",
        "=NOMINAL(.053543,4)",
        "=PDURATION(.025,2000,2200)",
        "=RRI(96,10000,11000)",
        "=WORKDAY(DATE(2024,12,20),A1,D1:D2)",
        "=NETWORKDAYS(DATE(2024,12,20),DATE(2024,12,31),D1:D2)",
        "=EDATE(DATE(2024,1,31),A1)",
        "=WEEKNUM(DATE(2024,12,31),21)",
        "=YEARFRAC(DATE(2023,3,1),DATE(2025,2,28),1)",
        "=DATEDIF(DATE(2020,2,29),DATE(2024,2,28),\"M\")",
        "=NETWORKDAYS.INTL(DATE(2024,1,1),DATE(2024,1,31),\"0000110\")",
        "=DAYS360(DATE(2024,2,29),DATE(2024,3,31),TRUE)",
        "=IFNA(NA(),\"fallback\")",
        "=IFS(A1>2,\"large\",TRUE,\"small\")",
        "=SWITCH(A1,1,\"one\",4,\"four\",\"other\")",
        "=CHOOSE(2,\"first\",\"second\")",
        "=MAXIFS(B1:B5,A1:A5,\">0\")",
        "=MINIFS(B1:B5,A1:A5,\">0\")",
        "=ISNA(NA())",
        "=ISERR(NA())",
        "=ERROR.TYPE(1/0)",
        "=TYPE(A4)",
        "=ISFORMULA(C1)",
        "=ISREF(B1)",
        "=ADDRESS(2,3)",
        "=CELL(\"address\",B3)",
        "=CELL(\"contents\",A1)",
        "=ROWS(A1:B5)",
        "=SHEETS()",
        "=AND(RAND()>=0,RAND()<1)",
        "=AND(RANDBETWEEN(1,6)>=1,RANDBETWEEN(1,6)<=6)",
        "=OFFSET(A1,1,0)",
        "=SUM(OFFSET(A1,0,0,2,1))",
        "=SUM(INDEX(A1:B5,0,2))",
        "=SUM(INDEX(A1:B5,2,0))",
        "=INDIRECT(\"A\"&2)",
        "=SUM(INDIRECT(\"A1:A2\"))",
        "=INDIRECT(\"R2C1\",FALSE)",
        "=INDIRECT(\"R1C[-2]\",FALSE)",
        "=ISREF(OFFSET(A1,1,1))",
        "=CELL(\"address\",OFFSET(A1,1,1))",
        "=XMATCH(6,B1:B5)",
        "=XMATCH(5,B1:B5,1)",
        "=XMATCH(5,B1:B5,-1)",
        "=HLOOKUP(2,A1:B2,2,FALSE)",
        "=SUBTOTAL(9,G2:G5)",
        "=SUBTOTAL(109,G2:G5)",
        "=SUBTOTAL(1,G2:G5)",
        "=AGGREGATE(9,1,G2:G5)",
        "=AGGREGATE(14,1,G2:G5,2)",
        "=SUM(OFFSET(G2,0,0,4,1))",
    ];
    for (i, formula) in formulas.iter().enumerate() {
        book.set_cell(i, 2, formula.to_string())?;
    }
    let grid = book.calculated_cells();
    let cases=formulas.iter().enumerate().map(|(i,formula)|serde_json::json!({"cell":format!("C{}",i+1),"formula":formula,"expected":grid.get(&(i,2)).map_or("",String::as_str)})).collect::<Vec<_>>();
    let json = serde_json::to_vec_pretty(&cases)?;
    for (name, data) in [
        ("statistics.xlsx", book.xlsx_bytes()?),
        ("statistics.ods", book.ods_bytes()?),
        ("cases.json", json),
    ] {
        let path = output.join(name);
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&path)?
            .write_all(&data)?;
        println!("{}", path.display());
    }
    Ok(())
}
