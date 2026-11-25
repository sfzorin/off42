use lopdf::{dictionary, Document, Object, Stream};
use std::fmt::Write;

pub fn calibration_pdf() -> Result<Vec<u8>, String> {
    let mm = 72.0 / 25.4;
    let mut doc = Document::with_version("1.5");
    let pages = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
    let mut content = String::from("0 G 0.5 w\n");
    let (x, y) = (30.0 * mm, 237.0 * mm);
    writeln!(content, "{x:.6} {y:.6} m {:.6} {y:.6} l S", x + 100.0 * mm).unwrap();
    writeln!(content, "{x:.6} {y:.6} m {x:.6} {:.6} l S", y - 100.0 * mm).unwrap();
    for tick in 0..=10 {
        let dx = x + f64::from(tick) * 10.0 * mm;
        let dy = y - f64::from(tick) * 10.0 * mm;
        writeln!(
            content,
            "{dx:.6} {:.6} m {dx:.6} {:.6} l S",
            y - 3.0,
            y + 3.0
        )
        .unwrap();
        writeln!(
            content,
            "{:.6} {dy:.6} m {:.6} {dy:.6} l S",
            x - 3.0,
            x + 3.0
        )
        .unwrap();
    }
    writeln!(content, "BT /F 12 Tf {:.6} {:.6} Td (Calibration sheet - print at 100%, calibration disabled.) Tj ET", 30.0 * mm, 262.0 * mm).unwrap();
    writeln!(
        content,
        "BT /F 10 Tf {:.6} {:.6} Td (100 mm) Tj ET",
        70.0 * mm,
        y + 12.0
    )
    .unwrap();
    writeln!(
        content,
        "BT /F 10 Tf {:.6} {:.6} Td (100 mm) Tj ET",
        x + 12.0,
        y - 50.0 * mm
    )
    .unwrap();
    let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
    let page = doc.add_object(dictionary! {
        "Type"=>"Page", "Parent"=>pages, "Contents"=>stream,
        "Resources"=>dictionary! {"Font"=>dictionary! {"F"=>font}},
        "MediaBox"=>vec![0.into(), 0.into(), Object::Real((210.0 * mm) as f32), Object::Real((297.0 * mm) as f32)],
    });
    doc.objects.insert(
        pages,
        Object::Dictionary(
            dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1},
        ),
    );
    let root = doc.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages, "ViewerPreferences"=>dictionary! {"PrintScaling"=>"None"}});
    doc.trailer.set("Root", root);
    doc.compress();
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|error| error.to_string())?;
    Ok(bytes)
}
