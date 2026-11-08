//! Lossless current-image recovery, independent of conversion/export codecs.
use super::*;
const MAGIC: &[u8; 8] = b"I42SNAP1";
const LIMIT: usize = 256 * 1024 * 1024;
const TIFF_TAGS: [tiff::tags::Tag; 8] = [
    tiff::tags::Tag::Artist,
    tiff::tags::Tag::Copyright,
    tiff::tags::Tag::DateTime,
    tiff::tags::Tag::ImageDescription,
    tiff::tags::Tag::Make,
    tiff::tags::Tag::Model,
    tiff::tags::Tag::Software,
    tiff::tags::Tag::HostComputer,
];

fn number(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn blob(out: &mut Vec<u8>, bytes: &[u8]) -> Result<(), Error> {
    if bytes.len() > LIMIT || out.len().saturating_add(bytes.len()).saturating_add(4) > LIMIT {
        return Err(Error::TooLarge);
    }
    number(out, bytes.len() as u32);
    out.extend_from_slice(bytes);
    Ok(())
}
struct Reader<'a>(&'a [u8]);
impl<'a> Reader<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8], Error> {
        if length > self.0.len() {
            return Err(Error::InvalidImage);
        }
        let (value, rest) = self.0.split_at(length);
        self.0 = rest;
        Ok(value)
    }
    fn number(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn blob(&mut self) -> Result<&'a [u8], Error> {
        let length = self.number()? as usize;
        self.take(length)
    }
}

impl ImageDocument {
    /// Capture exact current samples and retained metadata without marking the image saved.
    pub fn recovery_bytes(&self) -> Result<Vec<u8>, Error> {
        let mut out = MAGIC.to_vec();
        let raster = &self.current;
        for value in [
            raster.width,
            raster.height,
            raster.dpi.map_or(0, |v| v.0),
            raster.dpi.map_or(0, |v| v.1),
            match raster.sample_model {
                SampleModel::Gray => 0,
                SampleModel::GrayAlpha => 1,
                SampleModel::Rgb => 2,
                SampleModel::Rgba => 3,
            },
            u32::from(raster.pixels16.is_some()),
        ] {
            number(&mut out, value);
        }
        if let Some(pixels) = &raster.pixels16 {
            let length = pixels.len().checked_mul(2).ok_or(Error::TooLarge)?;
            if length > LIMIT {
                return Err(Error::TooLarge);
            }
            number(&mut out, length as u32);
            for sample in pixels.iter() {
                out.extend_from_slice(&sample.to_le_bytes());
            }
        } else {
            blob(&mut out, &raster.pixels)?;
        }
        blob(&mut out, self.icc.as_deref().unwrap_or_default())?;
        let exif = self
            .exif
            .as_ref()
            .map(|metadata| metadata.encode(raster.width, raster.height, raster.dpi))
            .transpose()
            .map_err(|_| Error::InvalidImage)?;
        blob(&mut out, exif.as_deref().unwrap_or_default())?;
        number(
            &mut out,
            u32::from(self.exif.as_ref().is_some_and(|m| m.lossy)),
        );
        number(&mut out, self.png_color.len() as u32);
        for (tag, data) in &self.png_color {
            out.extend_from_slice(tag);
            blob(&mut out, data)?;
        }
        number(&mut out, self.tiff_text.len() as u32);
        for (tag, text) in &self.tiff_text {
            number(
                &mut out,
                TIFF_TAGS
                    .iter()
                    .position(|value| value == tag)
                    .ok_or(Error::UnsupportedFormat)? as u32,
            );
            blob(&mut out, text.as_bytes())?;
        }
        if out.len() > LIMIT {
            return Err(Error::TooLarge);
        }
        Ok(out)
    }

    pub fn from_recovery_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() > LIMIT {
            return Err(Error::TooLarge);
        }
        let mut input = Reader(bytes);
        if input.take(8)? != MAGIC {
            return Err(Error::UnsupportedFormat);
        }
        let width = input.number()?;
        let height = input.number()?;
        let x = input.number()?;
        let y = input.number()?;
        let dpi = match (x, y) {
            (0, 0) => None,
            (1..=9600, 1..=9600) => Some((x, y)),
            _ => return Err(Error::InvalidImage),
        };
        let model = match input.number()? {
            0 => SampleModel::Gray,
            1 => SampleModel::GrayAlpha,
            2 => SampleModel::Rgb,
            3 => SampleModel::Rgba,
            _ => return Err(Error::InvalidImage),
        };
        let high = match input.number()? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidImage),
        };
        let count = u64::from(width) * u64::from(height);
        if width == 0
            || height == 0
            || width > MAX_DIMENSION
            || height > MAX_DIMENSION
            || count > MAX_PIXELS
        {
            return Err(Error::TooLarge);
        }
        let pixels = input.blob()?;
        if pixels.len() as u64 != count * if high { 8 } else { 4 } {
            return Err(Error::InvalidImage);
        }
        let mut raster = if high {
            Raster::new16(
                width,
                height,
                pixels
                    .chunks_exact(2)
                    .map(|p| u16::from_le_bytes([p[0], p[1]]))
                    .collect(),
            )?
        } else {
            Raster::new(width, height, pixels.to_vec())?
        };
        raster.dpi = dpi;
        raster.sample_model = model;
        let profile = input.blob()?;
        let icc = (!profile.is_empty()).then(|| profile.to_vec());
        let metadata = input.blob()?;
        let mut exif = if metadata.is_empty() {
            None
        } else {
            Some(
                exif_metadata::Metadata::parse(metadata.to_vec())
                    .map_err(|_| Error::InvalidImage)?,
            )
        };
        let lossy = match input.number()? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidImage),
        };
        if let Some(metadata) = exif.as_mut() {
            metadata.lossy = lossy;
        }
        let count = input.number()?;
        if count > 3 {
            return Err(Error::InvalidImage);
        }
        let mut png_color = Vec::new();
        for _ in 0..count {
            let tag: [u8; 4] = input.take(4)?.try_into().unwrap();
            let data = input.blob()?;
            let valid = match &tag {
                b"gAMA" => data.len() == 4,
                b"cHRM" => data.len() == 32,
                b"sRGB" => data.len() == 1 && data[0] <= 3,
                _ => false,
            };
            if !valid || png_color.iter().any(|(existing, _)| existing == &tag) {
                return Err(Error::InvalidImage);
            }
            png_color.push((tag, data.to_vec()));
        }
        let count = input.number()?;
        if count > 8 {
            return Err(Error::InvalidImage);
        }
        let mut tiff_text = Vec::new();
        for _ in 0..count {
            let tag = *TIFF_TAGS
                .get(input.number()? as usize)
                .ok_or(Error::InvalidImage)?;
            if tiff_text.iter().any(|(existing, _)| *existing == tag) {
                return Err(Error::InvalidImage);
            }
            let text = std::str::from_utf8(input.blob()?)
                .map_err(|_| Error::InvalidImage)?
                .to_owned();
            if text.contains('\0') {
                return Err(Error::InvalidImage);
            }
            tiff_text.push((tag, text));
        }
        if !input.0.is_empty() {
            return Err(Error::InvalidImage);
        }
        Ok(Self {
            exif,
            png_color,
            tiff_text,
            icc,
            current: raster.clone(),
            saved: raster,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn samples_density_and_retained_metadata_survive_without_a_save_or_history_change() {
        let mut image =
            ImageDocument::from_rgba(2, 1, vec![1, 2, 3, 4, 5, 6, 7, 8], Some((300, 150))).unwrap();
        image.current.pixels16 = Some(vec![1, 257, 65534, 65535, 1234, 2345, 3456, 4567].into());
        image.current.pixels = image
            .current
            .pixels16
            .as_ref()
            .unwrap()
            .iter()
            .map(|v| ((u32::from(*v) + 128) / 257) as u8)
            .collect::<Vec<_>>()
            .into();
        image.icc = Some(vec![1, 2, 3, 4]);
        image.png_color = vec![(*b"gAMA", 45455u32.to_be_bytes().to_vec())];
        image.tiff_text = vec![(tiff::tags::Tag::Artist, "Мария ©".into())];
        let artist = exif::Field {
            tag: exif::Tag::Artist,
            ifd_num: exif::In::PRIMARY,
            value: exif::Value::Ascii(vec![b"Recovery Artist".to_vec()]),
        };
        let mut writer = exif::experimental::Writer::new();
        writer.push_field(&artist);
        let mut data = Cursor::new(Vec::new());
        writer.write(&mut data, true).unwrap();
        let mut exif = exif_metadata::Metadata::parse(data.into_inner()).unwrap();
        exif.lossy = true;
        image.exif = Some(exif);
        image.rotate_cw().unwrap();
        let saved = image.saved.clone();
        let undo = image.undo.len();
        let bytes = image.recovery_bytes().unwrap();
        let restored = ImageDocument::from_recovery_bytes(&bytes).unwrap();
        assert!(restored.current == image.current);
        assert_eq!(restored.icc, image.icc);
        assert_eq!(restored.png_color, image.png_color);
        assert_eq!(restored.tiff_text, image.tiff_text);
        assert!(restored.exif.as_ref().unwrap().lossy);
        let metadata = exif::Reader::new()
            .read_raw(
                restored
                    .exif
                    .as_ref()
                    .unwrap()
                    .encode(1, 2, Some((150, 300)))
                    .unwrap(),
            )
            .unwrap();
        assert!(metadata
            .get_field(exif::Tag::Artist, exif::In::PRIMARY)
            .is_some());
        assert!(image.saved == saved);
        assert_eq!(image.undo.len(), undo);
        assert_eq!(restored.recovery_bytes().unwrap(), bytes);
        assert!(!restored.can_undo());
        for length in [0, 7, 31, bytes.len() - 1] {
            assert!(ImageDocument::from_recovery_bytes(&bytes[..length]).is_err());
        }
        let mut extra = bytes;
        extra.push(0);
        assert!(ImageDocument::from_recovery_bytes(&extra).is_err());
    }
    #[test]
    fn invalid_dimensions_and_declared_lengths_are_refused_before_allocating() {
        let image = ImageDocument::from_rgba(1, 1, vec![1, 2, 3, 255], None).unwrap();
        let bytes = image.recovery_bytes().unwrap();
        for (offset, value) in [(8, u32::MAX), (16, 9601), (24, 4), (28, 2), (32, u32::MAX)] {
            let mut broken = bytes.clone();
            broken[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(ImageDocument::from_recovery_bytes(&broken).is_err());
        }
    }
}
