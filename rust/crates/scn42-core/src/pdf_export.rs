use super::{Page, ScanDocument};
use image::{codecs::jpeg::JpegEncoder, imageops, ExtendedColorType, RgbImage};
use lopdf::{dictionary, Document, Object, Stream};
use std::{io::Write, path::Path};
use suite42_core::{session::FileIdentity, win32_path, write_new};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PdfRaster {
    #[default]
    Lossless,
    Jpeg {
        dpi: u32,
        quality: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PdfExportOptions {
    pub raster: PdfRaster,
    pub searchable: bool,
    pub compress_streams: bool,
}

impl Default for PdfExportOptions {
    fn default() -> Self {
        Self {
            raster: PdfRaster::Lossless,
            searchable: true,
            compress_streams: true,
        }
    }
}

impl ScanDocument {
    pub fn export_pdf(&self, path: &Path) -> Result<(), String> {
        self.export_pdf_compressed(path, false)
    }

    pub fn export_pdf_compressed(&self, path: &Path, compressed: bool) -> Result<(), String> {
        self.export_pdf_with_options(
            path,
            PdfExportOptions {
                compress_streams: compressed,
                ..Default::default()
            },
        )
    }

    pub fn export_pdf_with_options(
        &self,
        path: &Path,
        options: PdfExportOptions,
    ) -> Result<(), String> {
        let bytes = pdf(self.pages(), options)?;
        write_new(path, &bytes).map_err(|error| error.to_string())
    }

    /// Publish one independently searchable PDF per page. The selected name is the prefix.
    pub fn export_pdf_pages(
        &self,
        path: &Path,
        options: PdfExportOptions,
    ) -> Result<usize, String> {
        self.export_pdf_page_files(path, options).map(|files| files.len())
    }

    pub fn export_pdf_page_files(
        &self,
        path: &Path,
        options: PdfExportOptions,
    ) -> Result<Vec<std::path::PathBuf>, String> {
        if self.pages().is_empty() {
            return Err("cannot export an empty scan".into());
        }
        let stem = path.file_stem().ok_or("Missing PDF filename prefix")?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let directory = win32_path(parent).map_err(|error| error.to_string())?;
        let mut paths = Vec::with_capacity(self.pages().len());
        for number in 1..=self.pages().len() {
            let mut name = stem.to_os_string();
            name.push(format!("-{number:03}.pdf"));
            let target = directory.join(name);
            match std::fs::symlink_metadata(&target) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => paths.push(target),
                Ok(_) => {
                    return Err(format!(
                        "PDF export destination exists: {}",
                        target.display()
                    ))
                }
                Err(error) => return Err(error.to_string()),
            }
        }

        let mut prepared = Vec::with_capacity(paths.len());
        for (page, target) in self.pages().iter().zip(paths) {
            let bytes = pdf(std::slice::from_ref(page), options)?;
            let mut temp =
                tempfile::NamedTempFile::new_in(&directory).map_err(|error| error.to_string())?;
            temp.write_all(&bytes)
                .and_then(|()| temp.as_file().sync_all())
                .map_err(|error| error.to_string())?;
            let modified = temp
                .as_file()
                .metadata()
                .and_then(|meta| meta.modified())
                .ok();
            let identity = FileIdentity::from_bytes(&bytes, modified);
            prepared.push((temp, target, identity));
        }

        let mut published: Vec<(std::path::PathBuf, FileIdentity)> = Vec::new();
        let result = (|| {
            for (temp, path, identity) in prepared {
                drop(
                    temp.persist_noclobber(&path)
                        .map_err(|error| error.error.to_string())?,
                );
                published.push((path, identity));
            }
            Ok(())
        })();
        if let Err(error) = result {
            let mut issues = Vec::new();
            for (path, identity) in published.into_iter().rev() {
                if !matches!(FileIdentity::read(&path), Ok(current) if current == identity)
                    || !identity.metadata_matches(&path).unwrap_or(false)
                    || std::fs::remove_file(&path).is_err()
                {
                    issues.push(path.display().to_string());
                }
            }
            if issues.is_empty() {
                return Err(error);
            }
            return Err(format!(
                "{error}. Could not roll back: {}",
                issues.join(", ")
            ));
        }
        Ok(published.into_iter().map(|(path, _)| path).collect())
    }
}

fn pdf(pages: &[Page], options: PdfExportOptions) -> Result<Vec<u8>, String> {
    if pages.is_empty() {
        return Err("cannot export an empty scan".into());
    }
    let mut doc = Document::with_version("1.5");
    let text: String = pages
        .iter()
        .filter(|_| options.searchable)
        .flat_map(|p| p.words.iter())
        .map(|w| w.text.as_str())
        .collect();
    // Image-only scans need no font; OCR text gets a glyph subset.
    let embedded = if text.is_empty() {
        None
    } else {
        Some(suite42_pdf_text::PdfFont::embed_invisible_subset(
            &mut doc, &text,
        )?)
    };
    let pages_id = doc.new_object_id();
    let mut kids = Vec::new();
    for page in pages {
        let image = doc.add_object(image_stream(page, options.raster)?);
        let w = page.width as f32 * 72.0 / page.dpi_x as f32;
        let h = page.height as f32 * 72.0 / page.dpi_y as f32;
        let mut commands = format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q\n");
        let font = embedded.as_ref().filter(|_| !page.words.is_empty());
        if let Some(font) = font {
            let chars = &font.chars;
            for word in page.words.iter() {
                let x = word.x as f32 * 72.0 / page.dpi_x as f32;
                let y = (page.height - word.y - word.height) as f32 * 72.0 / page.dpi_y as f32;
                let size = word.height as f32 * 72.0 / page.dpi_y as f32;
                let hex: String = word
                    .text
                    .chars()
                    .map(|c| format!("{:04X}", chars[&c].0))
                    .collect();
                let advance: u64 = word.text.chars().map(|c| u64::from(chars[&c].2)).sum();
                let target = word.width as f32 * 72.0 / page.dpi_x as f32;
                let scale = if advance == 0 {
                    100.0
                } else {
                    target * 100000.0 / (advance as f32 * size)
                };
                commands.push_str(&format!(
                    "BT /F1 {size} Tf {scale} Tz 3 Tr 1 0 0 1 {x} {y} Tm <{hex}> Tj ET\n"
                ));
            }
        }
        let content = doc.add_object(Stream::new(dictionary! {}, commands.into_bytes()));
        let mut resources = dictionary! {"XObject"=>dictionary!{"Im0"=>image}};
        if let Some(font) = font {
            resources.set("Font", dictionary! {"F1"=>font.id});
        }
        let id=doc.add_object(dictionary!{"Type"=>"Page","Parent"=>pages_id,"MediaBox"=>vec![0.into(),0.into(),w.into(),h.into()],"Resources"=>resources,"Contents"=>content});
        kids.push(Object::Reference(id));
    }
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {"Type"=>"Pages","Count"=>kids.len() as i64,"Kids"=>kids}),
    );
    let catalog = doc.add_object(dictionary! {"Type"=>"Catalog","Pages"=>pages_id});
    doc.trailer.set("Root", catalog);
    if options.compress_streams {
        doc.compress();
    }
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn image_stream(page: &Page, raster: PdfRaster) -> Result<Stream, String> {
    let mut rgb = Vec::with_capacity(page.pixels.len() / 4 * 3);
    for pixel in page.pixels.as_chunks::<4>().0 {
        let alpha = u32::from(pixel[3]);
        for &channel in &pixel[..3] {
            rgb.push(((u32::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8);
        }
    }
    let mut image = dictionary! {
        "Type" => "XObject", "Subtype" => "Image",
        "Width" => i64::from(page.width), "Height" => i64::from(page.height),
        "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
    };
    if let PdfRaster::Jpeg { dpi, quality } = raster {
        if !(72..=600).contains(&dpi) || !(1..=100).contains(&quality) {
            return Err("Invalid PDF image quality settings".into());
        }
        if rgb
            .as_chunks::<3>()
            .0
            .iter()
            .all(|pixel| *pixel == [0; 3] || *pixel == [255; 3])
        {
            let stride = (page.width as usize).div_ceil(8);
            let mut packed = vec![0; stride * page.height as usize];
            for (y, row) in rgb.chunks_exact(page.width as usize * 3).enumerate() {
                for (x, pixel) in row.chunks_exact(3).enumerate() {
                    if pixel[0] == 255 {
                        packed[y * stride + x / 8] |= 0x80 >> (x % 8);
                    }
                }
            }
            image.set("ColorSpace", Object::Name(b"DeviceGray".to_vec()));
            image.set("BitsPerComponent", 1);
            return Ok(Stream::new(image, packed));
        }
        let dimensions = |pixels: u32, original: u32| {
            if original <= dpi {
                pixels
            } else {
                (u64::from(pixels) * u64::from(dpi) / u64::from(original)).max(1) as u32
            }
        };
        let width = dimensions(page.width, page.dpi_x);
        let height = dimensions(page.height, page.dpi_y);
        if (width, height) != (page.width, page.height) {
            let pixels =
                RgbImage::from_raw(page.width, page.height, rgb).ok_or("Invalid scan pixels")?;
            rgb =
                imageops::resize(&pixels, width, height, imageops::FilterType::Lanczos3).into_raw();
        }
        let mut encoded = Vec::new();
        JpegEncoder::new_with_quality(&mut encoded, quality)
            .encode(&rgb, width, height, ExtendedColorType::Rgb8)
            .map_err(|error| error.to_string())?;
        image.set("Width", i64::from(width));
        image.set("Height", i64::from(height));
        image.set("Filter", Object::Name(b"DCTDecode".to_vec()));
        rgb = encoded;
    }
    Ok(Stream::new(image, rgb))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OcrWord;

    #[test]
    fn compressed_export_preserves_physical_geometry_and_controls_ocr_and_bilevel_pixels() {
        let mut scan = ScanDocument::new();
        let photo: Vec<u8> = (0..96).flat_map(|n| [n, n * 2, 80, 255]).collect();
        let bilevel: Vec<u8> = (0..34)
            .flat_map(|n| {
                let tone = if n % 2 == 0 { 0 } else { 255 };
                [tone, tone, tone, 255]
            })
            .collect();
        scan.add_pages_with_dpi(vec![(8, 12, photo, 600, 300), (17, 2, bilevel, 600, 600)])
            .unwrap();
        let source = scan.pages()[0].clone();
        scan.apply_ocr(
            0,
            &source,
            vec![OcrWord {
                text: "Board 42".into(),
                x: 1,
                y: 1,
                width: 5,
                height: 3,
            }],
        )
        .unwrap();
        let original = scan.pages().to_vec();
        let parse = |options| Document::load_mem(&pdf(scan.pages(), options).unwrap()).unwrap();
        let lossless = parse(PdfExportOptions::default());
        let compact_options = PdfExportOptions {
            raster: PdfRaster::Jpeg {
                dpi: 150,
                quality: 82,
            },
            searchable: false,
            compress_streams: true,
        };
        let compact = parse(compact_options);
        let ids = compact.get_pages();
        let page_dict = compact.get_dictionary(ids[&1]).unwrap();
        let original_page = lossless.get_dictionary(lossless.get_pages()[&1]).unwrap();
        assert_eq!(
            page_dict.get(b"MediaBox").unwrap(),
            original_page.get(b"MediaBox").unwrap()
        );
        let images = compact.get_page_images(ids[&1]).unwrap();
        assert_eq!((images[0].width, images[0].height), (2, 6));
        assert_eq!(
            images[0].filters.as_deref(),
            Some(["DCTDecode".to_owned()].as_slice())
        );
        assert!(page_dict
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .is_err());
        let images = compact.get_page_images(ids[&2]).unwrap();
        assert_eq!((images[0].width, images[0].height), (17, 2));
        assert_eq!(images[0].bits_per_component, Some(1));
        assert_eq!(images[0].color_space.as_deref(), Some("DeviceGray"));
        let searchable = parse(PdfExportOptions {
            searchable: true,
            ..compact_options
        });
        assert!(searchable
            .get_dictionary(searchable.get_pages()[&1])
            .unwrap()
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Font")
            .is_ok());
        assert_eq!(scan.pages(), original);
    }

    #[test]
    fn split_pdf_prepares_independent_pages_and_refuses_existing_outputs() {
        let folder = tempfile::tempdir().unwrap();
        let prefix = folder.path().join("scan.pdf");
        let first = folder.path().join("scan-001.pdf");
        let second = folder.path().join("scan-002.pdf");
        let mut scan = ScanDocument::new();
        scan.add_pages_with_dpi(vec![
            (2, 1, vec![255; 8], 300, 150),
            (1, 2, vec![255; 8], 150, 300),
        ])
        .unwrap();
        std::fs::write(&second, b"keep original").unwrap();
        assert!(scan
            .export_pdf_pages(&prefix, PdfExportOptions::default())
            .is_err());
        assert!(!first.exists() && !prefix.exists());
        assert_eq!(std::fs::read(&second).unwrap(), b"keep original");
        std::fs::remove_file(&second).unwrap();
        assert_eq!(
            scan.export_pdf_pages(&prefix, PdfExportOptions::default())
                .unwrap(),
            2
        );
        for (path, dimensions) in [(first, (2, 1)), (second, (1, 2))] {
            let pdf = Document::load(path).unwrap();
            let pages = pdf.get_pages();
            assert_eq!(pages.len(), 1);
            let images = pdf.get_page_images(pages[&1]).unwrap();
            assert_eq!((images[0].width, images[0].height), dimensions);
        }
        assert!(!prefix.exists());
        assert_eq!(std::fs::read_dir(folder.path()).unwrap().count(), 2);
    }
}
