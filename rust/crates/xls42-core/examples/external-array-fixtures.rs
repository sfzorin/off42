//! Exercise standard array imports and resizing without a private project payload.
use std::{
    fs,
    io::{Cursor, Write, Read},
    path::PathBuf,
};
use xls42_core::Workbook;
fn address(text: &str) -> (usize, usize) {
    let mut c = 0;
    let mut split = 0;
    for ch in text.chars() {
        if ch.is_ascii_alphabetic() {
            c = c * 26 + (ch as u8 - b'A' + 1) as usize;
            split += 1;
        } else {
            break;
        }
    }
    (text[split..].parse::<usize>().unwrap() - 1, c - 1)
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let input = PathBuf::from(args.next().ok_or("input directory required")?);
    let output = PathBuf::from(args.next().ok_or("output directory required")?);
    fs::create_dir_all(&output)?;
    let original: Vec<serde_json::Value> =
        serde_json::from_slice(&fs::read(input.join("cases.json"))?)?;
    let mut expected = None;
    for kind in ["xlsx", "ods"] {
        let bytes = fs::read(input.join(format!("arrays.{kind}")))?;
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut item = archive.by_index(i)?;
            if item.name() == "xls42/project.json" {
                continue;
            }
            if matches!(item.name(), "[Content_Types].xml" | "META-INF/manifest.xml") {
                let mut text = String::new();
                item.read_to_string(&mut text)?;
                let doc = roxmltree::Document::parse(&text)?;
                let mut ranges: Vec<_> = doc
                    .descendants()
                    .filter(|n| {
                        n.attributes().any(|a| {
                            matches!(a.name(), "PartName" | "full-path")
                                && a.value().trim_start_matches('/') == "xls42/project.json"
                        })
                    })
                    .map(|n| n.range())
                    .collect();
                ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
                for r in ranges {
                    text.replace_range(r, "");
                }
                writer.start_file(item.name(), zip::write::SimpleFileOptions::default())?;
                writer.write_all(text.as_bytes())?;
            } else {
                writer.raw_copy_file(item)?;
            }
        }
        writer.start_file("foreign-keep.bin", zip::write::SimpleFileOptions::default())?;
        writer.write_all(b"preserve this complete unknown package entry")?;
        let source = writer.finish()?.into_inner();
        let mut book = Workbook::from_office_bytes(&source, kind)?;
        for case in &original {
            let (r, c) = address(case["cell"].as_str().unwrap());
            assert_eq!(
                book.calculated_grid()[r][c],
                case["expected"].as_str().unwrap(),
                "{kind}: {}",
                case["cell"]
            );
        }
        book.set_cell(0, 0, "4".into())?;
        book.set_cell(0, 3, "=SEQUENCE(3,2)".into())?;
        let grid = book.calculated_grid();
        let mut cases = original.clone();
        for case in &mut cases {
            let (r, c) = address(case["cell"].as_str().unwrap());
            case["expected"] = grid[r][c].clone().into();
        }
        for (cell, r, c) in [("D3", 2, 3), ("E3", 2, 4), ("F1", 0, 5), ("F2", 1, 5)] {
            if !cases.iter().any(|v| v["cell"] == cell) {
                cases.push(serde_json::json!({"cell":cell,"expected":grid[r][c]}));
            }
        }
        if let Some(expected) = &expected {
            assert_eq!(&cases, expected, "format-dependent calculations");
        } else {
            expected = Some(cases);
        }
        let saved = if kind == "xlsx" {
            book.xlsx_bytes()?
        } else {
            book.ods_bytes()?
        };
        let mut archive = zip::ZipArchive::new(Cursor::new(saved.as_slice()))?;
        assert!(archive.by_name("xls42/project.json").is_err());
        let mut keep = Vec::new();
        archive
            .by_name("foreign-keep.bin")?
            .read_to_end(&mut keep)?;
        assert_eq!(keep, b"preserve this complete unknown package entry");
        let reopened = Workbook::from_office_bytes(&saved, kind)?;
        for case in expected.as_ref().unwrap() {
            let (r, c) = address(case["cell"].as_str().unwrap());
            assert_eq!(
                reopened.calculated_grid()[r][c],
                case["expected"].as_str().unwrap()
            );
        }
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output.join(format!("arrays.{kind}")))?
            .write_all(&saved)?;
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output.join("cases.json"))?
        .write_all(&serde_json::to_vec_pretty(&expected.unwrap())?)?;
    Ok(())
}
