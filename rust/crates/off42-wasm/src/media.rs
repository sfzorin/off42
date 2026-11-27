//! Browser I/O for the existing raster editor and slide writers.
use crate::Result;
use img42_core::ImageDocument;
use ppt42_core::{odp, pptx, Presentation};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlideText {
    title: String,
    body: String,
    notes: String,
}

pub fn presentation_create(slides_json: &str, format: &str) -> Result<Vec<u8>> {
    let slides: Vec<SlideText> = serde_json::from_str(slides_json).map_err(|e| e.to_string())?;
    if slides.is_empty() { return Err("A presentation needs at least one slide".into()); }
    let mut deck = Presentation::new();
    for (index, slide) in slides.into_iter().enumerate() {
        if index > 0 { deck.add()?; }
        deck.edit(slide.title, slide.body)?;
        if !slide.notes.is_empty() { deck.set_notes(&slide.notes)?; }
    }
    match format {
        "pptx" => pptx::export_new(&deck),
        "odp" => odp::export(&deck),
        _ => Err("Unsupported presentation format".into()),
    }
}

thread_local! {
    static IMAGES: RefCell<BTreeMap<u32, ImageDocument>> = RefCell::new(BTreeMap::new());
    static NEXT_IMAGE: std::cell::Cell<u32> = const { std::cell::Cell::new(1) };
}

pub fn image_open(bytes: &[u8]) -> Result<u32> {
    IMAGES.with_borrow_mut(|images| {
        if images.len() >= 8 { return Err("Close an image before opening another".into()); }
        let image = ImageDocument::open_raster_bytes(bytes).map_err(|e| e.to_string())?;
        let id = NEXT_IMAGE.get();
        NEXT_IMAGE.set(id.checked_add(1).ok_or("Image handle limit reached")?);
        images.insert(id, image);
        Ok(id)
    })
}

fn with_image<T>(id: u32, action: impl FnOnce(&mut ImageDocument) -> Result<T>) -> Result<T> {
    IMAGES.with_borrow_mut(|images| action(images.get_mut(&id).ok_or("Image is closed")?))
}

pub fn image_edit(id: u32, action: &str) -> Result<String> {
    with_image(id, |image| {
        match action {
            "rotate" => image.rotate_cw(),
            "flip" => image.flip_horizontal(),
            "undo" => { image.undo(); Ok(()) }
            "redo" => { image.redo(); Ok(()) }
            "info" => Ok(()),
            _ => return Err("Unknown image edit".into()),
        }.map_err(|e| e.to_string())?;
        Ok(serde_json::json!({"width":image.width(),"height":image.height(),"dirty":image.is_dirty(),
            "canUndo":image.can_undo(),"canRedo":image.can_redo()}).to_string())
    })
}

pub fn image_save(id: u32, format: &str, mark: bool) -> Result<Vec<u8>> {
    with_image(id, |image| {
        let format = if format == "jpg" { "jpeg" } else { format };
        let (bytes, state) = image.save_snapshot_format(format).map_err(|e| e.to_string())?;
        if mark { image.mark_saved_snapshot(state); }
        Ok(bytes)
    })
}

pub fn image_close(id: u32) { IMAGES.with_borrow_mut(|images| images.remove(&id)); }
