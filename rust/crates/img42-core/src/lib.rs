//! Portable raster editor with bounded codecs and versioned image snapshots.
mod exif_metadata;
use image::{ImageFormat, ImageReader, Limits};
use std::{
    fmt,
    fs::File,
    io::{self, BufReader, Cursor, Read, Write},
    path::Path,
    sync::Arc,
};

const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_DIMENSION: u32 = 16_384;
const MAX_PIXELS: u64 = 16_000_000;
const HISTORY_LIMIT: usize = 4;

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Decode(image::ImageError),
    UnsupportedFormat,
    IncompatibleProfile,
    TooLarge,
    InvalidImage,
    DestinationExists,
    MultiframeRequiresGif,
    MultipageTiffUnsupported,
    MultiframeRequiresNavigation,
    SvgResourcesDisallowed,
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O: {e}"),
            Self::Decode(e) => write!(f, "image: {e}"),
            Self::UnsupportedFormat => write!(f, "unsupported image format"),
            Self::IncompatibleProfile => write!(f, "The embedded ICC profile is incompatible with the output color model; color-managed conversion is unavailable"),
            Self::TooLarge => write!(f, "image exceeds size limits"),
            Self::InvalidImage => write!(f, "invalid image dimensions or pixels"),
            Self::SvgResourcesDisallowed => write!(
                f,
                "SVG resource is external, invalid, unsupported or exceeds embedded-image limits"
            ),
            Self::MultiframeRequiresNavigation => write!(
                f,
                "Animated image requires frame navigation; import its frames separately"
            ),
            Self::MultipageTiffUnsupported => write!(
                f,
                "Multi-page TIFF is not yet supported; the file was not opened"
            ),
            Self::MultiframeRequiresGif => write!(
                f,
                "Multi-frame images require GIF export; other formats would discard frames"
            ),
            Self::DestinationExists => write!(f, "destination already exists"),
        }
    }
}
impl std::error::Error for Error {}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<image::ImageError> for Error {
    fn from(e: image::ImageError) -> Self {
        Self::Decode(e)
    }
}

/// Validate and retain bounded PNG color-interpretation chunks. None means the
/// source has ambiguous/invalid color metadata and must not be overwritten.
fn png_color_chunks(bytes: &[u8]) -> Option<Vec<([u8; 4], Vec<u8>)>> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return None;
    }
    let mut pos = 8usize;
    let mut chunks = Vec::new();
    let mut seen_icc = false;
    let mut seen_srgb = false;
    let mut seen_pixels = false;
    while pos.checked_add(12)? <= bytes.len() {
        let n = u32::from_be_bytes(bytes.get(pos..pos + 4)?.try_into().ok()?) as usize;
        let end = pos.checked_add(12)?.checked_add(n)?;
        if end > bytes.len() {
            return None;
        }
        let tag: [u8; 4] = bytes[pos + 4..pos + 8].try_into().ok()?;
        let data = &bytes[pos + 8..pos + 8 + n];
        if tag == *b"PLTE" || tag == *b"IDAT" {
            seen_pixels = true;
        }
        if tag == *b"iCCP" {
            if seen_icc || seen_pixels {
                return None;
            }
            seen_icc = true;
        }
        if matches!(&tag, b"gAMA" | b"cHRM" | b"sRGB") {
            if seen_pixels || chunks.iter().any(|(t, _)| *t == tag) {
                return None;
            }
            let valid = match &tag {
                b"gAMA" => n == 4 && data != [0, 0, 0, 0],
                b"cHRM" => n == 32,
                b"sRGB" => {
                    seen_srgb = true;
                    n == 1 && data[0] <= 3
                }
                _ => false,
            };
            if !valid {
                return None;
            }
            let mut crc = crc32fast::Hasher::new();
            crc.update(&tag);
            crc.update(data);
            if crc.finalize() != u32::from_be_bytes(bytes[pos + 8 + n..end].try_into().ok()?) {
                return None;
            }
            chunks.push((tag, data.to_vec()));
        }
        pos = end;
        if tag == *b"IEND" {
            if seen_icc && seen_srgb {
                return None;
            }
            chunks.sort_by_key(|(tag, _)| match tag {
                b"gAMA" => 0,
                b"cHRM" => 1,
                _ => 2,
            });
            return Some(chunks);
        }
    }
    None
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SampleModel {
    Gray,
    GrayAlpha,
    Rgb,
    Rgba,
}
#[derive(Clone, PartialEq, Eq)]
struct Raster {
    width: u32,
    height: u32,
    pixels: Arc<[u8]>,
    pixels16: Option<Arc<[u16]>>,
    sample_model: SampleModel,
    dpi: Option<(u32, u32)>,
}
impl Raster {
    fn new(width: u32, height: u32, pixels: Vec<u8>) -> Result<Self, Error> {
        let count = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 {
            return Err(Error::InvalidImage);
        }
        if width > MAX_DIMENSION || height > MAX_DIMENSION || count > MAX_PIXELS {
            return Err(Error::TooLarge);
        }
        if pixels.len() as u64 != count * 4 {
            return Err(Error::InvalidImage);
        }
        Ok(Self {
            width,
            height,
            pixels: pixels.into(),
            pixels16: None,
            sample_model: SampleModel::Rgba,
            dpi: None,
        })
    }
    fn new16(width: u32, height: u32, pixels: Vec<u16>) -> Result<Self, Error> {
        let display = pixels
            .iter()
            .map(|v| ((u32::from(*v) + 128) / 257) as u8)
            .collect();
        let mut raster = Self::new(width, height, display)?;
        raster.pixels16 = Some(pixels.into());
        Ok(raster)
    }
    fn orient(&self, orientation: u16) -> Self {
        let swap = (5..=8).contains(&orientation);
        let (w, h) = if swap {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        };
        let mut out = vec![0; self.pixels.len()];
        let mut out16 = self.pixels16.as_ref().map(|v| vec![0u16; v.len()]);
        for y in 0..self.height {
            for x in 0..self.width {
                let (dx, dy) = match orientation {
                    2 => (self.width - 1 - x, y),
                    3 => (self.width - 1 - x, self.height - 1 - y),
                    4 => (x, self.height - 1 - y),
                    5 => (y, x),
                    6 => (self.height - 1 - y, x),
                    7 => (self.height - 1 - y, self.width - 1 - x),
                    8 => (y, self.width - 1 - x),
                    _ => (x, y),
                };
                let src = ((y as usize * self.width as usize) + x as usize) * 4;
                let dst = ((dy as usize * w as usize) + dx as usize) * 4;
                out[dst..dst + 4].copy_from_slice(&self.pixels[src..src + 4]);
                if let (Some(source), Some(target)) = (&self.pixels16, &mut out16) {
                    target[dst..dst + 4].copy_from_slice(&source[src..src + 4]);
                }
            }
        }
        Self {
            width: w,
            height: h,
            pixels: out.into(),
            pixels16: out16.map(Into::into),
            sample_model: self.sample_model,
            dpi: if swap {
                self.dpi.map(|(x, y)| (y, x))
            } else {
                self.dpi
            },
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum AnimationLoop {
    #[default]
    Once,
    Finite(u16),
    Infinite,
}
pub struct SaveState(Raster);

pub struct ImageDocument {
    exif: Option<exif_metadata::Metadata>,
    png_color: Vec<([u8; 4], Vec<u8>)>,
    tiff_text: Vec<(tiff::tags::Tag, String)>,
    icc: Option<Vec<u8>>,
    current: Raster,
    saved: Raster,
    undo: Vec<Raster>,
    redo: Vec<Raster>,
}
impl ImageDocument {
    pub fn from_rgba(
        width: u32,
        height: u32,
        pixels: Vec<u8>,
        dpi: Option<(u32, u32)>,
    ) -> Result<Self, Error> {
        if dpi.is_some_and(|(x, y)| !(1..=9600).contains(&x) || !(1..=9600).contains(&y)) {
            return Err(Error::InvalidImage);
        }
        let mut raster = Raster::new(width, height, pixels)?;
        raster.dpi = dpi;
        Ok(Self {
            exif: None,
            png_color: Vec::new(),
            tiff_text: Vec::new(),
            icc: None,
            saved: raster.clone(),
            current: raster,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    pub fn is_16bit(&self) -> bool {
        self.current.pixels16.is_some()
    }
    pub fn dpi(&self) -> Option<(u32, u32)> {
        self.current.dpi
    }
    /// Render a self-contained SVG snapshot with a bounded canvas and bundled font.
    /// Bounded PNG/JPEG data URLs are supported; local/network resources are rejected.
    pub fn open_svg(bytes: &[u8]) -> Result<Self, Error> {
        use std::sync::atomic::{AtomicBool, Ordering};
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::TooLarge);
        }
        // Avoid unbounded SVGZ expansion and prohibit XML document types/entities.
        if bytes.starts_with(&[0x1f, 0x8b]) || bytes.windows(9).any(|v| v == b"<!DOCTYPE") {
            return Err(Error::UnsupportedFormat);
        }
        let xml = std::str::from_utf8(bytes).map_err(|_| Error::InvalidImage)?;
        let tree = roxmltree::Document::parse(xml).map_err(|_| Error::InvalidImage)?;
        for node in tree.descendants().filter(|n| n.is_element()) {
            for attr in node.attributes() {
                if attr.name().eq_ignore_ascii_case("href") {
                    let value = attr.value().trim();
                    if !value.starts_with('#')
                        && !(node.tag_name().name() == "image" && value.starts_with("data:"))
                    {
                        return Err(Error::SvgResourcesDisallowed);
                    }
                }
                let value = attr.value().to_ascii_lowercase();
                if value.contains("@import") {
                    return Err(Error::SvgResourcesDisallowed);
                }
                for part in value.split("url(").skip(1) {
                    let target = part.trim_start().trim_start_matches(['\"', '\'']);
                    if !target.starts_with('#') {
                        return Err(Error::SvgResourcesDisallowed);
                    }
                }
            }
            if node.tag_name().name() == "style" {
                let css = node.text().unwrap_or("").to_ascii_lowercase();
                if css.contains("@import")
                    || css.split("url(").skip(1).any(|part| {
                        !part
                            .trim_start()
                            .trim_start_matches(['\"', '\''])
                            .starts_with('#')
                    })
                {
                    return Err(Error::SvgResourcesDisallowed);
                }
            }
        }
        let rejected = Arc::new(AtomicBool::new(false));
        let mut options = resvg::usvg::Options::default();
        options.font_family = "Noto Sans".into();
        options.fontdb_mut().load_font_data(
            include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf").to_vec(),
        );
        let a = rejected.clone();
        let b = rejected.clone();
        let used = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        options.image_href_resolver = resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(move |mime, data, _| {
                use image::ImageDecoder;
                let format = match mime {
                    "image/png" => ImageFormat::Png,
                    "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
                    _ => {
                        a.store(true, Ordering::Relaxed);
                        return None;
                    }
                };
                if data.len() > 2 * 1024 * 1024 || image::guess_format(&data).ok() != Some(format) {
                    a.store(true, Ordering::Relaxed);
                    return None;
                }
                let mut reader = ImageReader::with_format(Cursor::new(data.as_slice()), format);
                let mut limits = Limits::default();
                limits.max_image_width = Some(4096);
                limits.max_image_height = Some(4096);
                limits.max_alloc = Some(16 * 1024 * 1024);
                reader.limits(limits);
                let Ok(decoder) = reader.into_decoder() else {
                    a.store(true, Ordering::Relaxed);
                    return None;
                };
                let (w, h) = decoder.dimensions();
                let count = w as usize * h as usize;
                if count == 0
                    || count > 4_000_000
                    || used
                        .fetch_add(count, Ordering::Relaxed)
                        .saturating_add(count)
                        > 4_000_000
                    || image::DynamicImage::from_decoder(decoder).is_err()
                {
                    a.store(true, Ordering::Relaxed);
                    return None;
                }
                Some(match format {
                    ImageFormat::Png => resvg::usvg::ImageKind::PNG(data),
                    _ => resvg::usvg::ImageKind::JPEG(data),
                })
            }),
            resolve_string: Box::new(move |_, _| {
                b.store(true, Ordering::Relaxed);
                None
            }),
        };
        let tree =
            resvg::usvg::Tree::from_data(bytes, &options).map_err(|_| Error::InvalidImage)?;
        if rejected.load(Ordering::Relaxed) {
            return Err(Error::SvgResourcesDisallowed);
        }
        let width = tree.size().width().ceil() as u32;
        let height = tree.size().height().ceil() as u32;
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Err(Error::TooLarge);
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height).ok_or(Error::TooLarge)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        let mut pixels = pixmap.take();
        for p in pixels.chunks_exact_mut(4) {
            let a = u32::from(p[3]);
            if a > 0 {
                for c in &mut p[..3] {
                    *c = ((u32::from(*c) * 255 + a / 2) / a).min(255) as u8;
                }
            }
        }
        Self::from_rgba(width, height, pixels, Some((96, 96)))
    }
    pub fn open(path: &Path) -> Result<Self, Error> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Self::open_svg(&bytes);
        }
        let format = image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)?;
        if format == ImageFormat::Tiff {
            let decoder = tiff::decoder::Decoder::new(Cursor::new(&bytes))
                .map_err(|_| Error::InvalidImage)?;
            if decoder.more_images() {
                return Err(Error::MultipageTiffUnsupported);
            }
            let (mut pages, _) = Self::open_frames(path)?;
            return pages.pop().ok_or(Error::InvalidImage);
        }
        if !matches!(
            format,
            ImageFormat::Png
                | ImageFormat::Jpeg
                | ImageFormat::Bmp
                | ImageFormat::Tiff
                | ImageFormat::Gif
                | ImageFormat::WebP
        ) {
            return Err(Error::UnsupportedFormat);
        }
        // Single-raster callers must never silently discard animation.
        if format == ImageFormat::Gif {
            use image::AnimationDecoder;
            let mut frames =
                image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))?.into_frames();
            if let Some(frame) = frames.next() {
                frame?;
            }
            if let Some(frame) = frames.next() {
                frame?;
                return Err(Error::MultiframeRequiresNavigation);
            }
        }
        if format == ImageFormat::WebP
            && image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))?.has_animation()
        {
            return Err(Error::MultiframeRequiresNavigation);
        }
        let orientation = exif::Reader::new()
            .read_from_container(&mut BufReader::new(Cursor::new(&bytes)))
            .ok()
            .and_then(|e| {
                e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                    .and_then(|v| v.value.get_uint(0))
            })
            .filter(|v| (1..=8).contains(v))
            .unwrap_or(1) as u16;
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_PIXELS * 8);
        reader.limits(limits);
        let mut decoder = reader.into_decoder()?;
        use image::ImageDecoder;
        let icc = decoder.icc_profile()?;
        let exif = decoder
            .exif_metadata()?
            .map(exif_metadata::Metadata::parse)
            .transpose()
            .map_err(|_| Error::InvalidImage)?;
        let (w, h) = decoder.dimensions();
        if u64::from(w) * u64::from(h) > MAX_PIXELS {
            return Err(Error::TooLarge);
        }
        let color = decoder.color_type();
        let is_png16 = format == ImageFormat::Png
            && matches!(
                color,
                image::ColorType::L16
                    | image::ColorType::La16
                    | image::ColorType::Rgb16
                    | image::ColorType::Rgba16
            );
        if is_png16 && u64::from(w) * u64::from(h) * 12 > MAX_PIXELS * 4 {
            return Err(Error::TooLarge);
        }
        let image = image::DynamicImage::from_decoder(decoder)?;
        let mut raster = if is_png16 {
            let mut raster = Raster::new16(w, h, image.into_rgba16().into_raw())?;
            raster.sample_model = match color {
                image::ColorType::L16 => SampleModel::Gray,
                image::ColorType::La16 => SampleModel::GrayAlpha,
                image::ColorType::Rgb16 => SampleModel::Rgb,
                _ => SampleModel::Rgba,
            };
            raster.orient(orientation)
        } else {
            Raster::new(w, h, image.into_rgba8().into_raw())?.orient(orientation)
        };
        raster.dpi = Self::source_dpi(path)?;
        Ok(Self {
            exif,
            png_color: if format == ImageFormat::Png {
                png_color_chunks(&bytes).unwrap_or_default()
            } else {
                Vec::new()
            },
            tiff_text: Vec::new(),
            icc,
            saved: raster.clone(),
            current: raster,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
    /// Read physical resolution where encoded; absent/invalid density remains unknown.
    pub fn source_dpi(path: &Path) -> Result<Option<(u32, u32)>, Error> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Ok(Some((96, 96)));
        }
        let format = image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)?;
        let mut dpi = None;
        let exif = exif::Reader::new()
            .read_from_container(&mut BufReader::new(Cursor::new(&bytes)))
            .ok();
        if let Some(ref exif) = exif {
            let rational = |tag| {
                exif.get_field(tag, exif::In::PRIMARY)
                    .and_then(|f| match &f.value {
                        exif::Value::Rational(v) => {
                            v.first().filter(|r| r.denom != 0).map(|r| r.to_f64())
                        }
                        _ => None,
                    })
            };
            if let (Some(x), Some(y)) = (
                rational(exif::Tag::XResolution),
                rational(exif::Tag::YResolution),
            ) {
                let unit = exif
                    .get_field(exif::Tag::ResolutionUnit, exif::In::PRIMARY)
                    .and_then(|f| f.value.get_uint(0));
                let factor = match unit {
                    Some(2) => 1.0,
                    Some(3) => 2.54,
                    _ => 0.0,
                };
                if factor > 0.0 {
                    dpi = Some(((x * factor).round() as u32, (y * factor).round() as u32));
                }
            }
        }
        if dpi.is_none() && format == ImageFormat::Png {
            let mut p = 8usize;
            while p + 12 <= bytes.len() {
                let n = u32::from_be_bytes(bytes[p..p + 4].try_into().unwrap()) as usize;
                let Some(end) = p.checked_add(12).and_then(|v| v.checked_add(n)) else {
                    break;
                };
                if end > bytes.len() {
                    break;
                }
                if &bytes[p + 4..p + 8] == b"pHYs" && n == 9 && bytes[p + 16] == 1 {
                    let x = u32::from_be_bytes(bytes[p + 8..p + 12].try_into().unwrap());
                    let y = u32::from_be_bytes(bytes[p + 12..p + 16].try_into().unwrap());
                    dpi = Some((
                        (x as f64 * 0.0254).round() as u32,
                        (y as f64 * 0.0254).round() as u32,
                    ));
                    break;
                }
                p = end;
            }
        }
        if dpi.is_none() && format == ImageFormat::Jpeg {
            if let Some(i) = bytes.windows(5).position(|v| v == b"JFIF\0") {
                if i + 12 <= bytes.len() {
                    let unit = bytes[i + 7];
                    let x = u16::from_be_bytes([bytes[i + 8], bytes[i + 9]]) as f64;
                    let y = u16::from_be_bytes([bytes[i + 10], bytes[i + 11]]) as f64;
                    let f = match unit {
                        1 => 1.0,
                        2 => 2.54,
                        _ => 0.0,
                    };
                    if f > 0.0 {
                        dpi = Some(((x * f).round() as u32, (y * f).round() as u32));
                    }
                }
            }
        }
        if exif
            .as_ref()
            .and_then(|e| e.get_field(exif::Tag::Orientation, exif::In::PRIMARY))
            .and_then(|f| f.value.get_uint(0))
            .is_some_and(|v| (5..=8).contains(&v))
        {
            dpi = dpi.map(|(x, y)| (y, x));
        }
        Ok(dpi.filter(|(x, y)| (1..=9600).contains(x) && (1..=9600).contains(y)))
    }
    pub fn source_page_dpi(path: &Path) -> Result<Vec<Option<(u32, u32)>>, Error> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Ok(vec![Some((96, 96))]);
        }
        if image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)? != ImageFormat::Tiff {
            return Ok(vec![Self::source_dpi(path)?]);
        }
        use tiff::{
            decoder::{ifd::Value, Decoder},
            tags::Tag,
        };
        let mut d = Decoder::new(Cursor::new(&bytes)).map_err(|_| Error::InvalidImage)?;
        let mut result = Vec::new();
        loop {
            let rational = |v| match v {
                Ok(Value::Rational(n, d)) if d > 0 => Some(n as f64 / d as f64),
                _ => None,
            };
            let x = rational(d.get_tag(Tag::XResolution));
            let y = rational(d.get_tag(Tag::YResolution));
            let unit = d.get_tag_unsigned::<u16>(Tag::ResolutionUnit).unwrap_or(1);
            let factor = match unit {
                2 => 1.0,
                3 => 2.54,
                _ => 0.0,
            };
            let mut dpi = x
                .zip(y)
                .filter(|_| factor > 0.0)
                .map(|(x, y)| ((x * factor).round() as u32, (y * factor).round() as u32))
                .filter(|(x, y)| (1..=9600).contains(x) && (1..=9600).contains(y));
            let orientation = d.get_tag_unsigned::<u16>(Tag::Orientation).unwrap_or(1);
            if (5..=8).contains(&orientation) {
                dpi = dpi.map(|(x, y)| (y, x));
            }
            result.push(dpi);
            if result.len() > 256 {
                return Err(Error::TooLarge);
            }
            if !d.more_images() {
                break;
            }
            d.next_image().map_err(|_| Error::InvalidImage)?;
        }
        Ok(result)
    }
    /// Detect source properties that the current 8-bit encoders cannot preserve.
    pub fn has_conversion_risk(path: &Path) -> Result<bool, Error> {
        use image::ImageDecoder;
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Ok(true);
        }
        let format = image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)?;
        if format == ImageFormat::Tiff {
            let mut d = tiff::decoder::Decoder::new(Cursor::new(&bytes))
                .map_err(|_| Error::InvalidImage)?;
            loop {
                let _ = d.colortype().map_err(|_| Error::InvalidImage)?;
                use tiff::tags::Tag;
                if [Tag::ExifDirectory, Tag::GpsDirectory]
                    .iter()
                    .any(|tag| d.get_tag(*tag).is_ok())
                {
                    return Ok(true);
                }
                if !d.more_images() {
                    break;
                }
                d.next_image().map_err(|_| Error::InvalidImage)?;
            }
        }
        if format == ImageFormat::Png {
            if png_color_chunks(&bytes).is_none() {
                return Ok(true);
            }
            // Metadata not represented by the editor must not be discarded in place.
            let mut pos = 8usize;
            while pos.checked_add(12).is_some_and(|end| end <= bytes.len()) {
                let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
                let Some(end) = pos.checked_add(12).and_then(|n| n.checked_add(len)) else {
                    return Err(Error::InvalidImage);
                };
                if end > bytes.len() {
                    return Err(Error::InvalidImage);
                }
                let tag = &bytes[pos + 4..pos + 8];
                if !matches!(
                    tag,
                    b"IHDR"
                        | b"IDAT"
                        | b"IEND"
                        | b"PLTE"
                        | b"tRNS"
                        | b"pHYs"
                        | b"iCCP"
                        | b"eXIf"
                        | b"gAMA"
                        | b"cHRM"
                        | b"sRGB"
                ) {
                    return Ok(true);
                }
                pos = end;
                if tag == b"IEND" {
                    break;
                }
            }
        }
        if format == ImageFormat::Gif {
            use image::AnimationDecoder;
            let mut frames =
                image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))?.into_frames();
            if let Some(frame) = frames.next() {
                frame?;
            }
            if let Some(frame) = frames.next() {
                frame?;
                return Ok(true);
            }
        }
        if format == ImageFormat::WebP
            && image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))?.has_animation()
        {
            return Ok(true);
        }
        let mut reader = ImageReader::with_format(Cursor::new(&bytes), format);
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DIMENSION);
        limits.max_image_height = Some(MAX_DIMENSION);
        limits.max_alloc = Some(MAX_PIXELS * 8);
        reader.limits(limits);
        let mut d = reader.into_decoder()?;
        let color = d.color_type();
        if !matches!(format, ImageFormat::Tiff | ImageFormat::Png)
            && color.bits_per_pixel() > u16::from(color.channel_count()) * 8
        {
            return Ok(true);
        }
        let exif = d.exif_metadata()?;
        if exif.as_ref().is_some_and(|raw| {
            !matches!(format, ImageFormat::Png | ImageFormat::Jpeg)
                || exif_metadata::Metadata::parse(raw.clone()).map_or(true, |m| m.lossy)
        }) || (!matches!(
            format,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::Tiff
        ) && d.icc_profile()?.is_some())
        {
            return Ok(true);
        }
        Ok(false)
    }
    /// Decode all GIF or animated WebP frames with bounded aggregate memory.
    pub fn open_frames(path: &Path) -> Result<(Vec<Self>, Vec<image::Delay>), Error> {
        use image::AnimationDecoder;
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Ok((
                vec![Self::open_svg(&bytes)?],
                vec![image::Delay::from_numer_denom_ms(0, 1)],
            ));
        }
        let format = image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)?;
        if format == ImageFormat::Tiff {
            use tiff::{
                decoder::{Decoder, DecodingResult},
                tags::Tag,
                ColorType,
            };
            let mut decoder = Decoder::new(Cursor::new(&bytes)).map_err(|_| Error::InvalidImage)?;
            let mut docs = Vec::new();
            let page_dpi = Self::source_page_dpi(path)?;
            let mut total = 0u64;
            loop {
                let (w, h) = decoder.dimensions().map_err(|_| Error::InvalidImage)?;
                if w == 0
                    || h == 0
                    || w > MAX_DIMENSION
                    || h > MAX_DIMENSION
                    || u64::from(w) * u64::from(h) > MAX_PIXELS
                {
                    return Err(Error::TooLarge);
                }
                total += u64::from(w) * u64::from(h) * 4;
                if total > MAX_PIXELS * 4 || docs.len() >= 256 {
                    return Err(Error::TooLarge);
                }
                let color = decoder.colortype().map_err(|_| Error::InvalidImage)?;
                let icc = decoder.get_tag_u8_vec(Tag::IccProfile).ok();
                let tiff_text = [
                    Tag::Artist,
                    Tag::Copyright,
                    Tag::DateTime,
                    Tag::ImageDescription,
                    Tag::Make,
                    Tag::Model,
                    Tag::Software,
                    Tag::HostComputer,
                ]
                .into_iter()
                .filter_map(|tag| match decoder.get_tag(tag) {
                    Ok(tiff::decoder::ifd::Value::Ascii(value)) => Some((tag, value)),
                    _ => None,
                })
                .collect();
                let orientation = decoder
                    .get_tag_unsigned::<u16>(Tag::Orientation)
                    .unwrap_or(1);
                let channels = match color {
                    ColorType::Gray(8 | 16) => 1,
                    ColorType::GrayA(8 | 16) => 2,
                    ColorType::RGB(8 | 16) => 3,
                    ColorType::RGBA(8 | 16) => 4,
                    _ => return Err(Error::UnsupportedFormat),
                };
                let orient = if (1..=8).contains(&orientation) {
                    orientation
                } else {
                    1
                };
                let mut raster = match decoder.read_image().map_err(|_| Error::InvalidImage)? {
                    DecodingResult::U8(samples) => {
                        if samples.len() != w as usize * h as usize * channels {
                            return Err(Error::InvalidImage);
                        }
                        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
                        for p in samples.chunks_exact(channels) {
                            match channels {
                                1 => rgba.extend_from_slice(&[p[0], p[0], p[0], 255]),
                                2 => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
                                3 => rgba.extend_from_slice(&[p[0], p[1], p[2], 255]),
                                _ => rgba.extend_from_slice(p),
                            }
                        }
                        Raster::new(w, h, rgba)?.orient(orient)
                    }
                    DecodingResult::U16(samples) => {
                        if samples.len() != w as usize * h as usize * channels {
                            return Err(Error::InvalidImage);
                        }
                        total += u64::from(w) * u64::from(h) * 8;
                        if total > MAX_PIXELS * 4 {
                            return Err(Error::TooLarge);
                        }
                        let mut rgba = Vec::with_capacity(w as usize * h as usize * 4);
                        for p in samples.chunks_exact(channels) {
                            match channels {
                                1 => rgba.extend_from_slice(&[p[0], p[0], p[0], u16::MAX]),
                                2 => rgba.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
                                3 => rgba.extend_from_slice(&[p[0], p[1], p[2], u16::MAX]),
                                _ => rgba.extend_from_slice(p),
                            }
                        }
                        Raster::new16(w, h, rgba)?.orient(orient)
                    }
                    _ => return Err(Error::UnsupportedFormat),
                };
                raster.dpi = page_dpi.get(docs.len()).copied().flatten();
                docs.push(Self {
                    exif: None,
                    png_color: Vec::new(),
                    tiff_text,
                    icc,
                    saved: raster.clone(),
                    current: raster,
                    undo: Vec::new(),
                    redo: Vec::new(),
                });
                if !decoder.more_images() {
                    break;
                }
                decoder.next_image().map_err(|_| Error::InvalidImage)?;
            }
            let delays = vec![image::Delay::from_numer_denom_ms(0, 1); docs.len()];
            return Ok((docs, delays));
        }
        let frames = match format {
            ImageFormat::Gif => {
                Some(image::codecs::gif::GifDecoder::new(Cursor::new(&bytes))?.into_frames())
            }
            ImageFormat::WebP => {
                let decoder = image::codecs::webp::WebPDecoder::new(Cursor::new(&bytes))?;
                if decoder.has_animation() {
                    Some(decoder.into_frames())
                } else {
                    None
                }
            }
            _ => None,
        };
        if let Some(frames) = frames {
            let mut docs = Vec::new();
            let mut delays = Vec::new();
            let mut total = 0u64;
            for frame in frames {
                let frame = frame?;
                let delay = frame.delay();
                let image = frame.into_buffer();
                let raster = Raster::new(image.width(), image.height(), image.into_raw())?;
                total += raster.pixels.len() as u64;
                if total > MAX_PIXELS * 4 || docs.len() >= 256 {
                    return Err(Error::TooLarge);
                }
                docs.push(Self {
                    exif: None,
                    png_color: Vec::new(),
                    tiff_text: Vec::new(),
                    icc: None,
                    saved: raster.clone(),
                    current: raster,
                    undo: Vec::new(),
                    redo: Vec::new(),
                });
                delays.push(delay);
            }
            if docs.is_empty() {
                return Err(Error::InvalidImage);
            }
            Ok((docs, delays))
        } else {
            Ok((
                vec![Self::open(path)?],
                vec![image::Delay::from_numer_denom_ms(0, 1)],
            ))
        }
    }
    /// Read animation playback count. GIF stores repetitions; WebP stores total plays.
    pub fn animation_loop(path: &Path) -> Result<AnimationLoop, Error> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_FILE_BYTES {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes)?;
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
        {
            return Ok(AnimationLoop::Once);
        }
        let format = image::guess_format(&bytes).map_err(|_| Error::UnsupportedFormat)?;
        match format {
            ImageFormat::Gif => {
                let mut decoder = gif::DecodeOptions::new()
                    .read_info(Cursor::new(&bytes))
                    .map_err(|_| Error::InvalidImage)?;
                while decoder
                    .read_next_frame()
                    .map_err(|_| Error::InvalidImage)?
                    .is_some()
                {}
                Ok(match decoder.repeat() {
                    gif::Repeat::Infinite => AnimationLoop::Infinite,
                    gif::Repeat::Finite(0) => AnimationLoop::Once,
                    gif::Repeat::Finite(n) => AnimationLoop::Finite(n),
                })
            }
            ImageFormat::WebP => {
                let decoder = image_webp::WebPDecoder::new(Cursor::new(&bytes))
                    .map_err(|_| Error::InvalidImage)?;
                Ok(match decoder.loop_count() {
                    image_webp::LoopCount::Forever => AnimationLoop::Infinite,
                    image_webp::LoopCount::Times(n) if n.get() == 1 => AnimationLoop::Once,
                    image_webp::LoopCount::Times(n) => AnimationLoop::Finite(n.get() - 1),
                })
            }
            _ => Ok(AnimationLoop::Once),
        }
    }
    /// Encode each page independently into a lossless RGBA TIFF.
    pub fn save_tiff_pages(pages: &[&Self]) -> Result<(Vec<u8>, Vec<SaveState>), Error> {
        if pages.is_empty() {
            return Err(Error::InvalidImage);
        }
        let mut output = Cursor::new(Vec::new());
        {
            let mut encoder =
                tiff::encoder::TiffEncoder::new(&mut output).map_err(|_| Error::InvalidImage)?;
            for page in pages {
                if page
                    .icc
                    .as_ref()
                    .is_some_and(|p| p.len() >= 20 && &p[16..20] == b"GRAY")
                {
                    return Err(Error::IncompatibleProfile);
                }
                let r = &page.current;
                macro_rules! write_page {
                    ($kind:ty, $pixels:expr) => {{
                        let mut image = encoder
                            .new_image::<$kind>(r.width, r.height)
                            .map_err(|_| Error::InvalidImage)?;
                        if let Some((x, y)) = r.dpi {
                            use tiff::encoder::Rational;
                            image
                                .encoder()
                                .write_tag(tiff::tags::Tag::XResolution, Rational { n: x, d: 1 })
                                .map_err(|_| Error::InvalidImage)?;
                            image
                                .encoder()
                                .write_tag(tiff::tags::Tag::YResolution, Rational { n: y, d: 1 })
                                .map_err(|_| Error::InvalidImage)?;
                            image
                                .encoder()
                                .write_tag(tiff::tags::Tag::ResolutionUnit, 2u16)
                                .map_err(|_| Error::InvalidImage)?;
                        }
                        for (tag, text) in &page.tiff_text {
                            image
                                .encoder()
                                .write_tag(*tag, text.as_str())
                                .map_err(|_| Error::InvalidImage)?;
                        }
                        if let Some(icc) = &page.icc {
                            image
                                .encoder()
                                .write_tag(tiff::tags::Tag::IccProfile, icc.as_slice())
                                .map_err(|_| Error::InvalidImage)?;
                        }
                        image.write_data($pixels).map_err(|_| Error::InvalidImage)?;
                    }};
                }
                if let Some(pixels) = &r.pixels16 {
                    write_page!(tiff::encoder::colortype::RGBA16, pixels);
                } else {
                    write_page!(tiff::encoder::colortype::RGBA8, &r.pixels);
                }
            }
        }
        Ok((
            output.into_inner(),
            pages.iter().map(|p| SaveState(p.current.clone())).collect(),
        ))
    }
    /// Encode every composited frame; GIF may quantize colors and timing.
    pub fn save_gif_frames(
        frames: &[&Self],
        delays: &[image::Delay],
        repeat: AnimationLoop,
    ) -> Result<(Vec<u8>, Vec<SaveState>), Error> {
        if frames.is_empty() || frames.len() != delays.len() {
            return Err(Error::InvalidImage);
        }
        if frames
            .iter()
            .any(|d| d.width() != frames[0].width() || d.height() != frames[0].height())
        {
            return Err(Error::InvalidImage);
        }
        let mut bytes = Vec::new();
        {
            let mut encoder = image::codecs::gif::GifEncoder::new(&mut bytes);
            match repeat {
                AnimationLoop::Once => {}
                AnimationLoop::Finite(n) if n > 0 => {
                    encoder.set_repeat(image::codecs::gif::Repeat::Finite(n))?
                }
                AnimationLoop::Finite(_) => {}
                AnimationLoop::Infinite => {
                    encoder.set_repeat(image::codecs::gif::Repeat::Infinite)?
                }
            }
            for (doc, delay) in frames.iter().zip(delays) {
                let r = &doc.current;
                let image = image::RgbaImage::from_raw(r.width, r.height, r.pixels.to_vec())
                    .ok_or(Error::InvalidImage)?;
                encoder.encode_frame(image::Frame::from_parts(image, 0, 0, *delay))?;
            }
        }
        Ok((
            bytes,
            frames
                .iter()
                .map(|d| SaveState(d.current.clone()))
                .collect(),
        ))
    }
    pub fn width(&self) -> u32 {
        self.current.width
    }
    pub fn height(&self) -> u32 {
        self.current.height
    }
    pub fn pixels(&self) -> &[u8] {
        &self.current.pixels
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn is_dirty(&self) -> bool {
        self.current != self.saved
    }
    fn push(history: &mut Vec<Raster>, raster: Raster) {
        if history.len() == HISTORY_LIMIT {
            history.remove(0);
        }
        history.push(raster);
    }
    fn rotate(&mut self, orientation: u16) -> Result<(), Error> {
        let next = self.current.orient(orientation);
        if next != self.current {
            Self::push(&mut self.undo, self.current.clone());
            self.current = next;
            self.redo.clear();
        }
        Ok(())
    }
    pub fn flip_horizontal(&mut self) -> Result<(), Error> {
        self.rotate(2)
    }
    pub fn flip_vertical(&mut self) -> Result<(), Error> {
        self.rotate(4)
    }
    /// Resize using nearest-neighbour sampling, preserving straight RGBA samples.
    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        self.resize_quality(width, height, "nearest")
    }
    pub fn resize_quality(&mut self, width: u32, height: u32, quality: &str) -> Result<(), Error> {
        let filter = match quality {
            "nearest" => image::imageops::FilterType::Nearest,
            "linear" => image::imageops::FilterType::Triangle,
            "cubic" => image::imageops::FilterType::CatmullRom,
            "lanczos" => image::imageops::FilterType::Lanczos3,
            _ => return Err(Error::InvalidImage),
        };
        if width == 0 || height == 0 {
            return Err(Error::InvalidImage);
        }
        if width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || u64::from(width) * u64::from(height) > MAX_PIXELS
        {
            return Err(Error::TooLarge);
        }
        if (width, height) == (self.width(), self.height()) {
            return Ok(());
        }
        let mut next = if let Some(src) = &self.current.pixels16 {
            let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                self.width(),
                self.height(),
                src.to_vec(),
            )
            .ok_or(Error::InvalidImage)?;
            Raster::new16(
                width,
                height,
                image::imageops::resize(&image, width, height, filter).into_raw(),
            )?
        } else {
            let image =
                image::RgbaImage::from_raw(self.width(), self.height(), self.pixels().to_vec())
                    .ok_or(Error::InvalidImage)?;
            Raster::new(
                width,
                height,
                image::imageops::resize(&image, width, height, filter).into_raw(),
            )?
        };
        next.dpi = self.current.dpi;
        next.sample_model = self.current.sample_model;
        Self::push(&mut self.undo, self.current.clone());
        self.current = next;
        self.redo.clear();
        Ok(())
    }
    fn apply_raster(&mut self, mut next: Raster) {
        next.dpi = self.current.dpi;
        next.sample_model = self.current.sample_model;
        // Paint and masks are generated in the display buffer. Preserve exact untouched
        // 16-bit samples and promote changed display channels to 16-bit.
        if next.pixels16.is_none()
            && (next.width, next.height) == (self.current.width, self.current.height)
        {
            if let Some(source) = &self.current.pixels16 {
                let mut high = source.to_vec();
                for ((old, new), target) in self
                    .current
                    .pixels
                    .chunks_exact(4)
                    .zip(next.pixels.chunks_exact(4))
                    .zip(high.chunks_exact_mut(4))
                {
                    if old != new {
                        if new[3] == 0 {
                            target.fill(0);
                        } else {
                            for c in 0..4 {
                                if old[c] != new[c] {
                                    target[c] = u16::from(new[c]) * 257;
                                }
                            }
                        }
                    }
                }
                next.pixels16 = Some(high.into());
            }
        }
        if next != self.current {
            Self::push(&mut self.undo, self.current.clone());
            self.current = next;
            self.redo.clear();
        }
    }
    /// Crop to a rectangle in image pixels. Reject out-of-bounds rectangles.
    pub fn crop(&mut self, x: u32, y: u32, width: u32, height: u32) -> Result<(), Error> {
        if width == 0
            || height == 0
            || x.checked_add(width).is_none_or(|v| v > self.width())
            || y.checked_add(height).is_none_or(|v| v > self.height())
        {
            return Err(Error::InvalidImage);
        }
        let mut pixels = Vec::with_capacity((width as usize) * (height as usize) * 4);
        for row in y..y + height {
            let start = ((row * self.width() + x) as usize) * 4;
            pixels.extend_from_slice(&self.pixels()[start..start + width as usize * 4]);
        }
        let next = if let Some(src) = &self.current.pixels16 {
            let mut pixels16 = Vec::with_capacity(width as usize * height as usize * 4);
            for row in y..y + height {
                let start = ((row * self.width() + x) * 4) as usize;
                pixels16.extend_from_slice(&src[start..start + width as usize * 4]);
            }
            Raster::new16(width, height, pixels16)?
        } else {
            Raster::new(width, height, pixels)?
        };
        self.apply_raster(next);
        Ok(())
    }
    /// Apply brightness (-255..255), contrast (-100..100) and saturation (-100..100).
    pub fn adjust(&mut self, brightness: i32, contrast: i32, saturation: i32) -> Result<(), Error> {
        if !(-255..=255).contains(&brightness)
            || !(-100..=100).contains(&contrast)
            || !(-100..=100).contains(&saturation)
        {
            return Err(Error::InvalidImage);
        }
        if let Some(src) = &self.current.pixels16 {
            let mut pixels = src.to_vec();
            let c = (100.0 + contrast as f32) / 100.0;
            let sat = (100.0 + saturation as f32) / 100.0;
            for p in pixels.chunks_exact_mut(4) {
                let gray = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
                for v in &mut p[..3] {
                    let value = gray + (*v as f32 - gray) * sat;
                    *v = ((value - 32767.5) * c + 32767.5 + brightness as f32 * 257.0)
                        .round()
                        .clamp(0.0, 65535.0) as u16;
                }
            }
            self.apply_raster(Raster::new16(self.width(), self.height(), pixels)?);
            return Ok(());
        }
        let mut pixels = self.pixels().to_vec();
        let c = (100.0 + contrast as f32) / 100.0;
        let sat = (100.0 + saturation as f32) / 100.0;
        for p in pixels.chunks_exact_mut(4) {
            let gray = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
            for v in &mut p[..3] {
                let value = gray + (*v as f32 - gray) * sat;
                *v = ((value - 127.5) * c + 127.5 + brightness as f32)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
        }
        self.apply_raster(Raster::new(self.width(), self.height(), pixels)?);
        Ok(())
    }
    /// Draw a freehand gesture as one undoable edit.
    /// Apply exposure (stops ×100), gamma (×100), temperature and tonal recovery.
    pub fn develop(
        &mut self,
        exposure: i32,
        gamma: i32,
        temperature: i32,
        shadows: i32,
        highlights: i32,
        sharpness: i32,
    ) -> Result<(), Error> {
        if !(-400..=400).contains(&exposure)
            || !(10..=400).contains(&gamma)
            || !(-100..=100).contains(&temperature)
            || !(-100..=100).contains(&shadows)
            || !(-100..=100).contains(&highlights)
            || !(0..=100).contains(&sharpness)
        {
            return Err(Error::InvalidImage);
        }
        if let Some(src) = &self.current.pixels16 {
            let mut pixels = src.to_vec();
            let exposure = 2f32.powf(exposure as f32 / 100.0);
            let gamma = 100.0 / gamma as f32;
            for p in pixels.chunks_exact_mut(4) {
                let lum =
                    (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 65535.0;
                let shadow = shadows as f32 / 100.0 * (1.0 - lum).powi(2) * 0.35;
                let highlight = highlights as f32 / 100.0 * lum.powi(2) * 0.35;
                for (c, v) in p[..3].iter_mut().enumerate() {
                    let tint = match c {
                        0 => temperature as f32 / 500.0,
                        2 => -temperature as f32 / 500.0,
                        _ => 0.0,
                    };
                    *v = (((*v as f32 / 65535.0 * exposure).clamp(0.0, 1.0).powf(gamma)
                        + shadow
                        + highlight
                        + tint)
                        .clamp(0.0, 1.0)
                        * 65535.0)
                        .round() as u16;
                }
            }
            if sharpness > 0 {
                let image = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_raw(
                    self.width(),
                    self.height(),
                    pixels,
                )
                .ok_or(Error::InvalidImage)?;
                let blurred = image::imageops::blur(&image, 1.0);
                let mut out = image.into_raw();
                for (p, b) in out
                    .chunks_exact_mut(4)
                    .zip(blurred.as_raw().chunks_exact(4))
                {
                    for c in 0..3 {
                        p[c] = (p[c] as f32 + (p[c] as f32 - b[c] as f32) * sharpness as f32 / 50.0)
                            .round()
                            .clamp(0.0, 65535.0) as u16;
                    }
                }
                pixels = out;
            }
            self.apply_raster(Raster::new16(self.width(), self.height(), pixels)?);
            return Ok(());
        }
        let mut pixels = self.pixels().to_vec();
        let exposure = 2f32.powf(exposure as f32 / 100.0);
        let gamma = 100.0 / gamma as f32;
        for p in pixels.chunks_exact_mut(4) {
            let lum = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0;
            let shadow = shadows as f32 / 100.0 * (1.0 - lum).powi(2) * 0.35;
            let highlight = highlights as f32 / 100.0 * lum.powi(2) * 0.35;
            for (c, sample) in p[..3].iter_mut().enumerate() {
                let tint = match c {
                    0 => temperature as f32 / 500.0,
                    2 => -temperature as f32 / 500.0,
                    _ => 0.0,
                };
                let value = ((*sample as f32 / 255.0 * exposure)
                    .clamp(0.0, 1.0)
                    .powf(gamma)
                    + shadow
                    + highlight
                    + tint)
                    .clamp(0.0, 1.0);
                *sample = (value * 255.0).round() as u8;
            }
        }
        if sharpness > 0 {
            let image = image::RgbaImage::from_raw(self.width(), self.height(), pixels)
                .ok_or(Error::InvalidImage)?;
            let blurred = image::imageops::blur(&image, 1.0);
            let mut out = image.into_raw();
            for (p, b) in out
                .chunks_exact_mut(4)
                .zip(blurred.as_raw().chunks_exact(4))
            {
                for c in 0..3 {
                    p[c] = (p[c] as f32 + (p[c] as f32 - b[c] as f32) * sharpness as f32 / 50.0)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
            pixels = out;
        }
        self.apply_raster(Raster::new(self.width(), self.height(), pixels)?);
        Ok(())
    }
    pub fn erase_mask(&mut self, mask: &[bool]) -> Result<(), Error> {
        if mask.len() != (self.width() as usize) * (self.height() as usize) {
            return Err(Error::InvalidImage);
        }
        let mut pixels = self.pixels().to_vec();
        for (p, selected) in pixels.chunks_exact_mut(4).zip(mask) {
            if *selected {
                p.fill(0);
            }
        }
        self.apply_raster(Raster::new(self.width(), self.height(), pixels)?);
        Ok(())
    }
    pub fn masked_png(&self, mask: &[bool]) -> Result<Vec<u8>, Error> {
        use image::ImageEncoder;
        if mask.len() != (self.width() as usize) * (self.height() as usize) {
            return Err(Error::InvalidImage);
        }
        let mut pixels = self.pixels().to_vec();
        for (p, selected) in pixels.chunks_exact_mut(4).zip(mask) {
            if !*selected {
                p.fill(0);
            }
        }
        let mut out = Vec::new();
        image::codecs::png::PngEncoder::new(&mut out).write_image(
            &pixels,
            self.width(),
            self.height(),
            image::ExtendedColorType::Rgba8,
        )?;
        Ok(out)
    }
    /// Shape and rasterize text into image pixels using the bundled OFL Noto Sans font.
    pub fn draw_text(
        &mut self,
        x: u32,
        y: u32,
        size: u32,
        text: &str,
        color: [u8; 3],
    ) -> Result<(), Error> {
        use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
        if x >= self.width()
            || y >= self.height()
            || !(4..=512).contains(&size)
            || text.is_empty()
            || text.len() > 4096
        {
            return Err(Error::InvalidImage);
        }
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(
            include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf").to_vec(),
        );
        let mut fs = FontSystem::new_with_locale_and_db("en-US".into(), db);
        let mut buffer = Buffer::new(&mut fs, Metrics::new(size as f32, size as f32 * 1.3));
        buffer.set_size(
            &mut fs,
            Some((self.width() - x) as f32),
            Some((self.height() - y) as f32),
        );
        buffer.set_text(
            &mut fs,
            text,
            &Attrs::new().family(Family::Name("Noto Sans")),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(&mut fs, false);
        let mut pixels = self.pixels().to_vec();
        let mut cache = SwashCache::new();
        let mut missing = false;
        let width = self.width();
        let height = self.height();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                if glyph.glyph_id == 0 {
                    missing = true;
                    continue;
                }
                let p = glyph.physical((x as f32, 0.0), 1.0);
                cache.with_pixels(
                    &mut fs,
                    p.cache_key,
                    Color::rgb(color[0], color[1], color[2]),
                    |dx, dy, c| {
                        let px = p.x + dx;
                        let py = y as i32 + run.line_y as i32 + p.y + dy;
                        if px < 0 || py < 0 || px >= width as i32 || py >= height as i32 {
                            return;
                        }
                        let i = (py as usize * width as usize + px as usize) * 4;
                        let a = c.a() as u32;
                        let old_a = pixels[i + 3] as u32;
                        let out_a = a + (old_a * (255 - a) + 127) / 255;
                        if out_a > 0 {
                            for k in 0..3 {
                                pixels[i + k] = ((color[k] as u32 * a
                                    + pixels[i + k] as u32 * old_a * (255 - a) / 255
                                    + out_a / 2)
                                    / out_a)
                                    .min(255) as u8;
                            }
                            pixels[i + 3] = out_a as u8;
                        }
                    },
                );
            }
        }
        if missing {
            return Err(Error::UnsupportedFormat);
        }
        self.apply_raster(Raster::new(width, height, pixels)?);
        Ok(())
    }
    pub fn draw_stroke(
        &mut self,
        points: &[(u32, u32)],
        color: [u8; 4],
        width: u32,
    ) -> Result<(), Error> {
        if points.is_empty()
            || width == 0
            || width > 100
            || points
                .iter()
                .any(|&(x, y)| x >= self.width() || y >= self.height())
        {
            return Err(Error::InvalidImage);
        }
        let mut pixels = self.pixels().to_vec();
        let w = self.width() as i32;
        let h = self.height() as i32;
        let paint = |pixels: &mut [u8], x: i32, y: i32| {
            let left = (width as i32 - 1) / 2;
            let right = width as i32 / 2;
            for yy in (y - left).max(0)..=(y + right).min(h - 1) {
                for xx in (x - left).max(0)..=(x + right).min(w - 1) {
                    let i = ((yy * w + xx) * 4) as usize;
                    pixels[i..i + 4].copy_from_slice(&color);
                }
            }
        };
        paint(&mut pixels, points[0].0 as i32, points[0].1 as i32);
        for pair in points.windows(2) {
            let (mut x, mut y) = (pair[0].0 as i32, pair[0].1 as i32);
            let (tx, ty) = (pair[1].0 as i32, pair[1].1 as i32);
            let dx = (tx - x).abs();
            let sx = if x < tx { 1 } else { -1 };
            let dy = -(ty - y).abs();
            let sy = if y < ty { 1 } else { -1 };
            let mut err = dx + dy;
            loop {
                paint(&mut pixels, x, y);
                if x == tx && y == ty {
                    break;
                }
                let e = 2 * err;
                if e >= dy {
                    err += dy;
                    x += sx;
                }
                if e <= dx {
                    err += dx;
                    y += sy;
                }
            }
        }
        self.apply_raster(Raster::new(self.width(), self.height(), pixels)?);
        Ok(())
    }
    /// Paint a line on the raster, in image coordinates.
    pub fn draw_line(
        &mut self,
        x0: u32,
        y0: u32,
        x1: u32,
        y1: u32,
        color: [u8; 4],
    ) -> Result<(), Error> {
        if x0 >= self.width() || x1 >= self.width() || y0 >= self.height() || y1 >= self.height() {
            return Err(Error::InvalidImage);
        }
        let mut pixels = self.pixels().to_vec();
        let (mut x, mut y) = (x0 as i32, y0 as i32);
        let (tx, ty) = (x1 as i32, y1 as i32);
        let dx = (tx - x).abs();
        let sx = if x < tx { 1 } else { -1 };
        let dy = -(ty - y).abs();
        let sy = if y < ty { 1 } else { -1 };
        let mut err = dx + dy;
        loop {
            let i = ((y as u32 * self.width() + x as u32) * 4) as usize;
            pixels[i..i + 4].copy_from_slice(&color);
            if x == tx && y == ty {
                break;
            }
            let e = 2 * err;
            if e >= dy {
                err += dy;
                x += sx;
            }
            if e <= dx {
                err += dx;
                y += sy;
            }
        }
        self.apply_raster(Raster::new(self.width(), self.height(), pixels)?);
        Ok(())
    }
    pub fn rotate_cw(&mut self) -> Result<(), Error> {
        self.rotate(6)
    }
    pub fn rotate_ccw(&mut self) -> Result<(), Error> {
        self.rotate(8)
    }
    pub fn undo(&mut self) -> bool {
        if let Some(previous) = self.undo.pop() {
            Self::push(&mut self.redo, self.current.clone());
            self.current = previous;
            true
        } else {
            false
        }
    }
    pub fn redo(&mut self) -> bool {
        if let Some(next) = self.redo.pop() {
            Self::push(&mut self.undo, self.current.clone());
            self.current = next;
            true
        } else {
            false
        }
    }
    /// Writes a new destination without replacing an existing file. The temporary file
    /// is created in the destination directory; publication uses tempfile’s no-clobber
    /// operation, whose atomicity depends on the platform and filesystem.
    /// On success the current pixels become the clean baseline.
    pub fn save_snapshot(&self) -> Result<(Vec<u8>, SaveState), Error> {
        self.save_snapshot_format("png")
    }
    /// Encode a snapshot. JPEG composites transparency against white; metadata is not preserved.
    pub fn save_snapshot_format(&self, format: &str) -> Result<(Vec<u8>, SaveState), Error> {
        self.save_snapshot_format_quality(format, 90)
    }
    pub fn save_snapshot_format_quality(
        &self,
        format: &str,
        quality: u8,
    ) -> Result<(Vec<u8>, SaveState), Error> {
        if !(1..=100).contains(&quality) {
            return Err(Error::InvalidImage);
        }
        let state = self.current.clone();
        let mut bytes = Vec::new();
        if format == "jpeg" || format == "bmp" {
            use image::ImageEncoder;
            let mut rgb = Vec::with_capacity(state.pixels.len() / 4 * 3);
            for p in state.pixels.chunks_exact(4) {
                let a = u32::from(p[3]);
                for c in &p[..3] {
                    rgb.push(((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8);
                }
            }
            if format == "jpeg" {
                if self
                    .icc
                    .as_ref()
                    .is_some_and(|p| p.len() >= 20 && &p[16..20] == b"GRAY")
                {
                    return Err(Error::IncompatibleProfile);
                }
                let mut encoder =
                    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, quality);
                if let Some((x, y)) = state.dpi {
                    encoder.set_pixel_density(image::codecs::jpeg::PixelDensity {
                        density: (x as u16, y as u16),
                        unit: image::codecs::jpeg::PixelDensityUnit::Inches,
                    });
                }
                if let Some(exif) = &self.exif {
                    let bytes = exif
                        .encode(state.width, state.height, state.dpi)
                        .map_err(|_| Error::InvalidImage)?;
                    if bytes.len() > 65527 {
                        return Err(Error::TooLarge);
                    }
                    encoder
                        .set_exif_metadata(bytes)
                        .map_err(|_| Error::UnsupportedFormat)?;
                }
                if let Some(icc) = &self.icc {
                    encoder
                        .set_icc_profile(icc.clone())
                        .map_err(|_| Error::UnsupportedFormat)?;
                }
                encoder.write_image(
                    &rgb,
                    state.width,
                    state.height,
                    image::ExtendedColorType::Rgb8,
                )?;
            } else {
                image::codecs::bmp::BmpEncoder::new(&mut bytes).write_image(
                    &rgb,
                    state.width,
                    state.height,
                    image::ExtendedColorType::Rgb8,
                )?;
            }
            return Ok((bytes, SaveState(state)));
        }
        if format == "tiff" {
            let (bytes, mut states) = Self::save_tiff_pages(&[self])?;
            return Ok((bytes, states.remove(0)));
        }
        if matches!(format, "gif" | "webp") {
            let image =
                image::RgbaImage::from_raw(state.width, state.height, state.pixels.to_vec())
                    .ok_or(Error::InvalidImage)?;
            let target = match format {
                "tiff" => ImageFormat::Tiff,
                "gif" => ImageFormat::Gif,
                _ => ImageFormat::WebP,
            };
            let mut output = Cursor::new(Vec::new());
            image::DynamicImage::ImageRgba8(image).write_to(&mut output, target)?;
            return Ok((output.into_inner(), SaveState(state)));
        }
        if format != "png" {
            return Err(Error::UnsupportedFormat);
        }
        use image::ImageEncoder;
        let mut encoder = image::codecs::png::PngEncoder::new(&mut bytes);
        if let Some(exif) = &self.exif {
            encoder
                .set_exif_metadata(
                    exif.encode(state.width, state.height, state.dpi)
                        .map_err(|_| Error::InvalidImage)?,
                )
                .map_err(|_| Error::UnsupportedFormat)?;
        }
        if let Some(icc) = &self.icc {
            encoder
                .set_icc_profile(icc.clone())
                .map_err(|_| Error::UnsupportedFormat)?;
        }
        if let Some(high) = &state.pixels16 {
            let gray = matches!(
                state.sample_model,
                SampleModel::Gray | SampleModel::GrayAlpha
            ) && high.chunks_exact(4).all(|p| p[0] == p[1] && p[1] == p[2]);
            let alpha = matches!(
                state.sample_model,
                SampleModel::GrayAlpha | SampleModel::Rgba
            ) || high.chunks_exact(4).any(|p| p[3] != u16::MAX);
            let (kind, channels) = match (gray, alpha) {
                (true, false) => (image::ExtendedColorType::L16, 1),
                (true, true) => (image::ExtendedColorType::La16, 2),
                (false, false) => (image::ExtendedColorType::Rgb16, 3),
                (false, true) => (image::ExtendedColorType::Rgba16, 4),
            };
            if let Some(profile) = &self.icc {
                if profile.len() >= 20
                    && ((gray && &profile[16..20] == b"RGB ")
                        || (!gray && &profile[16..20] == b"GRAY"))
                {
                    return Err(Error::IncompatibleProfile);
                }
            }
            let mut encoded = Vec::with_capacity(high.len() / 4 * channels * 2);
            for p in high.chunks_exact(4) {
                let values: &[u16] = match (gray, alpha) {
                    (true, false) => &p[..1],
                    (false, false) => &p[..3],
                    (false, true) => p,
                    (true, true) => &[],
                };
                if gray && alpha {
                    encoded.extend_from_slice(&p[0].to_ne_bytes());
                    encoded.extend_from_slice(&p[3].to_ne_bytes());
                } else {
                    for v in values {
                        encoded.extend_from_slice(&v.to_ne_bytes());
                    }
                }
            }
            encoder.write_image(&encoded, state.width, state.height, kind)?;
        } else {
            encoder.write_image(
                &state.pixels,
                state.width,
                state.height,
                image::ExtendedColorType::Rgba8,
            )?;
        }
        if !self.png_color.is_empty() {
            if self.icc.is_some() && self.png_color.iter().any(|(tag, _)| tag == b"sRGB") {
                return Err(Error::IncompatibleProfile);
            }
            let mut chunks = Vec::new();
            for (tag, data) in &self.png_color {
                chunks.extend_from_slice(&(data.len() as u32).to_be_bytes());
                chunks.extend_from_slice(tag);
                chunks.extend_from_slice(data);
                let mut crc = crc32fast::Hasher::new();
                crc.update(tag);
                crc.update(data);
                chunks.extend_from_slice(&crc.finalize().to_be_bytes());
            }
            bytes.splice(33..33, chunks);
        }
        if let Some((x, y)) = state.dpi {
            let mut data = Vec::with_capacity(9);
            data.extend_from_slice(&((x as f64 / 0.0254).round() as u32).to_be_bytes());
            data.extend_from_slice(&((y as f64 / 0.0254).round() as u32).to_be_bytes());
            data.push(1);
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&9u32.to_be_bytes());
            chunk.extend_from_slice(b"pHYs");
            chunk.extend_from_slice(&data);
            let mut crc = crc32fast::Hasher::new();
            crc.update(b"pHYs");
            crc.update(&data);
            chunk.extend_from_slice(&crc.finalize().to_be_bytes());
            bytes.splice(33..33, chunk);
        }
        Ok((bytes, SaveState(state)))
    }
    pub fn mark_saved_snapshot(&mut self, state: SaveState) {
        self.saved = state.0;
    }
    pub fn save_as_png(&mut self, path: &Path) -> Result<(), Error> {
        let (bytes, state) = self.save_snapshot()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(suite42_core::win32_path(parent)?)?;
        temp.write_all(&bytes)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        match temp.persist_noclobber(suite42_core::win32_path(path)?) {
            Ok(_) => {
                self.mark_saved_snapshot(state);
                Ok(())
            }
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => {
                Err(Error::DestinationExists)
            }
            Err(e) => Err(Error::Io(e.error)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;
    use std::fs;
    fn fixture() -> Raster {
        Raster::new(3, 2, (1..=6).flat_map(|v| [v, 0, 0, 255 - v]).collect()).unwrap()
    }
    fn values(r: &Raster) -> Vec<u8> {
        r.pixels.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
    }
    fn document() -> ImageDocument {
        let r = fixture();
        ImageDocument {
            exif: None,
            png_color: Vec::new(),
            tiff_text: Vec::new(),
            icc: None,
            current: r.clone(),
            saved: r,
            undo: vec![],
            redo: vec![],
        }
    }
    #[test]
    fn png_color_chunks_handle_icc_and_reject_duplicates() {
        let mut raster = Raster::new16(1, 1, vec![1, 257, 4097, 65535]).unwrap();
        raster.sample_model = SampleModel::Rgb;
        let mut profile = vec![0; 128];
        profile[16..20].copy_from_slice(b"RGB ");
        let d = ImageDocument {
            exif: None,
            png_color: vec![(*b"gAMA", 45455u32.to_be_bytes().to_vec())],
            tiff_text: vec![],
            icc: Some(profile.clone()),
            current: raster.clone(),
            saved: raster,
            undo: vec![],
            redo: vec![],
        };
        let out = d.save_snapshot_format("png").unwrap().0;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icc.png");
        std::fs::write(&path, &out).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&path).unwrap());
        let loaded = ImageDocument::open(&path).unwrap();
        assert_eq!(loaded.icc, Some(profile));
        assert_eq!(loaded.png_color, d.png_color);
        let chunk = &out[33..49];
        assert_eq!(&chunk[4..8], b"gAMA");
        let mut duplicate = out.clone();
        duplicate.splice(33..33, chunk.iter().copied());
        let path = dir.path().join("duplicate.png");
        std::fs::write(&path, duplicate).unwrap();
        assert!(ImageDocument::has_conversion_risk(&path).unwrap());
    }
    #[test]
    fn png_color_chunks_and_exact_pixels_survive_geometry_save() {
        use image::{ImageDecoder, ImageEncoder};
        let mut png = Vec::new();
        let pixels = [1u16, 257, 4097, 65535, 60001, 32769];
        let raw: Vec<u8> = pixels.into_iter().flat_map(u16::to_ne_bytes).collect();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&raw, 2, 1, image::ExtendedColorType::Rgb16)
            .unwrap();
        let metadata: Vec<([u8; 4], Vec<u8>)> = vec![
            (*b"gAMA", 45455u32.to_be_bytes().to_vec()),
            (*b"cHRM", vec![0; 32]),
            (*b"sRGB", vec![0]),
        ];
        let mut insert = Vec::new();
        for (tag, data) in &metadata {
            insert.extend_from_slice(&(data.len() as u32).to_be_bytes());
            insert.extend_from_slice(tag);
            insert.extend_from_slice(data);
            let mut crc = crc32fast::Hasher::new();
            crc.update(tag);
            crc.update(data);
            insert.extend_from_slice(&crc.finalize().to_be_bytes());
        }
        png.splice(33..33, insert);
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("source.png");
        std::fs::write(&src, png).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&src).unwrap());
        let mut d = ImageDocument::open(&src).unwrap();
        d.rotate_cw().unwrap();
        let expected = d.current.pixels16.clone();
        let output = d.save_snapshot_format("png").unwrap().0;
        // Independently inspect every output chunk, including CRC, order and uniqueness.
        let mut pos = 8usize;
        let mut seen = Vec::new();
        let mut before_data = true;
        while pos < output.len() {
            let n = u32::from_be_bytes(output[pos..pos + 4].try_into().unwrap()) as usize;
            let tag = &output[pos + 4..pos + 8];
            let payload = &output[pos + 8..pos + 8 + n];
            let mut crc = crc32fast::Hasher::new();
            crc.update(tag);
            crc.update(payload);
            assert_eq!(
                crc.finalize(),
                u32::from_be_bytes(output[pos + 8 + n..pos + 12 + n].try_into().unwrap())
            );
            if tag == b"IDAT" {
                before_data = false;
            }
            if [b"gAMA", b"cHRM", b"sRGB"]
                .iter()
                .any(|t| t.as_slice() == tag)
            {
                assert!(before_data);
                seen.push((tag.to_vec(), payload.to_vec()));
            }
            pos += 12 + n;
        }
        assert_eq!(
            seen,
            metadata
                .iter()
                .map(|(a, b)| (a.to_vec(), b.clone()))
                .collect::<Vec<_>>()
        );
        let saved = dir.path().join("saved.png");
        std::fs::write(&saved, output).unwrap();
        let decoder = image::ImageReader::open(&saved)
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.color_type(), image::ColorType::Rgb16);
        let reopened = ImageDocument::open(&saved).unwrap();
        assert_eq!(reopened.current.pixels16, expected);
    }
    #[test]
    fn png_color_metadata_requires_converted_copy() {
        use image::ImageEncoder;
        let mut png = Vec::new();
        let samples: Vec<u8> = [1u16, 257, 4097]
            .into_iter()
            .flat_map(u16::to_ne_bytes)
            .collect();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&samples, 1, 1, image::ExtendedColorType::Rgb16)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (tag, data) in [
            (b"sBIT", vec![16, 16, 16]),
            (b"tEXt", b"Key\0Value".to_vec()),
        ] {
            let mut chunk = Vec::new();
            chunk.extend_from_slice(&(data.len() as u32).to_be_bytes());
            chunk.extend_from_slice(tag);
            chunk.extend_from_slice(&data);
            let mut crc = crc32fast::Hasher::new();
            crc.update(tag);
            crc.update(&data);
            chunk.extend_from_slice(&crc.finalize().to_be_bytes());
            let mut bytes = png.clone();
            bytes.splice(33..33, chunk);
            let path = dir
                .path()
                .join(format!("{}.png", String::from_utf8_lossy(tag)));
            std::fs::write(&path, bytes).unwrap();
            assert!(
                ImageDocument::has_conversion_risk(&path).unwrap(),
                "{}",
                String::from_utf8_lossy(tag)
            );
        }
    }
    #[test]
    fn sixteen_bit_png_preserves_icc_exif_and_density() {
        let field = exif::Field {
            tag: exif::Tag::Artist,
            ifd_num: exif::In::PRIMARY,
            value: exif::Value::Ascii(vec![b"Photographer".to_vec()]),
        };
        let mut writer = exif::experimental::Writer::new();
        writer.push_field(&field);
        let mut raw = Cursor::new(Vec::new());
        writer.write(&mut raw, true).unwrap();
        let mut raster =
            Raster::new16(2, 1, vec![1, 257, 4097, 65535, 32001, 60002, 7, 32769]).unwrap();
        raster.sample_model = SampleModel::Rgba;
        raster.dpi = Some((300, 150));
        let mut profile = vec![0u8; 128];
        profile[16..20].copy_from_slice(b"RGB ");
        let mut d = ImageDocument {
            exif: Some(exif_metadata::Metadata::parse(raw.into_inner()).unwrap()),
            png_color: Vec::new(),
            tiff_text: vec![],
            icc: Some(profile.clone()),
            current: raster.clone(),
            saved: raster,
            undo: vec![],
            redo: vec![],
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("source.png");
        std::fs::write(&path, d.save_snapshot_format("png").unwrap().0).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&path).unwrap());
        d = ImageDocument::open(&path).unwrap();
        assert_eq!(d.icc.as_deref(), Some(profile.as_slice()));
        assert_eq!(d.dpi(), Some((300, 150)));
        d.rotate_cw().unwrap();
        let path = dir.path().join("saved.png");
        std::fs::write(&path, d.save_snapshot_format("png").unwrap().0).unwrap();
        let reopened = ImageDocument::open(&path).unwrap();
        assert_eq!(reopened.icc.as_deref(), Some(profile.as_slice()));
        assert_eq!(reopened.dpi(), Some((150, 300)));
        assert_eq!(reopened.current.pixels16, d.current.pixels16);
        let metadata = exif::Reader::new()
            .read_from_container(&mut BufReader::new(File::open(path).unwrap()))
            .unwrap();
        assert!(metadata
            .get_field(exif::Tag::Artist, exif::In::PRIMARY)
            .is_some());
        assert_eq!(
            metadata
                .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(1)
        );
    }
    #[test]
    fn sixteen_bit_png_models_roundtrip_edits_and_history() {
        use image::ImageEncoder;
        let dir = tempfile::tempdir().unwrap();
        let fixtures: Vec<(image::ExtendedColorType, Vec<u16>)> = vec![
            (image::ExtendedColorType::L16, vec![1, 4097, 32001, 65534]),
            (
                image::ExtendedColorType::La16,
                vec![1, 65535, 4097, 32769, 32001, 0, 65534, 100],
            ),
            (
                image::ExtendedColorType::Rgb16,
                vec![
                    1, 257, 4097, 32001, 60002, 7, 65534, 40001, 12345, 33333, 22222, 11111,
                ],
            ),
            (
                image::ExtendedColorType::Rgba16,
                vec![
                    1, 257, 4097, 65535, 32001, 60002, 7, 32769, 65534, 40001, 12345, 0, 33333,
                    22222, 11111, 100,
                ],
            ),
        ];
        for (i, (kind, samples)) in fixtures.into_iter().enumerate() {
            let mut input = Vec::new();
            let raw: Vec<u8> = samples.iter().flat_map(|v| v.to_ne_bytes()).collect();
            image::codecs::png::PngEncoder::new(&mut input)
                .write_image(&raw, 2, 2, kind)
                .unwrap();
            let path = dir.path().join(format!("{i}.png"));
            std::fs::write(&path, &input).unwrap();
            assert!(!ImageDocument::has_conversion_risk(&path).unwrap());
            let mut d = ImageDocument::open(&path).unwrap();
            assert!(d.is_16bit());
            let original = d.current.pixels16.as_ref().unwrap().to_vec();
            d.rotate_cw().unwrap();
            d.crop(0, 0, 1, 2).unwrap();
            let expected = d.current.pixels16.as_ref().unwrap().to_vec();
            let (out, snapshot) = d.save_snapshot_format("png").unwrap();
            let saved = dir.path().join(format!("{i}-saved.png"));
            std::fs::write(&saved, out).unwrap();
            let decoder = image::ImageReader::open(&saved)
                .unwrap()
                .into_decoder()
                .unwrap();
            use image::ImageDecoder;
            assert_eq!(image::ExtendedColorType::from(decoder.color_type()), kind);
            let reopened = ImageDocument::open(&saved).unwrap();
            assert_eq!(
                reopened.current.pixels16.as_deref(),
                Some(expected.as_slice())
            );
            d.mark_saved_snapshot(snapshot);
            assert!(!d.is_dirty());
            assert!(d.undo());
            assert!(d.is_dirty());
            assert!(d.undo());
            assert_eq!(d.current.pixels16.as_deref(), Some(original.as_slice()));
            assert!(d.redo());
            assert!(d.redo());
            assert!(!d.is_dirty());
        }
    }
    #[test]
    fn sixteen_bit_markup_retains_untouched_samples_and_undo() {
        let source = vec![
            1000, 2000, 3000, 65535, 1001, 2001, 3001, 65535, 1002, 2002, 3002, 65535, 1003, 2003,
            3003, 65535,
        ];
        let raster = Raster::new16(2, 2, source.clone()).unwrap();
        let mut d = ImageDocument {
            exif: None,
            png_color: Vec::new(),
            tiff_text: Vec::new(),
            icc: None,
            current: raster.clone(),
            saved: raster,
            undo: vec![],
            redo: vec![],
        };
        d.draw_line(0, 0, 0, 0, [255, 0, 0, 255]).unwrap();
        assert!(d.is_16bit());
        let painted = d.current.pixels16.as_ref().unwrap();
        assert_eq!(&painted[4..], &source[4..]);
        d.erase_mask(&[false, true, false, false]).unwrap();
        let erased = d.current.pixels16.as_ref().unwrap();
        assert_eq!(&erased[4..8], &[0, 0, 0, 0]);
        assert_eq!(&erased[8..], &source[8..]);
        d.undo();
        d.undo();
        assert_eq!(d.current.pixels16.as_deref(), Some(source.as_slice()));
    }
    #[test]
    fn tiff_camera_and_capture_fields_are_preserved() {
        use tiff::tags::Tag;
        let mut bytes = Cursor::new(Vec::new());
        {
            let mut e = tiff::encoder::TiffEncoder::new(&mut bytes).unwrap();
            let mut im = e.new_image::<tiff::encoder::colortype::RGB8>(2, 1).unwrap();
            im.encoder().write_tag(Tag::Make, "Camera").unwrap();
            im.encoder()
                .write_tag(Tag::DateTime, "2026:11:06 12:00:00")
                .unwrap();
            im.write_data(&[12, 34, 56, 78, 90, 12]).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("in.tif");
        std::fs::write(&path, bytes.into_inner()).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&path).unwrap());
        let mut d = ImageDocument::open(&path).unwrap();
        d.rotate_cw().unwrap();
        let (out, _) = d.save_snapshot_format("tiff").unwrap();
        let mut decoded = tiff::decoder::Decoder::new(Cursor::new(out)).unwrap();
        assert!(
            matches!(decoded.get_tag(Tag::Make).unwrap(),tiff::decoder::ifd::Value::Ascii(ref x) if x=="Camera")
        );
        assert!(
            matches!(decoded.get_tag(Tag::DateTime).unwrap(),tiff::decoder::ifd::Value::Ascii(ref x) if x=="2026:11:06 12:00:00")
        );
    }
    #[test]
    fn exif_metadata_is_rebuilt_with_normalized_geometry_and_no_thumbnail() {
        use image::ImageEncoder;
        let fields = [
            exif::Field {
                tag: exif::Tag::Orientation,
                ifd_num: exif::In::PRIMARY,
                value: exif::Value::Short(vec![6]),
            },
            exif::Field {
                tag: exif::Tag::Artist,
                ifd_num: exif::In::PRIMARY,
                value: exif::Value::Ascii(vec![b"Author".to_vec()]),
            },
            exif::Field {
                tag: exif::Tag::DateTimeOriginal,
                ifd_num: exif::In::PRIMARY,
                value: exif::Value::Ascii(vec![b"2026:11:06 12:00:00".to_vec()]),
            },
            exif::Field {
                tag: exif::Tag::GPSLatitudeRef,
                ifd_num: exif::In::PRIMARY,
                value: exif::Value::Ascii(vec![b"N".to_vec()]),
            },
            exif::Field {
                tag: exif::Tag::GPSLatitude,
                ifd_num: exif::In::PRIMARY,
                value: exif::Value::Rational(vec![
                    exif::Rational { num: 41, denom: 1 },
                    exif::Rational { num: 1, denom: 1 },
                    exif::Rational { num: 2, denom: 1 },
                ]),
            },
        ];
        let mut writer = exif::experimental::Writer::new();
        for f in &fields {
            writer.push_field(f);
        }
        writer.set_jpeg(b"stale-thumbnail-secret", exif::In::THUMBNAIL);
        let mut raw = Cursor::new(Vec::new());
        writer.write(&mut raw, true).unwrap();
        let mut jpeg = Vec::new();
        let mut encoder = image::codecs::jpeg::JpegEncoder::new(&mut jpeg);
        encoder.set_exif_metadata(raw.into_inner()).unwrap();
        encoder
            .write_image(&[128; 18], 2, 3, image::ExtendedColorType::Rgb8)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("source.jpg");
        std::fs::write(&input, jpeg).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&input).unwrap());
        let doc = ImageDocument::open(&input).unwrap();
        assert_eq!((doc.width(), doc.height()), (3, 2));
        let (output, _) = doc.save_snapshot_format("jpeg").unwrap();
        assert!(!output
            .windows(b"stale-thumbnail-secret".len())
            .any(|v| v == b"stale-thumbnail-secret"));
        let saved = dir.path().join("saved.jpg");
        std::fs::write(&saved, output).unwrap();
        let exif = exif::Reader::new()
            .read_from_container(&mut BufReader::new(File::open(&saved).unwrap()))
            .unwrap();
        assert_eq!(
            exif.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(1)
        );
        assert_eq!(
            exif.get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(3)
        );
        assert_eq!(
            exif.get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY)
                .unwrap()
                .value
                .get_uint(0),
            Some(2)
        );
        assert!(exif
            .get_field(exif::Tag::Artist, exif::In::PRIMARY)
            .is_some());
        assert!(exif
            .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
            .is_some());
        assert!(exif
            .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
            .is_some());
        assert!(exif.fields().all(|f| f.ifd_num == exif::In::PRIMARY));
    }
    #[test]
    fn sixteen_bit_tiff_survives_rotation_crop_and_history() {
        let pixels = vec![
            1u16, 257, 4097, 65535, 1234, 2345, 3456, 65535, 50000, 40000, 30000, 65535, 65000,
            500, 100, 65535,
        ];
        let mut raw = std::io::Cursor::new(Vec::new());
        {
            let mut encoder = tiff::encoder::TiffEncoder::new(&mut raw).unwrap();
            encoder
                .write_image::<tiff::encoder::colortype::RGBA16>(2, 2, &pixels)
                .unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("16.tif");
        std::fs::write(&path, raw.into_inner()).unwrap();
        assert!(!ImageDocument::has_conversion_risk(&path).unwrap());
        let (mut frames, _) = ImageDocument::open_frames(&path).unwrap();
        let d = &mut frames[0];
        assert!(d.is_16bit());
        assert_eq!(d.current.pixels16.as_deref(), Some(pixels.as_slice()));
        d.rotate_cw().unwrap();
        d.crop(0, 0, 1, 2).unwrap();
        let expected = d.current.pixels16.as_ref().unwrap().to_vec();
        let (out, _) = d.save_snapshot_format("tiff").unwrap();
        let saved = dir.path().join("out.tif");
        std::fs::write(&saved, out).unwrap();
        let reopened = ImageDocument::open(&saved).unwrap();
        assert_eq!(
            reopened.current.pixels16.as_deref(),
            Some(expected.as_slice())
        );
        d.undo();
        d.undo();
        assert_eq!(d.current.pixels16.as_deref(), Some(pixels.as_slice()));
        d.redo();
        d.redo();
        assert_eq!(d.current.pixels16.as_deref(), Some(expected.as_slice()));
    }
    #[test]
    fn svg_render_and_resource_policy() {
        let svg=br##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="8" height="4" fill="#ff0000"/></svg>"##;
        let doc = ImageDocument::open_svg(svg).unwrap();
        assert_eq!((doc.width(), doc.height()), (8, 4));
        assert_eq!(&doc.pixels()[..4], &[255, 0, 0, 255]);
        let external=br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><image href="/tmp/private.png" width="2" height="2"/></svg>"#;
        assert!(matches!(
            ImageDocument::open_svg(external),
            Err(Error::SvgResourcesDisallowed)
        ));
        use base64::Engine;
        use image::ImageEncoder;
        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[255, 0, 0, 255], 1, 1, image::ExtendedColorType::Rgba8)
            .unwrap();
        let valid = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><image href="data:image/png;base64,{}" width="1" height="1"/></svg>"#,
            base64::engine::general_purpose::STANDARD.encode(png)
        );
        let image = ImageDocument::open_svg(valid.as_bytes()).unwrap();
        assert_eq!(image.pixels(), &[255, 0, 0, 255]);
        let embedded=br#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><image href="data:image/png;base64,aGVsbG8=" width="2" height="2"/></svg>"#;
        assert!(matches!(
            ImageDocument::open_svg(embedded),
            Err(Error::SvgResourcesDisallowed)
        ));
    }
    #[test]
    fn density_roundtrips_and_rotates_with_history() {
        let mut d = ImageDocument::from_rgba(3, 2, vec![255; 24], Some((300, 150))).unwrap();
        let dir = tempfile::tempdir().unwrap();
        for (fmt, ext) in [("png", "png"), ("jpeg", "jpg"), ("tiff", "tif")] {
            let (bytes, _) = d.save_snapshot_format(fmt).unwrap();
            let path = dir.path().join(ext);
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(
                ImageDocument::source_dpi(&path).unwrap(),
                Some((300, 150)),
                "{fmt}"
            );
        }
        d.rotate_cw().unwrap();
        assert_eq!(d.dpi(), Some((150, 300)));
        d.undo();
        assert_eq!(d.dpi(), Some((300, 150)));
        d.redo();
        assert_eq!(d.dpi(), Some((150, 300)));
    }
    #[test]
    fn icc_and_mask_export_roundtrip() {
        let mut d = document();
        let original = d.pixels().to_vec();
        d.icc = Some(vec![1, 2, 3, 4]);
        let (png, _) = d.save_snapshot_format("png").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("icc.png");
        std::fs::write(&path, png).unwrap();
        assert_eq!(
            ImageDocument::open(&path).unwrap().icc,
            Some(vec![1, 2, 3, 4])
        );
        let mask = vec![true, false, true, false, true, false];
        let png = d.masked_png(&mask).unwrap();
        let path = dir.path().join("mask.png");
        std::fs::write(&path, png).unwrap();
        let exported = ImageDocument::open(&path).unwrap();
        assert_eq!(exported.pixels()[7], 0);
        assert_eq!(exported.pixels()[3], original[3]);
        d.erase_mask(&mask).unwrap();
        assert_eq!(d.pixels()[3], 0);
        d.undo();
        assert_eq!(d.pixels(), original);
    }
    #[test]
    fn cyrillic_text_is_rendered_and_undoable() {
        let raster = Raster::new(180, 60, vec![255; 180 * 60 * 4]).unwrap();
        let mut d = ImageDocument {
            exif: None,
            png_color: Vec::new(),
            tiff_text: Vec::new(),
            icc: None,
            current: raster.clone(),
            saved: raster,
            undo: vec![],
            redo: vec![],
        };
        d.draw_text(2, 2, 24, "Привет", [0, 0, 0]).unwrap();
        assert!(d.is_dirty());
        assert!(d.pixels().chunks_exact(4).any(|p| p[0] < 128));
        d.undo();
        assert!(!d.is_dirty());
        d.redo();
        assert!(d.is_dirty());
    }
    #[test]
    fn multipage_tiff_roundtrip_and_per_page_history() {
        let a = document();
        let mut b = document();
        b.rotate(6).unwrap();
        let (bytes, _) = ImageDocument::save_tiff_pages(&[&a, &b]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pages.tif");
        std::fs::write(&path, bytes).unwrap();
        let (mut pages, _) = ImageDocument::open_frames(&path).unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].pixels(), a.pixels());
        assert_eq!(
            (pages[1].width(), pages[1].height()),
            (b.width(), b.height())
        );
        let before = pages[1].pixels().to_vec();
        pages[1].develop(50, 100, 0, 0, 0, 0).unwrap();
        assert!(pages[1].is_dirty());
        assert!(!pages[0].is_dirty());
        pages[1].undo();
        assert_eq!(pages[1].pixels(), before);
    }
    #[test]
    fn animated_gif_roundtrip_retains_frames_and_delays() {
        let mut a = document();
        let mut pixels = a.current.pixels.to_vec();
        for pixel in pixels.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        a.current = Raster::new(a.width(), a.height(), pixels).unwrap();
        let mut b = document();
        b.current = a.current.clone();
        b.rotate(3).unwrap();
        let delays = vec![
            image::Delay::from_numer_denom_ms(120, 1),
            image::Delay::from_numer_denom_ms(230, 1),
        ];
        let (bytes, _) =
            ImageDocument::save_gif_frames(&[&a, &b], &delays, AnimationLoop::Finite(3)).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("animation.gif");
        std::fs::write(&path, bytes).unwrap();
        let (frames, decoded_delays) = ImageDocument::open_frames(&path).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].pixels(), a.pixels());
        assert_eq!(frames[1].pixels(), b.pixels());
        assert_eq!(decoded_delays, delays);
        assert_eq!(
            ImageDocument::animation_loop(&path).unwrap(),
            AnimationLoop::Finite(3)
        );
    }
    #[test]
    fn stroke_is_one_history_entry() {
        let mut d = document();
        let before = d.pixels().to_vec();
        d.draw_stroke(&[(0, 0), (1, 0), (2, 1)], [255, 0, 0, 255], 1)
            .unwrap();
        let after = d.pixels().to_vec();
        assert_ne!(before, after);
        assert!(d.undo());
        assert_eq!(d.pixels(), before);
        assert!(d.redo());
        assert_eq!(d.pixels(), after);
    }
    #[test]
    fn crop_adjust_markup_and_export_history() {
        let mut d = document();
        d.crop(1, 0, 2, 2).unwrap();
        assert_eq!(values(&d.current), vec![2, 3, 5, 6]);
        d.adjust(10, 0, 0).unwrap();
        d.draw_line(0, 0, 1, 1, [255, 0, 0, 255]).unwrap();
        let marked = d.pixels().to_vec();
        assert!(d.undo());
        assert!(d.undo());
        assert!(d.undo());
        assert_eq!(values(&d.current), vec![1, 2, 3, 4, 5, 6]);
        assert!(d.redo());
        assert!(d.redo());
        assert!(d.redo());
        assert_eq!(d.pixels(), marked);
        for format in ["png", "jpeg", "bmp", "tiff", "gif", "webp"] {
            let (bytes, _) = d.save_snapshot_format(format).unwrap();
            let decoded = image::load_from_memory(&bytes).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (2, 2));
        }
    }
    #[test]
    fn published_snapshot_does_not_clean_a_later_edit() {
        let mut d = document();
        d.rotate_cw().unwrap();
        let (_, snapshot) = d.save_snapshot().unwrap();
        d.flip_horizontal().unwrap();
        d.mark_saved_snapshot(snapshot);
        assert!(d.is_dirty());
        assert!(d.undo());
        assert!(!d.is_dirty());
    }
    #[test]
    fn all_orientation_mappings_and_alpha() {
        let original = fixture();
        let cases = [
            vec![1, 2, 3, 4, 5, 6],
            vec![3, 2, 1, 6, 5, 4],
            vec![6, 5, 4, 3, 2, 1],
            vec![4, 5, 6, 1, 2, 3],
            vec![1, 4, 2, 5, 3, 6],
            vec![4, 1, 5, 2, 6, 3],
            vec![6, 3, 5, 2, 4, 1],
            vec![3, 6, 2, 5, 1, 4],
        ];
        for (i, expected) in cases.iter().enumerate() {
            let r = original.orient((i + 1) as u16);
            assert_eq!(&values(&r), expected);
            assert_eq!((r.width, r.height), if i >= 4 { (2, 3) } else { (3, 2) });
            for p in r.pixels.as_chunks::<4>().0 {
                assert_eq!(p[3], 255 - p[0]);
            }
        }
    }
    #[test]
    fn limits_and_invalid_pixels() {
        assert!(matches!(
            Raster::new(0, 1, vec![]),
            Err(Error::InvalidImage)
        ));
        assert!(matches!(
            Raster::new(1, 1, vec![0]),
            Err(Error::InvalidImage)
        ));
        assert!(matches!(
            Raster::new(16_385, 1, vec![]),
            Err(Error::TooLarge)
        ));
        assert!(matches!(
            Raster::new(4001, 4000, vec![]),
            Err(Error::TooLarge)
        ));
    }
    #[test]
    fn history_clean_state_and_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("output.png");
        let mut d = document();
        let original = d.current.clone();
        d.rotate_cw().unwrap();
        let rotated = d.current.clone();
        assert!(d.is_dirty());
        assert!(d.undo());
        assert!(!d.is_dirty());
        assert_eq!(d.current.pixels, original.pixels);
        assert!(d.redo());
        assert_eq!(d.current.pixels, rotated.pixels);
        d.save_as_png(&path).unwrap();
        assert!(!d.is_dirty());
        let reopened = ImageDocument::open(&path).unwrap();
        assert!(reopened.current == rotated);
        assert!(d.undo());
        assert!(d.is_dirty());
        assert!(d.redo());
        assert!(!d.is_dirty());
        d.undo();
        d.rotate_ccw().unwrap();
        assert!(!d.can_redo());
    }
    #[test]
    fn save_failure_preserves_existing_destination_and_dirty_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("exists.png");
        fs::write(&path, b"original").unwrap();
        let mut d = document();
        d.rotate_cw().unwrap();
        assert!(matches!(
            d.save_as_png(&path),
            Err(Error::DestinationExists)
        ));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert!(d.is_dirty());
        assert!(d
            .save_as_png(&dir.path().join("missing").join("out.png"))
            .is_err());
        assert!(d.is_dirty());
    }
    #[test]
    fn flips_resize_history_and_saved_pixels() {
        let mut d = document();
        d.flip_horizontal().unwrap();
        assert_eq!(values(&d.current), vec![3, 2, 1, 6, 5, 4]);
        d.flip_vertical().unwrap();
        assert_eq!(values(&d.current), vec![6, 5, 4, 3, 2, 1]);
        assert!(d.undo());
        assert_eq!(values(&d.current), vec![3, 2, 1, 6, 5, 4]);
        assert!(d.redo());
        d.resize(2, 1).unwrap();
        assert_eq!((d.width(), d.height()), (2, 1));
        let pixels = d.pixels().to_vec();
        assert!(d.resize(0, 1).is_err());
        assert!(d.resize(16385, 1).is_err());
        assert_eq!(d.pixels(), pixels);
        let path = tempfile::tempdir().unwrap();
        let output = path.path().join("out.png");
        d.save_as_png(&output).unwrap();
        let opened = ImageDocument::open(&output).unwrap();
        assert_eq!(opened.pixels(), pixels);
        assert!(!d.is_dirty());
        assert!(d.undo());
        assert!(d.is_dirty());
        assert!(d.redo());
        assert!(!d.is_dirty());
        d.undo();
        d.flip_vertical().unwrap();
        assert!(!d.can_redo());
    }
    #[test]
    fn jpeg_exif_orientation_is_applied_at_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
            .write_image(
                &[
                    255, 0, 0, 0, 255, 0, 0, 0, 255, 255, 255, 255, 0, 0, 0, 127, 127, 127,
                ],
                3,
                2,
                image::ExtendedColorType::Rgb8,
            )
            .unwrap();
        for orientation in 1u8..=8 {
            // Little-endian TIFF with one SHORT orientation entry.
            let exif = [
                b'E',
                b'x',
                b'i',
                b'f',
                0,
                0,
                b'I',
                b'I',
                42,
                0,
                8,
                0,
                0,
                0,
                1,
                0,
                0x12,
                0x01,
                3,
                0,
                1,
                0,
                0,
                0,
                orientation,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
            ];
            let mut bytes = vec![0xff, 0xd8, 0xff, 0xe1, 0, 34];
            bytes.extend(exif);
            bytes.extend(&jpeg[2..]);
            let path = dir.path().join(format!("{orientation}.jpg"));
            fs::write(&path, bytes).unwrap();
            let doc = ImageDocument::open(&path).unwrap();
            let base = ImageDocument::open(&{
                let p = dir.path().join("base.jpg");
                fs::write(&p, &jpeg).unwrap();
                p
            })
            .unwrap();
            let maps = [
                [0, 1, 2, 3, 4, 5],
                [2, 1, 0, 5, 4, 3],
                [5, 4, 3, 2, 1, 0],
                [3, 4, 5, 0, 1, 2],
                [0, 3, 1, 4, 2, 5],
                [3, 0, 4, 1, 5, 2],
                [5, 2, 4, 1, 3, 0],
                [2, 5, 1, 4, 0, 3],
            ];
            let expected: Vec<u8> = maps[orientation as usize - 1]
                .iter()
                .flat_map(|&i| base.pixels()[i * 4..i * 4 + 4].iter().copied())
                .collect();
            assert_eq!(doc.pixels(), expected);
            assert_eq!(
                (doc.width(), doc.height()),
                if orientation >= 5 { (2, 3) } else { (3, 2) }
            );
        }
    }
}
