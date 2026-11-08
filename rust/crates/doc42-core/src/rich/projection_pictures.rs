//! Rich Edit may re-encode a PNG after text editing. Restore the original picture
//! resource only after bounded sample comparison; never accept a changed image.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Copy, PartialEq, Eq)]
struct Samples {
    width: u32,
    height: u32,
    hash: [u8; 32],
}
pub(super) struct Matcher<'a> {
    pictures: Vec<&'a Image>,
    used: BTreeSet<u64>,
    samples: BTreeMap<u64, Option<Samples>>,
    budget: u64,
}
fn samples(image: &Image, budget: &mut u64) -> Result<Option<Samples>, Error> {
    if !matches!(image.mime.as_str(), "image/png" | "image/jpeg") {
        return Ok(None);
    }
    let reader = image::ImageReader::new(std::io::Cursor::new(&image.data))
        .with_guessed_format()
        .map_err(|_| Error::Invalid("picture codec".into()))?;
    let Ok((width, height)) = reader.into_dimensions() else {
        return Ok(None);
    };
    let pixels = u64::from(width) * u64::from(height);
    if width == 0
        || height == 0
        || width > 16384
        || height > 16384
        || pixels > 16_000_000
        || pixels > *budget
    {
        return Err(Error::Limit);
    }
    *budget -= pixels;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(&image.data))
        .with_guessed_format()
        .map_err(|_| Error::Invalid("picture codec".into()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16384);
    limits.max_image_height = Some(16384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    let Ok(decoded) = reader.decode() else {
        return Ok(None);
    };
    let pixels = decoded.to_rgba8();
    Ok(Some(Samples {
        width,
        height,
        hash: Sha256::digest(pixels.as_raw()).into(),
    }))
}
impl<'a> Matcher<'a> {
    pub(super) fn new(blocks: &'a [Block]) -> Self {
        Self {
            pictures: blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Image(image) => Some(image),
                    _ => None,
                })
                .collect(),
            used: BTreeSet::new(),
            samples: BTreeMap::new(),
            budget: 32_000_000,
        }
    }
    pub(super) fn used(&self, id: u64) -> bool {
        self.used.contains(&id)
    }
    pub(super) fn restore(&mut self, image: &mut Image) -> Result<bool, Error> {
        let mut found = self.pictures.iter().copied().find(|old| {
            !self.used.contains(&old.id) && old.mime == image.mime && old.data == image.data
        });
        if found.is_none() {
            if self.pictures.iter().all(|old| self.used.contains(&old.id)) {
                return Ok(false);
            }
            if let Some(current) = samples(image, &mut self.budget)? {
                for old in self
                    .pictures
                    .iter()
                    .copied()
                    .filter(|old| !self.used.contains(&old.id))
                {
                    let before = if let Some(value) = self.samples.get(&old.id) {
                        *value
                    } else {
                        let value = samples(old, &mut self.budget)?;
                        self.samples.insert(old.id, value);
                        value
                    };
                    if before == Some(current) {
                        found = Some(old);
                        break;
                    }
                }
            }
        }
        if let Some(old) = found {
            image.id = old.id;
            image.alt = old.alt.clone();
            // Keep original compressed samples, color/EXIF metadata and format, not Rich Edit's rewrite.
            if image.data != old.data || image.mime != old.mime {
                image.data = old.data.clone();
                image.mime = old.mime.clone();
            }
            self.used.insert(old.id);
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
