//! Reopen complete Office packages without the native snapshot, edit inputs and export real external caches.
use std::{
    fs,
    io::{Cursor, Write},
    path::PathBuf,
};
use xls42_core::Workbook;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().ok_or("input directory required")?);
    let output = PathBuf::from(args.next().ok_or("output directory required")?);
    fs::create_dir_all(&output)?;
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(input.join("cases.json"))?)?;
    let mut expected = None;
    for kind in ["xlsx", "ods"] {
        let source = fs::read(input.join(format!("statistics.{kind}")))?;
        let mut archive = zip::ZipArchive::new(Cursor::new(source))?;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            if file.name() != "xls42/project.json" {
                writer.raw_copy_file(file)?;
            }
        }
        let raw = writer.finish()?.into_inner();
        let mut book = Workbook::from_office_bytes(&raw, kind)?;
        book.set_cell(0, 0, "4".into())?;
        book.set_cell(3, 5, "keep".into())?;
        let grid = book.calculated_cells();
        let mut updated = cases.clone();
        for (i, c) in updated.iter_mut().enumerate() {
            c["expected"] = grid.get(&(i,2)).cloned().unwrap_or_default().into();
        }
        if let Some(prior) = &expected {
            assert_eq!(
                prior, &updated,
                "native formula results after external input edits differ by format"
            );
        } else {
            expected = Some(updated);
        }
        let bytes = if kind == "xlsx" {
            book.xlsx_bytes()?
        } else {
            book.ods_bytes()?
        };
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(output.join(format!("statistics.{kind}")))?
            .write_all(&bytes)?;
    }
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(output.join("cases.json"))?
        .write_all(&serde_json::to_vec_pretty(&expected.unwrap())?)?;
    Ok(())
}
