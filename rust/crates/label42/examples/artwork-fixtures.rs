//! Fixtures compared against the native macOS CoreGraphics drawing transform.
use label42::{sheet_pdf_artwork_stock, Stock};
use lopdf::{dictionary, Document, Object, Stream};
use std::{fs, path::PathBuf};
fn source(rotation: i64) -> Vec<u8> {
    let mut doc = Document::with_version("1.4");
    let root = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let body = doc.add_object(Stream::new(
        dictionary! {},
        b"q 0.2 0.4 0.8 rg 30 45 100 70 re f Q BT /F1 12 Tf 30 200 Td (VECTOR-ART) Tj ET".to_vec(),
    ));
    let page = doc.add_object(dictionary! {"Type"=>"Page","Parent"=>root,"Contents"=>body});
    doc.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>1,"Kids"=>vec![Object::Reference(page)],"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Rotate"=>rotation,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}}));
    let metadata =
        doc.add_object(dictionary! {"Title"=>Object::string_literal("PRIVATE-METADATA")});
    let unused = doc.add_object(Stream::new(
        dictionary! {},
        b"PRIVATE-UNUSED-ATTACHMENT".to_vec(),
    ));
    let _ = unused;
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    doc.trailer.set("Root", catalog);
    doc.trailer.set("Info", metadata);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    if output.join("source-0.pdf").exists() {
        return Err("fixture directory already used".into());
    }
    let stock = Stock::custom(70., 37., 0., 0.)?;
    for rotation in [0, 90, 180, 270] {
        let bytes = source(rotation);
        fs::write(output.join(format!("source-{rotation}.pdf")), &bytes)?;
        fs::write(
            output.join(format!("labels-{rotation}.pdf")),
            sheet_pdf_artwork_stock(&stock, 24, 2, &bytes, Some([30., 40., 130., 240.]))?,
        )?;
    }
    println!("{}", output.display());
    Ok(())
}
