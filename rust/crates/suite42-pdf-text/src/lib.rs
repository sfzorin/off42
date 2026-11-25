//! Embedded Noto Sans Type0 font shared by PDF-producing applications.
use lopdf::{dictionary, Document, Object, ObjectId, Stream};
use std::collections::BTreeMap;

pub struct PdfFont {
    pub id: ObjectId,
    pub chars: BTreeMap<char, (u16, u16, u32)>,
}
impl PdfFont {
    pub fn validate(text: &str) -> Result<(), String> {
        let data = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf");
        let face =
            ttf_parser::Face::parse(data, 0).map_err(|e| format!("Invalid bundled font: {e:?}"))?;
        for ch in text.chars() {
            if face.glyph_index(ch).is_none() {
                return Err(format!("Bundled font lacks character {ch:?}"));
            }
        }
        Ok(())
    }
    pub fn embed(doc: &mut Document, text: &str) -> Result<Self, String> {
        Self::embed_impl(doc, text, false)
    }
    /// Embeds a font program reduced to the glyphs `text` needs. Glyph IDs are
    /// preserved, so the CIDToGIDMap matches the full font's.
    pub fn embed_subset(doc: &mut Document, text: &str) -> Result<Self, String> {
        Self::embed_impl(doc, text, true)
    }
    fn embed_impl(doc: &mut Document, text: &str, subset: bool) -> Result<Self, String> {
        // Noto Sans is distributed under OFL 1.1 alongside the bundled font.
        let font_data = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf");
        let face = ttf_parser::Face::parse(font_data, 0)
            .map_err(|e| format!("Invalid bundled font: {e:?}"))?;
        let mut chars = std::collections::BTreeMap::new();
        for ch in text.chars() {
            if !chars.contains_key(&ch) {
                let glyph = face
                    .glyph_index(ch)
                    .ok_or_else(|| format!("Bundled font lacks character {ch:?}"))?;
                let cid =
                    u16::try_from(chars.len() + 1).map_err(|_| "Too many distinct characters")?;
                let width = u32::from(face.glyph_hor_advance(glyph).unwrap_or(face.units_per_em()))
                    * 1000
                    / u32::from(face.units_per_em());
                chars.insert(ch, (cid, glyph.0, width));
            }
        }
        let font_stream = if subset {
            let glyphs: Vec<u16> = chars.values().map(|v| v.1).collect();
            let program = subset_truetype(font_data, &glyphs)?;
            doc.add_object(Stream::new(
                dictionary! {"Length1"=>program.len() as i64},
                program,
            ))
        } else {
            Self::full_font_stream(doc, font_data)
        };
        let descriptor = doc.add_object(dictionary! {"Type"=>"FontDescriptor", "FontName"=>"NotoSans", "Flags"=>32, "FontBBox"=>vec![(-1000).into(),(-1000).into(),3000.into(),3000.into()], "ItalicAngle"=>0, "Ascent"=>1100, "Descent"=>-300, "CapHeight"=>750, "StemV"=>80, "FontFile2"=>font_stream});
        let mut cid_to_gid = vec![0u8; (chars.len() + 1) * 2];
        let mut widths = vec![Object::Integer(0); chars.len() + 1];
        let mut cmap = String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /SCN42Unicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
        for chunk in chars.iter().collect::<Vec<_>>().chunks(100) {
            cmap.push_str(&format!("{} beginbfchar\n", chunk.len()));
            for (ch, &(cid, glyph, width)) in chunk {
                cid_to_gid[usize::from(cid) * 2..usize::from(cid) * 2 + 2]
                    .copy_from_slice(&glyph.to_be_bytes());
                widths[usize::from(cid)] = Object::Integer(width.into());
                let mut units = [0u16; 2];
                let utf16: String = ch
                    .encode_utf16(&mut units)
                    .iter()
                    .map(|v| format!("{v:04X}"))
                    .collect();
                cmap.push_str(&format!("<{cid:04X}> <{utf16}>\n"));
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        let map = doc.add_object(Stream::new(dictionary! {}, cid_to_gid));
        let unicode = doc.add_object(Stream::new(dictionary! {}, cmap.into_bytes()));
        let cidfont = doc.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"CIDFontType2", "BaseFont"=>"NotoSans", "CIDSystemInfo"=>dictionary! {"Registry"=>Object::string_literal("Adobe"), "Ordering"=>Object::string_literal("Identity"), "Supplement"=>0}, "FontDescriptor"=>descriptor, "CIDToGIDMap"=>map, "W"=>vec![Object::Integer(0), Object::Array(widths)]});
        let font = doc.add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type0", "BaseFont"=>"NotoSans", "Encoding"=>"Identity-H", "DescendantFonts"=>vec![Object::Reference(cidfont)], "ToUnicode"=>unicode});
        Ok(Self { id: font, chars })
    }
    /// Reuses only an actual identical embedded font stream. CID maps and
    /// ToUnicode remain separate for each independently encoded character set.
    fn full_font_stream(doc: &mut Document, font_data: &[u8]) -> ObjectId {
        let existing = doc
            .objects
            .iter()
            .filter_map(|(id, obj)| obj.as_stream().ok().map(|s| (*id, s)))
            .take(10_000)
            .find_map(|(id, stream)| {
                if stream
                    .dict
                    .get(b"Length1")
                    .ok()
                    .and_then(|v| v.as_i64().ok())
                    != Some(font_data.len() as i64)
                {
                    return None;
                }
                if stream.dict.get(b"Filter").is_ok() {
                    stream
                        .decompressed_content_with_limit(font_data.len() + 1)
                        .ok()
                        .filter(|bytes| bytes.as_slice() == font_data)
                        .map(|_| id)
                } else if stream.content.as_slice() == font_data {
                    Some(id)
                } else {
                    None
                }
            });
        existing.unwrap_or_else(|| {
            doc.add_object(Stream::new(
                dictionary! {"Length1"=>font_data.len() as i64},
                font_data.to_vec(),
            ))
        })
    }
    pub fn encode(&self, text: &str) -> Result<String, String> {
        text.chars()
            .map(|c| {
                self.chars
                    .get(&c)
                    .map(|v| format!("{:04X}", v.0))
                    .ok_or_else(|| format!("Font lacks character {c:?}"))
            })
            .collect()
    }
}

/// Tables a PDF consumer needs from an embedded TrueType program (ISO 32000
/// 9.9) plus OS/2. Layout, variation and naming tables are dropped, which
/// leaves the default instance of a variable font.
const SUBSET_TABLES: [&[u8; 4]; 10] = [
    b"OS/2", b"cvt ", b"fpgm", b"glyf", b"head", b"hhea", b"hmtx", b"loca", b"maxp", b"prep",
];

/// Returns a TrueType program in which every glyph outside `glyphs` (and the
/// components they reference) is empty. Glyph IDs are unchanged.
fn subset_truetype(font: &[u8], glyphs: &[u16]) -> Result<Vec<u8>, String> {
    let bad = || "Invalid bundled font tables".to_string();
    let u16_at = |d: &[u8], o: usize| -> Result<u16, String> {
        d.get(o..o + 2)
            .map(|b| u16::from_be_bytes([b[0], b[1]]))
            .ok_or_else(bad)
    };
    let u32_at = |d: &[u8], o: usize| -> Result<u32, String> {
        d.get(o..o + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(bad)
    };
    let mut tables = BTreeMap::new();
    for i in 0..usize::from(u16_at(font, 4)?) {
        let record = 12 + 16 * i;
        let tag: [u8; 4] = font
            .get(record..record + 4)
            .ok_or_else(bad)?
            .try_into()
            .map_err(|_| bad())?;
        let offset = u32_at(font, record + 8)? as usize;
        let len = u32_at(font, record + 12)? as usize;
        let data = font.get(offset..offset + len).ok_or_else(bad)?;
        if SUBSET_TABLES.contains(&&tag) {
            tables.insert(tag, data.to_vec());
        }
    }
    let head = tables.get(b"head").ok_or_else(bad)?;
    let long_loca = u16_at(head, 50)? != 0;
    let glyph_count = usize::from(u16_at(tables.get(b"maxp").ok_or_else(bad)?, 4)?);
    let loca = tables.get(b"loca").ok_or_else(bad)?;
    let glyf = tables.get(b"glyf").ok_or_else(bad)?;
    let mut ranges = Vec::with_capacity(glyph_count);
    for gid in 0..glyph_count {
        let (start, end) = if long_loca {
            (
                u32_at(loca, gid * 4)? as usize,
                u32_at(loca, gid * 4 + 4)? as usize,
            )
        } else {
            (
                usize::from(u16_at(loca, gid * 2)?) * 2,
                usize::from(u16_at(loca, gid * 2 + 2)?) * 2,
            )
        };
        if start > end || end > glyf.len() {
            return Err(bad());
        }
        ranges.push(start..end);
    }
    // Glyph 0 (.notdef) is always kept; composite glyphs pull in components.
    let mut keep = vec![false; glyph_count];
    let mut pending: Vec<u16> = std::iter::once(0).chain(glyphs.iter().copied()).collect();
    while let Some(gid) = pending.pop() {
        let gid = usize::from(gid);
        if gid >= glyph_count || std::mem::replace(&mut keep[gid], true) {
            continue;
        }
        let data = &glyf[ranges[gid].clone()];
        if data.len() < 10 || (u16_at(data, 0)? as i16) >= 0 {
            continue;
        }
        let mut at = 10;
        loop {
            let flags = u16_at(data, at)?;
            pending.push(u16_at(data, at + 2)?);
            at += 4 + if flags & 0x0001 != 0 { 4 } else { 2 };
            at += match flags {
                f if f & 0x0008 != 0 => 2,
                f if f & 0x0040 != 0 => 4,
                f if f & 0x0080 != 0 => 8,
                _ => 0,
            };
            if flags & 0x0020 == 0 {
                break;
            }
        }
    }
    let mut new_glyf = Vec::new();
    let mut new_loca = Vec::with_capacity((glyph_count + 1) * 4);
    for (gid, range) in ranges.iter().enumerate() {
        new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
        if keep[gid] {
            new_glyf.extend_from_slice(&glyf[range.clone()]);
            new_glyf.resize(new_glyf.len().next_multiple_of(4), 0);
        }
    }
    new_loca.extend_from_slice(&(new_glyf.len() as u32).to_be_bytes());
    tables.insert(*b"glyf", new_glyf);
    tables.insert(*b"loca", new_loca);
    let head = tables.get_mut(b"head").ok_or_else(bad)?;
    head[8..12].fill(0);
    head[50..52].copy_from_slice(&1u16.to_be_bytes());

    let checksum = |d: &[u8]| {
        d.chunks(4).fold(0u32, |sum, c| {
            let mut word = [0u8; 4];
            word[..c.len()].copy_from_slice(c);
            sum.wrapping_add(u32::from_be_bytes(word))
        })
    };
    let count = tables.len() as u16;
    let entry_selector = 15 - count.leading_zeros() as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    for v in [
        count,
        search_range,
        entry_selector,
        count * 16 - search_range,
    ] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + 16 * tables.len();
    let mut head_offset = 0;
    for (tag, data) in &tables {
        if tag == b"head" {
            head_offset = offset;
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&(offset as u32).to_be_bytes());
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for data in tables.values() {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    let adjustment = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
    out[head_offset + 8..head_offset + 12].copy_from_slice(&adjustment.to_be_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_keeps_used_glyphs_and_ids() {
        let full = include_bytes!("../../doc42-layout-probe/fixtures/NotoSans.ttf");
        let full_face = ttf_parser::Face::parse(full, 0).unwrap();
        let used: Vec<_> = "Привет"
            .chars()
            .map(|c| full_face.glyph_index(c).unwrap().0)
            .collect();
        let program = subset_truetype(full, &used).unwrap();
        assert!(program.len() < 100_000, "{} bytes", program.len());
        let face = ttf_parser::Face::parse(&program, 0).unwrap();
        assert_eq!(face.number_of_glyphs(), full_face.number_of_glyphs());
        let unused = full_face.glyph_index('Z').unwrap();
        for gid in used {
            let gid = ttf_parser::GlyphId(gid);
            assert!(face.glyph_bounding_box(gid).is_some());
            assert_eq!(
                face.glyph_hor_advance(gid),
                full_face.glyph_hor_advance(gid)
            );
        }
        assert!(full_face.glyph_bounding_box(unused).is_some());
        assert!(face.glyph_bounding_box(unused).is_none());
    }
}
