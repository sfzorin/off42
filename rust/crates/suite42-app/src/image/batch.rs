use img42_core::ImageDocument;
use std::io::Write;
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

pub struct Options {
    pub input: PathBuf,
    pub output: PathBuf,
    pub format: String,
    pub size: Option<(u32, u32)>,
    pub resize_mode: String,
    pub frames: String,
    pub jpeg_quality: u8,
}
pub struct Worker {
    pub cancel: Arc<AtomicBool>,
    pub rx: mpsc::Receiver<Event>,
}
pub enum Event {
    Outcome(String),
    Done(bool),
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

pub fn start(options: Options) -> Result<Worker, String> {
    if !options.input.is_dir() || !options.output.is_dir() {
        return Err("Input and output must be existing folders".into());
    }
    let mut paths: Vec<_> = std::fs::read_dir(&options.input)
        .map_err(|e| e.to_string())?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension().and_then(|x| x.to_str()).is_some_and(|x| {
                    [
                        "png", "jpg", "jpeg", "tif", "tiff", "gif", "webp", "bmp", "svg",
                    ]
                    .iter()
                    .any(|e| x.eq_ignore_ascii_case(e))
                })
        })
        .collect();
    paths.sort();
    if paths.is_empty() {
        return Err("No supported images found in input folder".into());
    }
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    std::thread::spawn(move || {
        for path in paths {
            if flag.load(Ordering::Relaxed) {
                break;
            }
            let result = convert(&path, &options, &flag);
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let _ = tx.send(Event::Outcome(format!(
                "{}: {}",
                name,
                match result {
                    Ok(()) => "completed".into(),
                    Err(e) => e,
                }
            )));
        }
        let _ = tx.send(Event::Done(flag.load(Ordering::Relaxed)));
    });
    Ok(Worker { cancel, rx })
}
fn convert(path: &Path, options: &Options, cancel: &AtomicBool) -> Result<(), String> {
    // tempfile passes both names directly to MoveFileExW on Windows. Canonicalizing
    // the existing parent provides extended-length drive or UNC paths for both.
    let output =
        std::fs::canonicalize(&options.output).map_err(|e| format!("output folder: {e}"))?;
    let stem = path.file_stem().ok_or("Invalid filename")?;
    let mut name = stem.to_os_string();
    name.push(match options.format.as_str() {
        "png" => ".png",
        "jpeg" => ".jpg",
        _ => ".tiff",
    });
    let destination = output.join(name);
    // Existing destinations, including source paths and other inputs, are never replaced.
    if destination.exists() {
        return Err("skipped: destination already exists".into());
    }
    let (mut docs, _) = ImageDocument::open_frames(path).map_err(|e| e.to_string())?;
    if options.frames == "first" {
        docs.truncate(1);
    }
    if docs.len() != 1 && options.format != "tiff" {
        return Err("skipped: all frames require TIFF; choose first frame or TIFF".into());
    }
    if let Some((w, h)) = options.size {
        for doc in &mut docs {
            let (w, h) = if options.resize_mode == "exact" {
                (w, h)
            } else {
                let mut scale = (w as f64 / doc.width() as f64).min(h as f64 / doc.height() as f64);
                if options.resize_mode == "fit-no-upscale" {
                    scale = scale.min(1.0);
                }
                (
                    (doc.width() as f64 * scale).round().max(1.0) as u32,
                    (doc.height() as f64 * scale).round().max(1.0) as u32,
                )
            };
            doc.resize_quality(w, h, "lanczos")
                .map_err(|e| e.to_string())?;
        }
    }
    let bytes = if docs.len() > 1 {
        let refs: Vec<_> = docs.iter().collect();
        ImageDocument::save_tiff_pages(&refs)
            .map_err(|e| e.to_string())?
            .0
    } else {
        docs.first()
            .ok_or("No frames")?
            .save_snapshot_format_quality(&options.format, options.jpeg_quality)
            .map_err(|e| e.to_string())?
            .0
    };
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled before publication".into());
    }
    let mut temp = tempfile::NamedTempFile::new_in(&output).map_err(|e| e.to_string())?;
    temp.write_all(&bytes)
        .and_then(|_| temp.flush())
        .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("cancelled before publication".into());
    }
    temp.persist_noclobber(&destination)
        .map_err(|e| format!("publication failed: {}", e.error))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_resizes_and_never_clobbers() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in");
        let output = dir.path().join("out");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        let src = input.join("a.png");
        image::RgbaImage::from_pixel(4, 3, image::Rgba([11, 22, 33, 255]))
            .save(&src)
            .unwrap();
        let options = Options {
            input,
            output: output.clone(),
            format: "tiff".into(),
            size: Some((2, 2)),
            resize_mode: "exact".into(),
            frames: "first".into(),
            jpeg_quality: 90,
        };
        let cancel = AtomicBool::new(false);
        convert(&src, &options, &cancel).unwrap();
        let path = output.join("a.tiff");
        let data = std::fs::read(&path).unwrap();
        let image = ImageDocument::open(&path).unwrap();
        assert_eq!((image.width(), image.height()), (2, 2));
        assert!(convert(&src, &options, &cancel).is_err());
        assert_eq!(std::fs::read(path).unwrap(), data);
        cancel.store(true, Ordering::Relaxed);
        std::fs::remove_file(output.join("a.tiff")).unwrap();
        assert!(convert(&src, &options, &cancel).is_err());
        assert!(!output.join("a.tiff").exists());
    }
    #[test]
    fn jpeg_fit_and_multiframe_policy() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("in");
        let output = dir.path().join("out");
        std::fs::create_dir(&input).unwrap();
        std::fs::create_dir(&output).unwrap();
        let src = input.join("animated.gif");
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
                let image = image::RgbaImage::from_pixel(4, 2, image::Rgba(color));
                encoder.encode_frame(image::Frame::new(image)).unwrap();
            }
        }
        std::fs::write(&src, &bytes).unwrap();
        let mut options = Options {
            input,
            output: output.clone(),
            format: "jpeg".into(),
            size: Some((8, 8)),
            resize_mode: "fit-no-upscale".into(),
            frames: "first".into(),
            jpeg_quality: 90,
        };
        let cancel = AtomicBool::new(false);
        convert(&src, &options, &cancel).unwrap();
        let jpg = ImageDocument::open(&output.join("animated.jpg")).unwrap();
        assert_eq!((jpg.width(), jpg.height()), (4, 2));
        assert!(jpg.pixels()[0] > 200 && jpg.pixels()[2] < 40);
        options.frames = "all".into();
        options.format = "png".into();
        assert!(convert(&src, &options, &cancel).is_err());
        options.format = "tiff".into();
        options.resize_mode = "fit".into();
        convert(&src, &options, &cancel).unwrap();
        let (pages, _) = ImageDocument::open_frames(&output.join("animated.tiff")).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!((pages[0].width(), pages[0].height()), (8, 4));
        assert!(pages[0].pixels()[0] > 200);
        assert!(pages[1].pixels()[2] > 200);
        assert_eq!(std::fs::read(src).unwrap(), bytes);
    }
    #[test]
    fn long_folder_path_and_jpeg_quality_affect_output() {
        let root = tempfile::tempdir().unwrap();
        let mut folder = root.path().to_path_buf();
        for n in 0..5 {
            folder.push(format!("{}{}", "long-path-component-".repeat(3), n));
        }
        std::fs::create_dir_all(&folder).unwrap();
        assert!(folder.to_string_lossy().len() > 260);
        let input = root.path().join("input");
        std::fs::create_dir(&input).unwrap();
        let src = input.join("detail.png");
        let image = image::RgbaImage::from_fn(48, 48, |x, y| {
            image::Rgba([(x * 17) as u8, (y * 19) as u8, ((x * y) * 7) as u8, 255])
        });
        image.save(&src).unwrap();
        let mut options = Options {
            input,
            output: folder.clone(),
            format: "jpeg".into(),
            size: None,
            resize_mode: "fit".into(),
            frames: "first".into(),
            jpeg_quality: 10,
        };
        let cancel = AtomicBool::new(false);
        convert(&src, &options, &cancel).unwrap();
        let path = folder.join("detail.jpg");
        let low = std::fs::read(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        options.jpeg_quality = 95;
        convert(&src, &options, &cancel).unwrap();
        let high = std::fs::read(&path).unwrap();
        assert_ne!(low, high);
        assert!(high.len() > low.len());
        assert_eq!(
            (
                ImageDocument::open(&path).unwrap().width(),
                ImageDocument::open(&path).unwrap().height()
            ),
            (48, 48)
        );
    }
}
