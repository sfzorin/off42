//! Rebuild metadata from parsed fields; never copy stale thumbnail structures or private offsets.
use exif::{Field, In, Tag, Value};
use std::io::Cursor;

pub struct Metadata {
    fields: Vec<Field>,
    pub lossy: bool,
}
impl Metadata {
    pub fn parse(raw: Vec<u8>) -> Result<Self, String> {
        let data = exif::Reader::new()
            .read_raw(raw)
            .map_err(|e| e.to_string())?;
        let mut fields = Vec::new();
        let mut lossy = false;
        for f in data.fields().filter(|f| f.ifd_num == In::PRIMARY) {
            // Maker notes routinely contain vendor-specific offsets that cannot be relocated safely.
            if f.tag == Tag::MakerNote || matches!(f.value, Value::Unknown(..)) {
                lossy = true;
                continue;
            }
            fields.push(f.clone());
        }
        Ok(Self { fields, lossy })
    }
    pub fn encode(
        &self,
        width: u32,
        height: u32,
        dpi: Option<(u32, u32)>,
    ) -> Result<Vec<u8>, String> {
        let mut fields = Vec::new();
        for f in &self.fields {
            if matches!(
                f.tag,
                Tag::Orientation
                    | Tag::ImageWidth
                    | Tag::ImageLength
                    | Tag::PixelXDimension
                    | Tag::PixelYDimension
            ) {
                continue;
            }
            if dpi.is_some()
                && matches!(
                    f.tag,
                    Tag::XResolution | Tag::YResolution | Tag::ResolutionUnit
                )
            {
                continue;
            }
            fields.push(f.clone());
        }
        let field = |tag, value| Field {
            tag,
            ifd_num: In::PRIMARY,
            value,
        };
        fields.push(field(Tag::Orientation, Value::Short(vec![1])));
        fields.push(field(Tag::ImageWidth, Value::Long(vec![width])));
        fields.push(field(Tag::ImageLength, Value::Long(vec![height])));
        fields.push(field(Tag::PixelXDimension, Value::Long(vec![width])));
        fields.push(field(Tag::PixelYDimension, Value::Long(vec![height])));
        if let Some((x, y)) = dpi {
            fields.push(field(
                Tag::XResolution,
                Value::Rational(vec![exif::Rational { num: x, denom: 1 }]),
            ));
            fields.push(field(
                Tag::YResolution,
                Value::Rational(vec![exif::Rational { num: y, denom: 1 }]),
            ));
            fields.push(field(Tag::ResolutionUnit, Value::Short(vec![2])));
        }
        let mut writer = exif::experimental::Writer::new();
        for f in &fields {
            writer.push_field(f);
        }
        let mut bytes = Cursor::new(Vec::new());
        writer.write(&mut bytes, true).map_err(|e| e.to_string())?;
        Ok(bytes.into_inner())
    }
}
