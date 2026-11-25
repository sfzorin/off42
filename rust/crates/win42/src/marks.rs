use std::{
    fs::{self, File},
    io::{Cursor, ErrorKind, Read},
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 4_000_000;

#[cfg(windows)]
pub fn library_directory() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Windows local app data folder is unavailable")?;
    let directory = local.join("off42").join("Signatures and Stamps");
    fs::create_dir_all(&directory).map_err(|e| format!("Could not open the library: {e}"))?;
    Ok(directory)
}

pub fn import_image(source: &Path, directory: &Path) -> Result<PathBuf, String> {
    let mut bytes = Vec::new();
    File::open(source)
        .and_then(|file| file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| format!("Could not read the image: {e}"))?;
    if bytes.len() > MAX_BYTES {
        return Err("The image exceeds 16 MB.".into());
    }
    let format = image::guess_format(&bytes).map_err(|_| "Choose a PNG or JPEG image.")?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Jpeg) {
        return Err("Choose a PNG or JPEG image.".into());
    }
    let mut reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let (width, height) = reader
        .into_dimensions()
        .map_err(|e| format!("Invalid image: {e}"))?;
    if width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(
            "Use an image up to 4096 pixels per side and four million pixels in total.".into(),
        );
    }
    reader = image::ImageReader::with_format(Cursor::new(&bytes), format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(32 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|e| format!("Invalid image: {e}"))?
        .into_rgba8();
    let mut png = Cursor::new(Vec::new());
    image
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| format!("Could not encode the image: {e}"))?;

    let stem: String = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .take(80)
        .collect();
    let stem = stem.trim().trim_end_matches('.');
    let stem = if stem.is_empty() { "Image" } else { stem };
    for suffix in 1..=1000 {
        let name = if suffix == 1 {
            format!("Mark - {stem}.png")
        } else {
            format!("Mark - {stem} ({suffix}).png")
        };
        let path = directory.join(name);
        match suite42_core::write_new(&path, png.get_ref()) {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("Could not save to the library: {e}")),
        }
    }
    Err("The library already has too many images with this name.".into())
}

pub fn library_image(path: &Path, directory: &Path) -> Result<PathBuf, String> {
    let root = fs::canonicalize(directory).map_err(|e| e.to_string())?;
    let path = fs::canonicalize(path).map_err(|e| e.to_string())?;
    if path.parent() != Some(&root)
        || !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        return Err("Choose an image from the library, or import it first.".into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_a_reusable_copy_preserving_transparency_and_existing_entries() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("Example.png");
        let directory = temp.path().join("library");
        fs::create_dir(&directory).unwrap();
        let rgba = image::RgbaImage::from_pixel(3, 2, image::Rgba([12, 32, 85, 100]));
        rgba.save(&source).unwrap();
        let first = import_image(&source, &directory).unwrap();
        let second = import_image(&source, &directory).unwrap();
        assert_ne!(first, second);
        fs::remove_file(&source).unwrap();
        for entry in [first, second] {
            let path = library_image(&entry, &directory).unwrap();
            assert_eq!(image::open(path).unwrap().into_rgba8(), rgba);
        }
    }

    #[test]
    fn rejects_invalid_or_outside_images_without_publishing() {
        let temp = tempfile::tempdir().unwrap();
        let directory = temp.path().join("library");
        fs::create_dir(&directory).unwrap();
        let source = temp.path().join("Not an image.png");
        fs::write(&source, b"not an image").unwrap();
        assert!(import_image(&source, &directory).is_err());
        assert!(library_image(&source, &directory).is_err());
        assert_eq!(fs::read_dir(directory).unwrap().count(), 0);
    }
}
