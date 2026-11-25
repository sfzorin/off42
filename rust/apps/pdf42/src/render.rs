//! PDF-specific rendering workflows using the shared WinRT raster backend.
use suite42_pdf_raster::render;
pub use suite42_pdf_raster::{Request, Worker};
use windows::Win32::System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED};
pub type FlattenedPages = Vec<pdf42_core::FlattenedPage>;
/// Render and compress one page at a time on a dedicated WinRT apartment.
pub fn flatten_async(
    bytes: Vec<u8>,
    sizes: Vec<(f64, f64)>,
    target: usize,
    rect: [f64; 4],
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::sync::mpsc::Receiver<Result<FlattenedPages, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe { RoUninitialize() }
                }
            }
            let _apartment = Apartment;
            if bytes.len() > 64 * 1024 * 1024 {
                return Err("Flattening requires a source snapshot under 64 MB".into());
            }
            let mut output = Vec::new();
            let mut compressed_total = 0usize;
            let (left, right, top, bottom) = (
                rect[0].min(rect[2]).clamp(0., 1.),
                rect[0].max(rect[2]).clamp(0., 1.),
                rect[1].min(rect[3]).clamp(0., 1.),
                rect[1].max(rect[3]).clamp(0., 1.),
            );
            if right - left < 0.001 || bottom - top < 0.001 {
                return Err("Drag a larger redaction area".into());
            }
            for (index, &(pw, ph)) in sizes.iter().enumerate() {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Flattening cancelled".into());
                }
                let w = (pw * 150. / 72.).ceil() as u64;
                let h = (ph * 150. / 72.).ceil() as u64;
                if w == 0 || h == 0 || w > 4096 || h > 4096 || w * h > 4_000_000 {
                    return Err("Page exceeds the 150 DPI raster pixel limit".into());
                }
                let request = Request {
                    generation: 0,
                    bytes: bytes.clone(),
                    page: index as u32,
                    scale: 150. / 96.,
                };
                let (width, height, rgba) = render(&request).map_err(|e| e.to_string())?;
                if width as u64 * height as u64 > 4_000_000 {
                    return Err("Page exceeds raster pixel limit".into());
                }
                let page = pdf42_core::FlattenedPage::from_rgba(
                    width,
                    height,
                    &rgba,
                    pw,
                    ph,
                    if index == target { Some(rect) } else { None },
                )?;
                compressed_total = compressed_total
                    .checked_add(page.compressed_rgb.len())
                    .ok_or("Flattened document too large")?;
                if compressed_total > 96 * 1024 * 1024 {
                    return Err("Flattened document exceeds the 96 MB compressed limit".into());
                }
                output.push(page);
            }
            Ok(output)
        })();
        let _ = tx.send(result);
    });
    rx
}

/// Build a print job from an immutable snapshot, retaining 150/300-DPI pixels.
pub fn print_async(
    bytes: Vec<u8>,
    pages: Vec<(usize, f64, f64)>,
    dpi: u32,
    progress: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::sync::mpsc::Receiver<Result<win42::PrintJob, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe { RoUninitialize() }
                }
            }
            let _apartment = Apartment;
            let mut out = Vec::new();
            let mut total = 0usize;
            for (index, pw, ph) in pages {
                if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                    return Err("Print preparation cancelled".into());
                }
                let width = (pw * dpi as f64 / 72.).ceil() as u64;
                let height = (ph * dpi as f64 / 72.).ceil() as u64;
                if width == 0
                    || height == 0
                    || width > 4096
                    || height > 4096
                    || width * height > 16_000_000
                {
                    return Err(
                        "A print page exceeds the raster size limit; choose a lower DPI".into(),
                    );
                }
                let request = Request {
                    generation: 0,
                    bytes: bytes.clone(),
                    page: index as u32,
                    scale: dpi as f64 / 96.,
                };
                let (pixel_width, pixel_height, rgba) =
                    render(&request).map_err(|e| e.to_string())?;
                total = total.checked_add(rgba.len()).ok_or("Print job too large")?;
                if total > 256 * 1024 * 1024 {
                    return Err("Print preparation exceeds the 256 MB raster limit; select fewer pages or lower DPI".into());
                }
                let geometry = win42::PrintGeometry {
                    width_twips: (pw * 20.).round() as i32,
                    height_twips: (ph * 20.).round() as i32,
                    margin_left_twips: 0,
                    margin_top_twips: 0,
                    margin_right_twips: 0,
                    margin_bottom_twips: 0,
                };
                if !geometry.valid() {
                    return Err("Invalid print page geometry".into());
                }
                out.push(win42::PrintPage {
                    geometry,
                    scene: win42::CanvasScene {
                        viewport: win42::Viewport::default(),
                        items: vec![win42::SceneItem::ImageScaled {
                            x: 0.,
                            y: 0.,
                            width: pw,
                            height: ph,
                            pixel_width,
                            pixel_height,
                            rgba,
                        }],
                    },
                });
                progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(win42::PrintJob::ActualSizeScenes(out))
        })();
        let _ = tx.send(result);
    });
    rx
}

/// Export pages one at a time into an atomic ZIP with per-image physical DPI metadata.
pub fn export_async(
    bytes: Vec<u8>,
    pages: Vec<(usize, f64, f64)>,
    dpi: u32,
    jpeg: bool,
    output: std::path::PathBuf,
    progress: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::sync::mpsc::Receiver<Result<(), String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            use std::io::Write;
            unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(|e| e.to_string())?;
            struct Apartment;
            impl Drop for Apartment {
                fn drop(&mut self) {
                    unsafe { RoUninitialize() }
                }
            }
            let _apartment = Apartment;
            let dir = output
                .parent()
                .filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| std::path::Path::new("."));
            let mut temporary = suite42_core::win32_path(dir)
                .and_then(tempfile::NamedTempFile::new_in)
                .map_err(|e| e.to_string())?;
            {
                let mut zip = zip::ZipWriter::new(temporary.as_file_mut());
                let mut total_encoded = 0usize;
                for (index, pw, ph) in pages {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        return Err("Image export cancelled".into());
                    }
                    let w = (pw * dpi as f64 / 72.).ceil() as u64;
                    let h = (ph * dpi as f64 / 72.).ceil() as u64;
                    if w == 0 || h == 0 || w > 4096 || h > 4096 || w * h > 16_000_000 {
                        return Err(
                            "A page exceeds the image raster limit; choose lower DPI".into()
                        );
                    }
                    let request = Request {
                        generation: 0,
                        bytes: bytes.clone(),
                        page: index as u32,
                        scale: dpi as f64 / 96.,
                    };
                    let (width, height, rgba) = render(&request).map_err(|e| e.to_string())?;
                    let mut encoded = Vec::new();
                    if jpeg {
                        use image::ImageEncoder;
                        let mut rgb = Vec::with_capacity(width as usize * height as usize * 3);
                        for p in rgba.chunks_exact(4) {
                            let a = p[3] as u32;
                            for v in &p[..3] {
                                rgb.push(((*v as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
                            }
                        }
                        let mut encoder =
                            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut encoded, 90);
                        encoder
                            .set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(dpi as u16));
                        encoder
                            .write_image(&rgb, width, height, image::ExtendedColorType::Rgb8)
                            .map_err(|e| e.to_string())?;
                    } else {
                        let mut encoder = png::Encoder::new(&mut encoded, width, height);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        let ppm = (dpi as f64 / 0.0254).round() as u32;
                        encoder.set_pixel_dims(Some(png::PixelDimensions {
                            xppu: ppm,
                            yppu: ppm,
                            unit: png::Unit::Meter,
                        }));
                        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
                        writer.write_image_data(&rgba).map_err(|e| e.to_string())?;
                    }
                    zip.start_file(
                        format!("page-{:04}.{}", index + 1, if jpeg { "jpg" } else { "png" }),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .map_err(|e| e.to_string())?;
                    zip.write_all(&encoded).map_err(|e| e.to_string())?;
                    total_encoded = total_encoded
                        .checked_add(encoded.len())
                        .ok_or("Image archive too large")?;
                    if total_encoded > 512 * 1024 * 1024 {
                        return Err("Image archive exceeds 512 MB".into());
                    }
                    progress.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                zip.finish().map_err(|e| e.to_string())?;
            }
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err("Image export cancelled".into());
            }
            temporary
                .persist_noclobber(suite42_core::win32_path(&output).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            Ok(())
        })();
        let _ = tx.send(result);
    });
    rx
}
