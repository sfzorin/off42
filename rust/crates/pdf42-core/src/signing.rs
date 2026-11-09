//! Construct an unsigned placeholder and complete it without moving the signed byte ranges.
use super::{form_dictionary, form_fields_array, form_root_names, PdfDocument, MAX_FILE};
use lopdf::{dictionary, Dictionary, Document, LoadOptions, Object, StringFormat};
use std::ops::Range;

const RESERVE: usize = 64 * 1024;
const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub struct SignatureDraft {
    bytes: Vec<u8>,
    gap: Range<usize>,
}

impl SignatureDraft {
    pub fn signed_bytes(&self) -> [&[u8]; 2] {
        [&self.bytes[..self.gap.start], &self.bytes[self.gap.end..]]
    }

    pub fn finish(mut self, cms: &[u8]) -> Result<Vec<u8>, String> {
        if cms.is_empty() || cms.len() > RESERVE {
            return Err("CMS signature is empty or exceeds the PDF reserve".into());
        }
        let hex = &mut self.bytes[self.gap.start + 1..self.gap.end - 1];
        hex.fill(b'0');
        for (pair, &byte) in hex.chunks_exact_mut(2).zip(cms) {
            pair[0] = HEX[usize::from(byte >> 4)];
            pair[1] = HEX[usize::from(byte & 15)];
        }
        Ok(self.bytes)
    }
}

impl PdfDocument {
    /// Creates a detached approval signature field in a disposable copy of the current revision.
    pub fn prepare_signature(&self) -> Result<SignatureDraft, String> {
        self.ensure_unsigned_edit()?;
        if self.was_password_protected() {
            return Err("Choose an unencrypted PDF for certificate signing".into());
        }
        let (snapshot, _) = self.save_snapshot()?;
        if snapshot.len() as u64 > MAX_FILE {
            return Err("PDF exceeds the signing size limit".into());
        }
        let mut doc = Document::load_mem_with_options(
            &snapshot,
            LoadOptions {
                max_decompressed_size: Some(32 * 1024 * 1024),
                ..Default::default()
            },
        )
        .map_err(|error| error.to_string())?;
        let page = *doc
            .get_pages()
            .values()
            .nth(self.current())
            .ok_or("Current page is missing")?;
        let root = doc
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .map_err(|error| error.to_string())?;
        let catalog = doc.catalog().map_err(|error| error.to_string())?;
        if catalog.get(b"Perms").is_ok() {
            return Err(
                "PDF permissions or certification require a different signing workflow".into(),
            );
        }
        let (mut form, mut fields) = match catalog.get(b"AcroForm") {
            Ok(object) => {
                let form = form_dictionary(&doc, object)?;
                if form.get(b"XFA").is_ok() {
                    return Err("XFA forms are outside this signing workflow".into());
                }
                (form.clone(), form_fields_array(&doc, form)?)
            }
            Err(_) => (Dictionary::new(), Vec::new()),
        };
        let old_flags = match form.get(b"SigFlags") {
            Ok(value) => value.as_i64().map_err(|error| error.to_string())?,
            Err(_) => 0,
        };
        if old_flags < 0 {
            return Err("Invalid signature flags".into());
        }
        let page_dict = doc
            .get_dictionary(page)
            .map_err(|error| error.to_string())?;
        let mut annots = match page_dict.get(b"Annots") {
            Ok(value) => doc
                .dereference(value)
                .and_then(|(_, value)| value.as_array())
                .map_err(|error| error.to_string())?
                .clone(),
            Err(_) => Vec::new(),
        };
        let mut nonce = [0u8; 16];
        getrandom::fill(&mut nonce)
            .map_err(|error| format!("Cannot generate signature placeholders: {error}"))?;
        let text_nonce: String = nonce.iter().map(|byte| format!("{byte:02X}")).collect();
        let field_name = format!("off42 Signature {text_nonce}");
        if !fields.is_empty() && form_root_names(&doc, &form)?.contains(&field_name) {
            return Err("Signature field name collision".into());
        }
        let mut contents = vec![0; RESERVE];
        contents[..nonce.len()].copy_from_slice(&nonce);
        let mut placeholders = [0i64; 3];
        for (i, bytes) in nonce.chunks_exact(4).take(3).enumerate() {
            placeholders[i] = 1_000_000_000
                + i as i64 * 1_000_000_000
                + i64::from(
                    u32::from_le_bytes(bytes.try_into().expect("four-byte chunk")) % 1_000_000_000,
                );
        }
        let [a, b, c] = placeholders;
        let signature = doc.add_object(dictionary! {
            "Type" => "Sig",
            "Filter" => "Adobe.PPKLite",
            "SubFilter" => "adbe.pkcs7.detached",
            "ByteRange" => vec![0.into(), a.into(), b.into(), c.into()],
            "Contents" => Object::String(contents, StringFormat::Hexadecimal),
        });
        let widget = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Widget",
            "FT" => "Sig", "F" => 4, "P" => page,
            "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
            "T" => lopdf::text_string(&field_name), "V" => signature,
        });
        fields.push(widget.into());
        annots.push(widget.into());
        form.set("Fields", fields);
        form.set("SigFlags", old_flags | 3);
        doc.get_dictionary_mut(root)
            .map_err(|error| error.to_string())?
            .set("AcroForm", form);
        doc.get_dictionary_mut(page)
            .map_err(|error| error.to_string())?
            .set("Annots", annots);

        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).map_err(|error| error.to_string())?;
        if bytes.len() as u64 > MAX_FILE {
            return Err("Prepared PDF exceeds the signing size limit".into());
        }
        let content_marker = format!("/Contents<{text_nonce}");
        let start = unique_marker(&bytes, content_marker.as_bytes())? + b"/Contents".len();
        let end = start + 2 * RESERVE + 2;
        let tail = bytes
            .get(start + 1 + text_nonce.len()..end)
            .ok_or("PDF signature placeholder is incomplete")?;
        if !tail
            .strip_suffix(b">")
            .is_some_and(|tail| tail.iter().all(|&byte| byte == b'0'))
        {
            return Err("PDF signature placeholder changed during serialization".into());
        }
        let marker = format!("/ByteRange[0 {a} {b} {c}]");
        let range = unique_marker(&bytes, marker.as_bytes())?;
        let value = format!(
            "/ByteRange[0 {start:010} {end:010} {:010}]",
            bytes.len() - end
        );
        if value.len() != marker.len() {
            return Err("PDF signature byte range exceeds reserved space".into());
        }
        bytes[range..range + value.len()].copy_from_slice(value.as_bytes());
        Ok(SignatureDraft {
            bytes,
            gap: start..end,
        })
    }
}

fn unique_marker(bytes: &[u8], marker: &[u8]) -> Result<usize, String> {
    let mut matches = bytes
        .windows(marker.len())
        .enumerate()
        .filter(|(_, bytes)| *bytes == marker)
        .map(|(index, _)| index);
    let index = matches
        .next()
        .ok_or("Missing serialized PDF signature placeholder")?;
    if matches.next().is_some() {
        return Err("Ambiguous PDF signature placeholder".into());
    }
    Ok(index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_signature_draft_preserves_existing_form_and_freezes_all_signed_bytes() {
        let mut source = Document::with_version("1.5");
        let root = source.new_object_id();
        let field = source.add_object(dictionary! {
            "FT" => "Tx", "T" => lopdf::text_string("Reference"),
            "V" => lopdf::text_string("Board 42"),
        });
        let page = source.add_object(dictionary! {
            "Type" => "Page", "Parent" => root, "MediaBox" => vec![0.into(), 0.into(), 300.into(), 200.into()],
        });
        source.objects.insert(
            root,
            dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
            }
            .into(),
        );
        let catalog = source.add_object(dictionary! {
            "Type" => "Catalog", "Pages" => root, "AcroForm" => dictionary! {
                "Fields" => vec![Object::Reference(field)],
            },
        });
        source.trailer.set("Root", catalog);
        let doc = PdfDocument::from_loaded_document(source, false).unwrap();
        let draft = doc.prepare_signature().unwrap();
        let [left, right] = draft.signed_bytes().map(<[u8]>::to_vec);
        let cms = b"\x30\x01\x00";
        let result = draft.finish(cms).unwrap();
        assert!(result.starts_with(&left) && result.ends_with(&right));
        let output = Document::load_mem(&result).unwrap();
        let form =
            form_dictionary(&output, output.catalog().unwrap().get(b"AcroForm").unwrap()).unwrap();
        let fields = form_fields_array(&output, form).unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(form.get(b"SigFlags").unwrap().as_i64().unwrap(), 3);
        let original_field = output.dereference(&fields[0]).unwrap().1.as_dict().unwrap();
        assert_eq!(
            lopdf::decode_text_string(original_field.get(b"V").unwrap()).unwrap(),
            "Board 42"
        );
        let widget = output.dereference(&fields[1]).unwrap().1.as_dict().unwrap();
        let sig = output
            .dereference(widget.get(b"V").unwrap())
            .unwrap()
            .1
            .as_dict()
            .unwrap();
        let range = sig.get(b"ByteRange").unwrap().as_array().unwrap();
        let positions: Vec<_> = range
            .iter()
            .map(|value| value.as_i64().unwrap() as usize)
            .collect();
        assert_eq!(
            positions,
            [0, left.len(), result.len() - right.len(), right.len()]
        );
        let contents = sig.get(b"Contents").unwrap().as_str().unwrap();
        assert!(contents.starts_with(cms));
        assert!(contents[cms.len()..].iter().all(|&byte| byte == 0));
        assert!(doc.signature_source().is_none() && !doc.is_dirty() && !doc.can_undo());
    }
}
