//! Independent PDF renderer fixtures, including crop origins and all quarter turns.
use lopdf::{dictionary, Document, Object, Stream};
use pdf42_core::{
    decorations::{HeaderFooter, NumberFormat, PageNumbers, Watermark},
    PdfDocument,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
fn write(path: &Path, data: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)?
        .write_all(data)?;
    Ok(())
}
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("output directory required")?,
    );
    fs::create_dir_all(&output)?;
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let font =
        doc.add_object(dictionary! {"Type"=>"Font","Subtype"=>"Type1","BaseFont"=>"Helvetica"});
    let mut kids = Vec::new();
    for rotation in [0, 90, 180, 270] {
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("BT /F1 12 Tf 30 140 Td (ORIGINAL-{rotation}) Tj ET\n2 0 0 2 0 0 cm")
                .into_bytes(),
        ));
        let page=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>root,"MediaBox"=>vec![10.into(),20.into(),210.into(),320.into()],"CropBox"=>vec![20.into(),30.into(),200.into(),310.into()],"Rotate"=>rotation,"Contents"=>content});
        kids.push(Object::Reference(page));
    }
    doc.objects.insert(root,Object::Dictionary(dictionary!{"Type"=>"Pages","Count"=>4,"Kids"=>kids,"Resources"=>dictionary!{"Font"=>dictionary!{"F1"=>font}}}));
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>root});
    doc.trailer.set("Root", catalog);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes)?;
    let source = output.join("source.pdf");
    write(&source, &bytes)?;
    let mut doc = PdfDocument::open(&source)?;
    doc.add_watermark(
        &[0, 1, 2, 3],
        Watermark {
            text: "DRAFT".into(),
            size: 32.0,
            ..Default::default()
        },
    )?;
    write(&output.join("watermark.pdf"), &doc.save_snapshot()?.0)?;
    let mut spec = HeaderFooter::default();
    spec.texts = [
        "FILE <<file>>".into(),
        "Page <<page>>/<<pages>>".into(),
        "<<date>>".into(),
        "<<bates>>".into(),
        "Привет".into(),
        "END".into(),
    ];
    spec.bates_start = 41;
    spec.bates_prefix = "CASE-".into();
    spec.file_name = "source.pdf".into();
    spec.date = "2026-11-07".into();
    spec.horizontal = 10.0;
    spec.vertical = 14.0;
    spec.size = 7.0;
    doc.add_header_footer(&[0, 1, 2, 3], &spec)?;
    doc.add_page_numbers(
        &[0, 1, 2, 3],
        &PageNumbers {
            slot: 4,
            start: 5,
            skip_first: false,
            format: NumberFormat::PageOfTotal,
            size: 8.0,
            margin: 4.0,
        },
    )?;
    write(&output.join("headers.pdf"), &doc.save_snapshot()?.0)?;
    println!("{}", output.display());
    Ok(())
}
