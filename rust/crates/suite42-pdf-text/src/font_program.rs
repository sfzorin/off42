//! Immutable, bounded TrueType programs whose embedding flags permit document use.
use super::*;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
#[derive(Clone)]
pub struct FontProgram {
    data: Arc<[u8]>,
    name: String,
    id: u64,
}
impl std::fmt::Debug for FontProgram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontProgram")
            .field("name", &self.name)
            .field("bytes", &self.data.len())
            .finish()
    }
}
impl PartialEq for FontProgram {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id || self.data == other.data
    }
}
impl FontProgram {
    pub fn new(data: Vec<u8>) -> Result<Self, String> {
        if data.len() > 16 * 1024 * 1024
            || data.len() < 12
            || !matches!(&data[..4], b"\0\x01\0\0" | b"true")
        {
            return Err("Use a standalone TrueType font up to 16 MB".into());
        }
        let face = ttf_parser::Face::parse(&data, 0).map_err(|e| format!("Invalid font: {e:?}"))?;
        if !matches!(
            face.permissions(),
            Some(
                ttf_parser::Permissions::Installable
                    | ttf_parser::Permissions::Editable
                    | ttf_parser::Permissions::PreviewAndPrint
            )
        ) || !face.is_outline_embedding_allowed()
        {
            return Err("Font embedding is not permitted by its embedding flags".into());
        }
        if face.tables().glyf.is_none() {
            return Err("Font has no TrueType outlines".into());
        }
        let name = face
            .names()
            .into_iter()
            .filter(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME)
            .filter_map(|n| n.to_string())
            .find(|n| {
                !n.is_empty()
                    && n.len() <= 128
                    && n.bytes()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_'))
            })
            .unwrap_or_else(|| "Off42Embedded".into());
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let id = NEXT
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| v.checked_add(1))
            .map_err(|_| "Font identity limit reached")?;
        Ok(Self {
            data: data.into(),
            name,
            id,
        })
    }
    pub fn byte_len(&self) -> usize {
        self.data.len()
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Process-local identity shared by all clones; distinct font programs cannot alias a glyph cache.
    pub fn cache_id(&self) -> u64 {
        self.id
    }
    pub fn validate(&self, text: &str) -> Result<(), String> {
        let face = ttf_parser::Face::parse(&self.data, 0).map_err(|_| "Invalid font")?;
        validate_face(&face, text)
    }
    pub fn embed_subset(&self, doc: &mut Document, text: &str) -> Result<PdfFont, String> {
        let face = ttf_parser::Face::parse(&self.data, 0).map_err(|_| "Invalid font")?;
        PdfFont::embed_data(
            doc,
            text,
            face.is_subsetting_allowed(),
            false,
            &self.data,
            &self.name,
        )
    }
}
fn validate_face(face: &ttf_parser::Face<'_>, text: &str) -> Result<(), String> {
    for ch in text.chars() {
        if face.glyph_index(ch).is_none() {
            return Err(format!("Selected font lacks character {ch:?}"));
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn font(flags: u16) -> Vec<u8> {
        let mut data = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf").to_vec();
        let tables = u16::from_be_bytes(data[4..6].try_into().unwrap()) as usize;
        let record = (0..tables)
            .map(|i| 12 + 16 * i)
            .find(|&i| &data[i..i + 4] == b"OS/2")
            .unwrap();
        let offset = u32::from_be_bytes(data[record + 8..record + 12].try_into().unwrap()) as usize;
        data[offset + 8..offset + 10].copy_from_slice(&flags.to_be_bytes());
        data
    }
    #[test]
    fn font_program_respects_restricted_bitmap_and_no_subset_flags() {
        assert!(FontProgram::new(font(2)).is_err());
        assert!(FontProgram::new(font(512)).is_err());
        assert!(FontProgram::new(vec![0; 100]).is_err());
        for flags in [0, 4, 8, 256] {
            let data = font(flags);
            let program = FontProgram::new(data.clone()).unwrap();
            program.validate("Arial АБВ 42").unwrap();
            assert!(program.validate("\u{10FFFF}").is_err());
            let mut doc = Document::with_version("1.7");
            let encoded = program.embed_subset(&mut doc, "Arial АБВ 42").unwrap();
            assert!(!encoded.encode("Arial АБВ 42").unwrap().is_empty());
            let sizes: Vec<_> = doc
                .objects
                .values()
                .filter_map(|o| o.as_stream().ok())
                .filter_map(|s| s.dict.get(b"Length1").ok().and_then(|o| o.as_i64().ok()))
                .collect();
            assert_eq!(sizes.len(), 1);
            if flags == 256 {
                assert_eq!(sizes[0], data.len() as i64)
            } else {
                assert!(sizes[0] < data.len() as i64)
            }
        }
    }
}
