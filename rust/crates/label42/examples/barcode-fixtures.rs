//! Fixtures for optional independent ZXing-C++ and Poppler QA; neither is an app dependency.
use label42::{Format, TechnicalElement};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let folder = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("Choose an output folder")?,
    );
    std::fs::create_dir_all(&folder)?;
    let mut cases = vec![];
    for (format, name) in [
        (Format::Qr, "qr"),
        (Format::DataMatrix, "data-matrix"),
        (Format::Aztec, "aztec"),
        (Format::Code128, "code128"),
    ] {
        let values = if format == Format::Code128 {
            vec!["PCB-0007".to_owned()]
        } else {
            vec![
                "PCB-0007".to_owned(),
                "Плата №007".to_owned(),
                "基板-42 / لوحة / 😀".to_owned(),
                format!("PCB-{}", "1234567890".repeat(30)),
            ]
        };
        for (index, value) in values.into_iter().enumerate() {
            let name = format!("{name}-{index}");
            let element = TechnicalElement {
                symbol_id: None,
                code_type: format,
                value: value.clone(),
                width_mm: 55.,
                caption: false,
            };
            std::fs::write(folder.join(format!("{name}.png")), element.png()?)?;
            std::fs::write(folder.join(format!("{name}.pdf")), element.pdf()?)?;
            cases.push(serde_json::json!({"name":name,"format":format,"value":value}));
        }
    }
    std::fs::write(
        folder.join("cases.json"),
        serde_json::to_vec_pretty(&cases)?,
    )?;
    Ok(())
}
