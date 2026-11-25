use super::*;
use std::{io::Cursor, sync::Arc};

pub const MAX_PICTURE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SheetImage {
    pub id: u64,
    pub row: usize,
    pub column: usize,
    pub offset_x: f64,
    pub offset_y: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub width_points: f64,
    pub height_points: f64,
    #[serde(with = "image_data")]
    pub data: Arc<[u8]>,
    pub description: String,
    /// Original package object identity; only the imported baseline authorizes its removal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

mod image_data {
    use base64::{engine::general_purpose::STANDARD, Engine};
    use serde::{Deserialize, Deserializer, Serializer};
    use std::sync::Arc;

    pub fn serialize<S: Serializer>(data: &Arc<[u8]>, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&STANDARD.encode(data))
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Arc<[u8]>, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() > super::MAX_PICTURE_BYTES * 4 / 3 + 8 {
            return Err(serde::de::Error::custom("picture exceeds size limit"));
        }
        let data = STANDARD.decode(value).map_err(serde::de::Error::custom)?;
        if data.len() > super::MAX_PICTURE_BYTES {
            return Err(serde::de::Error::custom("picture exceeds size limit"));
        }
        Ok(data.into())
    }
}

pub(super) fn valid(image: &SheetImage, sheet: &Sheet) -> bool {
    image.id > 0
        && image.pixel_width > 0
        && image.pixel_height > 0
        && u64::from(image.pixel_width) * u64::from(image.pixel_height) <= 4_000_000
        && image.row < sheet.rows
        && image.column < sheet.cols
        && [
            image.offset_x,
            image.offset_y,
            image.width_points,
            image.height_points,
        ]
        .iter()
        .all(|n| n.is_finite())
        && (0.0..=144_000.0).contains(&image.offset_x)
        && (0.0..=144_000.0).contains(&image.offset_y)
        && image.width_points > 0.0
        && image.width_points <= 144_000.0
        && image.height_points > 0.0
        && image.height_points <= 144_000.0
        && image.description.len() <= 24_000
        && image
            .source
            .as_ref()
            .is_none_or(|source| source.len() <= 2048 && !source.contains('\0'))
        && !image.data.is_empty()
        && image.data.len() <= MAX_PICTURE_BYTES
}

pub(super) fn valid_budget(state: &State) -> bool {
    let images = state.sheets.iter().flat_map(|sheet| &sheet.images);
    let mut ids = std::collections::BTreeSet::new();
    images
        .clone()
        .all(|image| ids.insert(image.id) && !state.sheets.iter().any(|sheet| sheet.id == image.id))
        && images.clone().map(|image| image.data.len()).sum::<usize>() <= MAX_PICTURE_BYTES
        && images
            .map(|image| u64::from(image.pixel_width) * u64::from(image.pixel_height))
            .sum::<u64>()
            <= 16_000_000
}

pub(super) fn validate_png(data: &[u8]) -> Result<(u32, u32), Error> {
    if data.len() > 16 * 1024 * 1024 || !data.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Err(Error::Invalid("Use a PNG picture up to 16 MB"));
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(data), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let picture = reader
        .decode()
        .map_err(|_| Error::Invalid("Invalid PNG picture"))?;
    if u64::from(picture.width()) * u64::from(picture.height()) > 4_000_000 {
        return Err(Error::Invalid(
            "Pictures are limited to four million pixels",
        ));
    }
    Ok((picture.width(), picture.height()))
}

impl Workbook {
    pub fn images(&self) -> &[SheetImage] {
        &self.history.current().sheet().images
    }

    pub fn insert_image(
        &mut self,
        row: usize,
        column: usize,
        png: Vec<u8>,
        width_points: f64,
        height_points: f64,
        description: String,
    ) -> Result<u64, Error> {
        self.require_editable()?;
        let (pixel_width, pixel_height) = validate_png(&png)?;
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        let id = state.next_id;
        state.next_id = id.checked_add(1).ok_or(Error::Limit)?;
        let picture = SheetImage {
            id,
            row,
            column,
            offset_x: 0.0,
            offset_y: 0.0,
            width_points,
            height_points,
            pixel_width,
            pixel_height,
            data: png.into(),
            description,
            source: None,
        };
        if !valid(&picture, state.sheet()) {
            return Err(Error::Invalid("Invalid picture geometry"));
        }
        let total = state
            .sheets
            .iter()
            .flat_map(|sheet| &sheet.images)
            .map(|image| image.data.len())
            .sum::<usize>();
        let pixels = state
            .sheets
            .iter()
            .flat_map(|sheet| &sheet.images)
            .map(|image| u64::from(image.pixel_width) * u64::from(image.pixel_height))
            .sum::<u64>();
        if pixels + u64::from(pixel_width) * u64::from(pixel_height) > 16_000_000
            || state.sheet().images.len() >= 1000
            || total.saturating_add(picture.data.len()) > MAX_PICTURE_BYTES
        {
            return Err(Error::Limit);
        }
        state.sheet_mut().images.push(picture);
        self.history.apply(state);
        Ok(id)
    }

    pub fn update_image(&mut self, picture: SheetImage) -> Result<bool, Error> {
        self.require_editable()?;
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        if !valid(&picture, state.sheet()) {
            return Err(Error::Invalid("Invalid picture geometry"));
        }
        let index = state
            .sheet()
            .images
            .iter()
            .position(|image| image.id == picture.id)
            .ok_or(Error::Bounds)?;
        if state.sheet().images[index].source != picture.source {
            return Err(Error::Invalid(
                "The original picture identity cannot be changed",
            ));
        }
        if state.sheet().images[index].data == picture.data
            && (
                state.sheet().images[index].pixel_width,
                state.sheet().images[index].pixel_height,
            ) != (picture.pixel_width, picture.pixel_height)
        {
            return Err(Error::Invalid("Picture dimensions do not match its data"));
        }
        if state.sheet().images[index].data != picture.data {
            if validate_png(&picture.data)? != (picture.pixel_width, picture.pixel_height) {
                return Err(Error::Invalid("Picture dimensions do not match its data"));
            }
        }
        let total = state
            .sheets
            .iter()
            .flat_map(|sheet| &sheet.images)
            .map(|image| image.data.len())
            .sum::<usize>()
            - state.sheet().images[index].data.len()
            + picture.data.len();
        let pixels = state
            .sheets
            .iter()
            .flat_map(|sheet| &sheet.images)
            .filter(|image| image.id != picture.id)
            .map(|image| u64::from(image.pixel_width) * u64::from(image.pixel_height))
            .sum::<u64>()
            + u64::from(picture.pixel_width) * u64::from(picture.pixel_height);
        if pixels > 16_000_000 || total > MAX_PICTURE_BYTES {
            return Err(Error::Limit);
        }
        state.sheet_mut().images[index] = picture;
        Ok(self.history.apply(state))
    }

    pub fn delete_image(&mut self, id: u64) -> Result<bool, Error> {
        self.require_editable()?;
        let mut state = self.history.current().clone();
        if state.sheet().protected {
            return Err(Error::Protected);
        }
        let index = state
            .sheet()
            .images
            .iter()
            .position(|image| image.id == id)
            .ok_or(Error::Bounds)?;
        state.sheet_mut().images.remove(index);
        Ok(self.history.apply(state))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    pub(super) fn png() -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        image::RgbaImage::from_fn(8, 4, |x, y| {
            image::Rgba([
                x as u8 * 20,
                y as u8 * 40,
                100,
                if x == 0 { 0 } else { 255 },
            ])
        })
        .write_to(&mut output, image::ImageFormat::Png)
        .unwrap();
        output.into_inner()
    }
    fn external(bytes: &[u8]) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let file = archive.by_index(i).unwrap();
            if file.name() != "xls42/project.json" {
                writer.raw_copy_file(file).unwrap();
            }
        }
        writer
            .start_file(
                "custom/retained.bin",
                zip::write::SimpleFileOptions::default(),
            )
            .unwrap();
        writer
            .write_all(b"opaque content must survive picture edits")
            .unwrap();
        writer.finish().unwrap().into_inner()
    }
    fn part(bytes: &[u8], name: &str) -> Vec<u8> {
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut data = Vec::new();
        archive
            .by_name(name)
            .unwrap()
            .read_to_end(&mut data)
            .unwrap();
        data
    }
    fn office(book: &Workbook, kind: &str) -> Vec<u8> {
        if kind == "xlsx" {
            book.xlsx_bytes()
        } else {
            book.ods_bytes()
        }
        .unwrap()
    }
    #[test]
    fn geometry_pixels_history_protection_and_project_round_trip() {
        let mut book = Workbook::new(20, 8).unwrap();
        let id = book
            .insert_image(2, 3, png(), 72.5, 36.25, "own stamp".into())
            .unwrap();
        let original = book.images()[0].clone();
        let mut changed = original.clone();
        changed.row = 5;
        changed.offset_x = 3.5;
        changed.width_points = 99.25;
        book.update_image(changed.clone()).unwrap();
        assert!(book.undo());
        assert_eq!(book.images(), &[original.clone()]);
        assert!(book.redo());
        assert_eq!(book.images(), &[changed.clone()]);
        let restored = Workbook::from_project_bytes(&book.project_bytes().unwrap()).unwrap();
        assert_eq!(restored.images(), book.images());
        let mut invalid = changed.clone();
        invalid.pixel_width += 1;
        assert!(book.update_image(invalid).is_err());
        assert_eq!(book.images(), &[changed]);
        assert!(book.csv_bytes().is_err());
        book.delete_image(id).unwrap();
        assert!(book.images().is_empty());
        book.undo();
        assert_eq!(book.images().len(), 1);
        assert!(book.set_protected(true));
        assert!(book.delete_image(id).is_err());
        assert!(book
            .insert_image(0, 0, png(), 72.0, 36.0, String::new())
            .is_err());
    }
    #[test]
    fn native_office_contains_real_picture_data_and_independent_import_geometry() {
        let mut book = Workbook::new(20, 8).unwrap();
        book.insert_image(2, 3, png(), 72.0, 36.0, "stamp & signature".into())
            .unwrap();
        for kind in ["xlsx", "ods"] {
            let bytes = office(&book, kind);
            let reopened = Workbook::from_office_bytes(&bytes, kind).unwrap();
            assert_eq!(reopened.images(), book.images());
            let independently = Workbook::from_office_bytes(&external(&bytes), kind).unwrap();
            assert_eq!(independently.images().len(), 1, "{kind}");
            let image = &independently.images()[0];
            assert_eq!((image.row, image.column), (2, 3));
            assert!((image.width_points - 72.0).abs() < 0.01);
            assert!((image.height_points - 36.0).abs() < 0.01);
            assert_eq!(image.data.as_ref(), png());
            assert_eq!(image.description, "stamp & signature");
        }
    }
    #[test]
    fn insert_move_and_delete_in_external_packages_preserve_other_entries() {
        for kind in ["xlsx", "ods"] {
            let mut base = Workbook::new(20, 8).unwrap();
            base.set_cell(0, 0, "retained value".into()).unwrap();
            let source = external(&office(&base, kind));
            let mut book = Workbook::from_office_bytes(&source, kind).unwrap();
            book.insert_image(6, 5, png(), 108.0, 54.0, "Own mark".into())
                .unwrap();
            let inserted = office(&book, kind);
            assert_eq!(
                part(&inserted, "custom/retained.bin"),
                part(&source, "custom/retained.bin")
            );
            let mut book = Workbook::from_office_bytes(&inserted, kind).unwrap();
            assert_eq!(book.images().len(), 1, "{kind}");
            let mut picture = book.images()[0].clone();
            picture.row = 10;
            picture.column = 7;
            picture.offset_x = 8.25;
            picture.width_points = 123.0;
            book.update_image(picture).unwrap();
            book.set_cell(0, 0, "updated value".into()).unwrap();
            let moved = office(&book, kind);
            let mut book = Workbook::from_office_bytes(&moved, kind).unwrap();
            assert_eq!(book.cell(0, 0), Some("updated value"));
            assert_eq!(book.images().len(), 1, "{kind}");
            let picture = &book.images()[0];
            assert_eq!((picture.row, picture.column), (10, 7));
            assert!((picture.offset_x - 8.25).abs() < 0.01);
            assert!((picture.width_points - 123.0).abs() < 0.01);
            book.delete_image(picture.id).unwrap();
            let deleted = office(&book, kind);
            assert!(Workbook::from_office_bytes(&deleted, kind)
                .unwrap()
                .images()
                .is_empty());
            assert_eq!(
                part(&deleted, "custom/retained.bin"),
                part(&source, "custom/retained.bin")
            );
        }
    }
    #[test]
    fn reordered_external_ods_removes_the_original_picture_before_replacing() {
        let mut original = Workbook::new(20, 8).unwrap();
        let first = original.active_sheet();
        let first_name = original.sheets()[0].1.clone();
        original
            .insert_image(2, 3, png(), 72.0, 36.0, "original".into())
            .unwrap();
        original.add_sheet("Second").unwrap();
        original.select_sheet(first).unwrap();
        let source = external(&office(&original, "ods"));
        let mut book = Workbook::from_office_bytes(&source, "ods").unwrap();
        let first = book.active_sheet();
        book.reorder_sheet(first, 1).unwrap();
        book.select_sheet(first).unwrap();
        let mut picture = book.images()[0].clone();
        picture.column = 5;
        picture.width_points = 90.0;
        book.update_image(picture).unwrap();
        let moved = office(&book, "ods");
        let mut reopened = Workbook::from_office_bytes(&moved, "ods").unwrap();
        let first = reopened
            .sheets()
            .into_iter()
            .find(|(_, name)| name == &first_name)
            .unwrap()
            .0;
        reopened.select_sheet(first).unwrap();
        assert_eq!(
            reopened.images().len(),
            1,
            "moving a sheet must not duplicate its picture"
        );
        assert_eq!(reopened.images()[0].column, 5);
        assert!((reopened.images()[0].width_points - 90.0).abs() < 0.01);
        let id = reopened.images()[0].id;
        reopened.reorder_sheet(first, 0).unwrap();
        reopened.delete_image(id).unwrap();
        let deleted = office(&reopened, "ods");
        let mut reopened = Workbook::from_office_bytes(&deleted, "ods").unwrap();
        let first = reopened
            .sheets()
            .into_iter()
            .find(|(_, name)| name == &first_name)
            .unwrap()
            .0;
        reopened.select_sheet(first).unwrap();
        assert!(reopened.images().is_empty());
        assert_eq!(
            part(&deleted, "custom/retained.bin"),
            part(&source, "custom/retained.bin")
        );
    }
    #[test]
    fn structural_history_moves_anchors_and_duplicates_with_unique_ids() {
        let mut book = Workbook::new(20, 8).unwrap();
        let first = book.active_sheet();
        book.insert_image(2, 3, png(), 72.0, 36.0, String::new())
            .unwrap();
        let id = book.images()[0].id;
        book.insert_rows(1, 2).unwrap();
        assert_eq!(book.images()[0].row, 4);
        book.undo();
        assert_eq!(book.images()[0].row, 2);
        book.duplicate_sheet(first, "Copy").unwrap();
        assert_ne!(book.images()[0].id, id);
        assert!(book.images()[0].source.is_none());
        assert!(Workbook::from_project_bytes(&book.project_bytes().unwrap()).is_ok());
    }
}
