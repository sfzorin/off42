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
    let mut book = Workbook::new(25, 8)?;
    for (i, v) in ["1", "2", "text", "TRUE", ""].into_iter().enumerate() {
        book.set_cell(i, 0, v.into())?;
        book.set_cell(i, 1, (2 * (i + 1)).to_string())?;
    }
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
    ];
    for (i, formula) in formulas.iter().enumerate() {
        book.set_cell(i, 2, formula.to_string())?;
    }
    let grid = book.display_grid();
    let cases=formulas.iter().enumerate().map(|(i,formula)|serde_json::json!({"cell":format!("C{}",i+1),"formula":formula,"expected":grid[i][2]})).collect::<Vec<_>>();
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
