use scn42_core::{OcrWord, ScanDocument};
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    let mut doc = ScanDocument::new();
    let (width, height) = (600u32, 800u32);
    let mut pixels = vec![255; width as usize * height as usize * 4];
    for y in 30..100 {
        for x in 20..150 {
            let i = (y * width as usize + x) * 4;
            pixels[i..i + 4].copy_from_slice(&[60, 100, 200, 255]);
        }
    }
    doc.add_rgba(width, height, pixels)?;
    doc.set_dpi(0, 150, 150)?;
    doc.export_pdf(&output.join("scan-source.pdf"))?;
    let source = doc.pages()[0].clone();
    doc.apply_ocr(
        0,
        &source,
        ["SCAN-original", "Привет", "中文", "ไทย", "😀"]
            .iter()
            .enumerate()
            .map(|(i, text)| OcrWord {
                text: text.to_string(),
                x: 50,
                y: 150 + i as u32 * 50,
                width: 400,
                height: 40,
            })
            .collect(),
    )?;
    doc.export_pdf(&output.join("scan-recognized.pdf"))?;
    doc.export_pdf_compressed(&output.join("scan-compressed.pdf"), true)?;
    doc.save_project(&output.join("scan.scn42"))?;
    let reopened = ScanDocument::open_project(&output.join("scan.scn42"))?;
    assert_eq!(reopened.pages(), doc.pages());
    println!("{}", output.display());
    Ok(())
}
