pub(super) const COMMAND: u16 = 77;
pub(super) const FORMATS: [&str; 2] = ["PNG", "JPEG"];
pub(super) const WIDTHS: [&str; 3] = ["1280", "1920", "3840"];

pub(super) struct Options {
    jpeg: bool,
    #[cfg(windows)]
    width: u32,
    #[cfg(windows)]
    include_hidden: bool,
}

impl Options {
    pub(super) fn parse(values: &[String]) -> Result<Self, String> {
        let [format, width, include] = values else {
            return Err("Invalid slide image options".into());
        };
        if !FORMATS.contains(&format.as_str())
            || !WIDTHS.contains(&width.as_str())
            || !matches!(include.as_str(), "No" | "Yes")
        {
            return Err("Invalid slide image options".into());
        }
        Ok(Self {
            jpeg: format == "JPEG",
            #[cfg(windows)]
            width: width.parse().map_err(|_| "Invalid slide image width")?,
            #[cfg(windows)]
            include_hidden: include == "Yes",
        })
    }

    pub(super) fn extension(&self) -> &'static str {
        if self.jpeg {
            "jpg"
        } else {
            "png"
        }
    }

    #[cfg(windows)]
    pub(super) fn export(self, app: &crate::Ppt, path: &std::path::Path) -> Result<(), String> {
        use image::{
            codecs::{jpeg::JpegEncoder, png::PngEncoder},
            ExtendedColorType, ImageEncoder,
        };

        let size = app
            .pptx_source
            .as_ref()
            .map(|source| source.size_points())
            .or_else(|| {
                app.odp_source
                    .as_ref()
                    .and_then(|source| source.size_points())
            })
            .unwrap_or((720.0, 405.0));
        let height = (f64::from(self.width) * size.1 / size.0).round();
        if !(1.0..=8192.0).contains(&height) || f64::from(self.width) * height > 16_000_000.0 {
            return Err("The slide image dimensions exceed the export limit".into());
        }
        let height = height as u32;
        let slides: Vec<_> = app
            .deck
            .slides()
            .iter()
            .enumerate()
            .filter_map(|(index, slide)| (self.include_hidden || !slide.hidden).then_some(index))
            .collect();
        if slides.is_empty() {
            return Err("There are no slides to export".into());
        }
        let stem = path
            .file_stem()
            .ok_or("Missing slide image filename prefix")?;
        let paths = slides
            .iter()
            .map(|&index| {
                let mut filename = stem.to_os_string();
                filename.push(format!("-{:03}.{}", index + 1, self.extension()));
                path.with_file_name(filename)
            })
            .collect();
        suite42_core::publish_new_files(paths, "Slide image", |index| {
            let scene = app
                .scene_for(slides[index], false)
                .ok_or("Could not render slide")?;
            let pixels = win42::render_scene_rgb(scene, (720.0, 405.0), (self.width, height))?;
            let mut encoded = Vec::new();
            if self.jpeg {
                JpegEncoder::new_with_quality(&mut encoded, 90).encode(
                    &pixels,
                    self.width,
                    height,
                    ExtendedColorType::Rgb8,
                )
            } else {
                PngEncoder::new(&mut encoded).write_image(
                    &pixels,
                    self.width,
                    height,
                    ExtendedColorType::Rgb8,
                )
            }
            .map_err(|error| error.to_string())?;
            Ok(encoded)
        })
        .map(|_| ())
    }
}
