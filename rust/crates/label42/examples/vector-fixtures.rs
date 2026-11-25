//! Generate fixtures for independent SVG/PNG versus PDF vector raster comparisons.
use label42::{symbols, Format, TechnicalElement};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Choose an output directory")?,
    );
    std::fs::create_dir_all(&output)?;
    let mut elements = symbols()
        .iter()
        .map(|symbol| {
            (
                symbol.id.to_owned(),
                TechnicalElement {
                    symbol_id: Some(symbol.id.into()),
                    code_type: Format::Qr,
                    value: String::new(),
                    width_mm: 20.0,
                    caption: false,
                },
            )
        })
        .collect::<Vec<_>>();
    elements.extend(
        [
            (Format::Qr, "qr"),
            (Format::DataMatrix, "data-matrix"),
            (Format::Code128, "code128"),
            (Format::Aztec, "aztec"),
        ]
        .map(|(format, name)| {
            (
                name.into(),
                TechnicalElement {
                    symbol_id: None,
                    code_type: format,
                    value: "PCB-0007".into(),
                    width_mm: 55.0,
                    caption: true,
                },
            )
        }),
    );
    for (name, element) in elements {
        std::fs::write(output.join(format!("{name}.pdf")), element.pdf()?)?;
        std::fs::write(output.join(format!("{name}-reference.png")), element.png()?)?;
    }
    Ok(())
}
