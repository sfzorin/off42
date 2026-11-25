//! Renders the signature part of Kit42 `MarkSamples.write` into a folder, for comparison with the macOS sheet:
//! `cargo run -p marks42 --example signature-samples -- OUT_DIR`.

use marks42::signature::{design, render, Pen, SignatureForm};
use marks42::InkColor;
use std::path::PathBuf;

fn main() -> Result<(), String> {
    let folder = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "signature-samples".into()));
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let save = |image: Option<image::RgbaImage>, name: &str| -> Result<(), String> {
        let image = image.ok_or_else(|| format!("render {name}"))?;
        image.save(folder.join(format!("{name}.png"))).map_err(|e| e.to_string())
    };
    let ink = InkColor::Blue.pen();
    // Each name in every form (one style per form) and four copies of each style.
    let names = ["Sergey Zorin", "Иван Петров", "Ayşe Yılmaz", "John A. Smith"];
    for (n, name) in names.iter().enumerate() {
        for (f, form) in SignatureForm::ALL.iter().skip(1).enumerate() {
            let d = design(*form, n * 3 + f, (31 + n * 7 + f) as u64);
            for c in 0..4u64 {
                save(render(name, d, (c + 1).wrapping_mul(7919), ink, Pen::Ballpoint, 130.0), &format!("signature-{n}-form{f}-copy{c}"))?;
            }
        }
    }
    // Pens: one signature in each pen type.
    let d = design(SignatureForm::InitialSurname, 1, 501);
    for pen in Pen::ALL {
        save(render("Sergey Zorin", d, 5, ink, pen, 130.0), &format!("pen-{}", pen.raw_value()))?;
    }
    save(render("Sergey Zorin", d, 5, ink, Pen::Ballpoint, 130.0 * 2.0), "hires-signature")?;
    println!("{}", folder.display());
    Ok(())
}
