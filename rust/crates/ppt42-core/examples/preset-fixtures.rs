//! Office-renderer fixtures for the complete preset catalog.
use ppt42_core::{geometry, ObjectKind, Presentation};
use std::{fs, io::Write, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    let mut deck = Presentation::new();
    for (i, def) in geometry::catalog().iter().enumerate() {
        let slot = i % 12;
        if i > 0 && slot == 0 {
            deck.add()?;
        }
        let id = deck.insert_preset(geometry::Preset::new(&def.name)?)?;
        let mut shape = deck.slides()[deck.current()]
            .objects
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .clone();
        shape.x = 25.0 + (slot % 4) as f64 * 175.0;
        shape.y = 20.0 + (slot / 4) as f64 * 130.0;
        shape.width = 140.0;
        shape.height = 80.0;
        deck.update_object(shape.clone())?;
        let id = deck.insert_object(ObjectKind::Text, def.name.clone())?;
        let mut label = deck.slides()[deck.current()]
            .objects
            .iter()
            .find(|o| o.id == id)
            .unwrap()
            .clone();
        label.x = shape.x;
        label.y = shape.y + 90.0;
        label.width = 160.0;
        label.height = 25.0;
        label.fill_visible = false;
        label.stroke_visible = false;
        label.text_style.points = 10.0;
        deck.update_object(label)?;
    }
    for (name, data) in [
        ("presets.pptx", ppt42_core::pptx::export_new(&deck)?),
        ("presets.odp", ppt42_core::odp::export(&deck)?),
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
