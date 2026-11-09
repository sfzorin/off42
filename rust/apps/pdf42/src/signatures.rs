//! Bind detached signature values to the byte ranges of actual AcroForm fields.
use lopdf::{Dictionary, Document, LoadOptions, Object, StringFormat};
use std::{collections::HashSet, ops::Range};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::Worker;

const MAX_FILE: usize = 256 * 1024 * 1024;
const MAX_FIELDS: usize = 20_000;
const MAX_SIGNATURES: usize = 32;
const MAX_GAP: usize = 1024 * 1024;

struct Field<'a> {
    name: String,
    value: &'a Dictionary,
}

struct Signature<'a> {
    cms: &'a [u8],
    left: Range<usize>,
    right: Range<usize>,
}

fn load(data: &[u8]) -> Result<Document, String> {
    if data.len() > MAX_FILE {
        return Err("PDF exceeds the signature inspection limit".into());
    }
    let document = Document::load_mem_with_options(
        data,
        LoadOptions {
            max_decompressed_size: Some(32 * 1024 * 1024),
            ..Default::default()
        },
    )
    .map_err(|error| error.to_string())?;
    if document.is_encrypted() || document.was_encrypted() {
        return Err("Signature inspection for encrypted PDFs is unavailable".into());
    }
    Ok(document)
}

fn fields(doc: &Document) -> Result<Vec<Field<'_>>, String> {
    let catalog = doc.catalog().map_err(|error| error.to_string())?;
    let Ok(form) = catalog.get(b"AcroForm") else {
        return Ok(Vec::new());
    };
    let form = doc
        .dereference(form)
        .and_then(|(_, obj)| obj.as_dict())
        .map_err(|error| error.to_string())?;
    let roots = form
        .get(b"Fields")
        .and_then(|value| doc.dereference(value))
        .and_then(|(_, obj)| obj.as_array())
        .map_err(|error| error.to_string())?;
    if roots.len() > MAX_FIELDS {
        return Err("PDF signature field limit exceeded".into());
    }
    let mut pending: Vec<_> = roots.iter().rev().map(|value| (value, false, 0)).collect();
    let mut inspected = 0usize;
    let mut visited = HashSet::new();
    let mut signatures = Vec::new();
    while let Some((value, inherited, depth)) = pending.pop() {
        inspected += 1;
        if inspected > MAX_FIELDS || depth > 64 {
            return Err("PDF signature field limit exceeded".into());
        }
        let (id, object) = doc.dereference(value).map_err(|error| error.to_string())?;
        if id.is_some_and(|id| !visited.insert(id)) {
            continue;
        }
        let field = object.as_dict().map_err(|error| error.to_string())?;
        let is_signature = field
            .get(b"FT")
            .ok()
            .map_or(inherited, |kind| kind.as_name().ok() == Some(b"Sig"));
        if is_signature {
            if let Ok(value) = field.get(b"V") {
                let (_, value) = doc.dereference(value).map_err(|error| error.to_string())?;
                if !matches!(value, Object::Null) {
                    if signatures.len() == MAX_SIGNATURES {
                        return Err("Too many signed fields".into());
                    }
                    let name = field
                        .get(b"T")
                        .ok()
                        .and_then(|value| lopdf::decode_text_string(value).ok())
                        .map_or_else(
                            || "Signature".into(),
                            |name| name.chars().filter(|c| !c.is_control()).take(100).collect(),
                        );
                    signatures.push(Field {
                        name,
                        value: value.as_dict().map_err(|error| error.to_string())?,
                    });
                }
            }
        }
        if let Ok(kids) = field.get(b"Kids") {
            let kids = doc
                .dereference(kids)
                .and_then(|(_, obj)| obj.as_array())
                .map_err(|error| error.to_string())?;
            if kids.len().saturating_add(pending.len()) > MAX_FIELDS {
                return Err("PDF signature field limit exceeded".into());
            }
            pending.extend(
                kids.iter()
                    .rev()
                    .map(|value| (value, is_signature, depth + 1)),
            );
        }
    }
    Ok(signatures)
}

fn whitespace(value: u8) -> bool {
    matches!(value, 0 | b'\t' | b'\n' | 12 | b'\r' | b' ')
}

fn trim_end(bytes: &[u8]) -> &[u8] {
    let end = bytes
        .iter()
        .rposition(|&value| !whitespace(value))
        .map_or(0, |index| index + 1);
    &bytes[..end]
}

fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

fn raw_contents_matches(gap: &[u8], bytes: &[u8]) -> bool {
    let Some(gap) = gap
        .strip_prefix(b"<")
        .and_then(|value| value.strip_suffix(b">"))
    else {
        return false;
    };
    let mut hex = gap.iter().copied().filter(|&value| !whitespace(value));
    for &byte in bytes {
        let Some(high) = hex.next().and_then(hex_digit) else {
            return false;
        };
        let low = match hex.next() {
            Some(value) => match hex_digit(value) {
                Some(digit) => digit,
                None => return false,
            },
            None => 0,
        };
        if high * 16 + low != byte {
            return false;
        }
    }
    hex.next().is_none()
}

fn der_value(bytes: &[u8], tag: u8) -> Option<(&[u8], &[u8])> {
    if bytes.first() != Some(&tag) {
        return None;
    }
    let marker = *bytes.get(1)?;
    let (header, length) = if marker < 128 {
        (2, usize::from(marker))
    } else {
        let size = usize::from(marker & 0x7f);
        if !(1..=4).contains(&size) {
            return None;
        }
        let length = bytes
            .get(2..2 + size)?
            .iter()
            .try_fold(0usize, |length, &byte| {
                length.checked_mul(256)?.checked_add(usize::from(byte))
            })?;
        if length < 128 || bytes[2] == 0 {
            return None;
        }
        (2 + size, length)
    };
    let end = header.checked_add(length)?;
    Some((bytes.get(header..end)?, bytes.get(end..)?))
}

fn cms_der(bytes: &[u8]) -> Option<&[u8]> {
    let signed_data = b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x02";
    let plain_data = b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x01";
    let (info, padding) = der_value(bytes, 0x30)?;
    if padding.iter().any(|&byte| byte != 0) {
        return None;
    }
    let (kind, rest) = der_value(info, 0x06)?;
    if kind != signed_data {
        return None;
    }
    let (signed, rest) = der_value(rest, 0xa0)?;
    let (body, after) = der_value(signed, 0x30)?;
    if !rest.is_empty() || !after.is_empty() {
        return None;
    }
    let (_, body) = der_value(body, 0x02)?;
    let (_, body) = der_value(body, 0x31)?;
    let (content, _) = der_value(body, 0x30)?;
    let (kind, attached) = der_value(content, 0x06)?;
    if kind != plain_data || !attached.is_empty() {
        return None;
    }
    Some(&bytes[..bytes.len() - padding.len()])
}

fn envelope<'a>(field: &Field<'a>, data: &[u8]) -> Result<Signature<'a>, String> {
    let value = field.value;
    if !matches!(
        value
            .get(b"SubFilter")
            .ok()
            .and_then(|object| object.as_name().ok()),
        Some(b"adbe.pkcs7.detached" | b"ETSI.CAdES.detached")
    ) {
        return Err("Unsupported PDF signature format".into());
    }
    let range = value
        .get(b"ByteRange")
        .and_then(|object| object.as_array())
        .map_err(|_| "Missing signature byte ranges")?;
    let [start, a, b, length] = range.as_slice() else {
        return Err("Expected exactly two signature byte ranges".into());
    };
    let number = |object: &Object| object.as_i64().ok().and_then(|n| usize::try_from(n).ok());
    let (a, b, length) = match (number(start), number(a), number(b), number(length)) {
        (Some(0), Some(a), Some(b), Some(length))
            if a > 0 && b > a && b - a <= MAX_GAP && length > 0 =>
        {
            (a, b, length)
        }
        _ => return Err("Invalid PDF signature byte ranges".into()),
    };
    let end = b
        .checked_add(length)
        .filter(|&end| end <= data.len())
        .ok_or("PDF signature ranges extend past the file")?;
    if !trim_end(&data[end.saturating_sub(65_536)..end]).ends_with(b"%%EOF") {
        return Err("Signature does not cover a complete PDF revision".into());
    }
    let contents = match value.get(b"Contents") {
        Ok(Object::String(data, StringFormat::Hexadecimal)) => data,
        _ => return Err("Expected hexadecimal PDF signature contents".into()),
    };
    if !trim_end(&data[a.saturating_sub(512)..a]).ends_with(b"/Contents")
        || !raw_contents_matches(&data[a..b], contents)
    {
        return Err("Signature byte ranges do not surround its Contents value".into());
    }
    let cms = cms_der(contents).ok_or("Invalid or non-detached CMS signature")?;
    Ok(Signature {
        cms,
        left: 0..a,
        right: b..end,
    })
}

/// A later revision can replace a field dictionary. Check it also exists in the signed prefix.
fn is_bound_to_revision(data: &[u8], signature: &Signature<'_>) -> Result<bool, String> {
    let end = signature.right.end;
    if end == data.len() {
        return Ok(true);
    }
    let source = data
        .get(..end)
        .ok_or("Signed revision exceeds the file length")?;
    let revision = load(source)?;
    Ok(fields(&revision)?.iter().any(|field| {
        envelope(field, source).is_ok_and(|old| {
            old.left == signature.left && old.right == signature.right && old.cms == signature.cms
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut bytes = vec![tag, u8::try_from(value.len()).unwrap()];
        bytes.extend(value);
        bytes
    }

    fn fake_cms(attached: bool) -> Vec<u8> {
        let signed_data = tlv(0x06, b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x02");
        let mut content = tlv(0x06, b"\x2a\x86\x48\x86\xf7\x0d\x01\x07\x01");
        if attached {
            content.extend(tlv(0xa0, &tlv(0x04, b"embedded")));
        }
        let mut body = tlv(0x02, &[1]);
        body.extend(tlv(0x31, &[]));
        body.extend(tlv(0x30, &content));
        body.extend(tlv(0x31, &[]));
        tlv(0x30, &[signed_data, tlv(0xa0, &tlv(0x30, &body))].concat())
    }

    fn pdf(cms: &[u8], attach_field: bool) -> Vec<u8> {
        use std::io::Write;
        let catalog = if attach_field {
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>"
        } else {
            "<< /Type /Catalog /Pages 2 0 R >>"
        };
        let blank_range = "[0 0000000000 0000000000 0000000000]";
        let contents = cms
            .iter()
            .chain(&[0; 5])
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>();
        let sig = format!("<< /Type /Sig /SubFilter /adbe.pkcs7.detached /ByteRange {blank_range} /Contents <{contents}> >>");
        let mut data = b"%PDF-1.5\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in [
            catalog,
            "<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] >>",
            "<< /FT /Sig /T (Audit) /V 5 0 R >>",
            &sig,
        ]
        .into_iter()
        .enumerate()
        {
            offsets.push(data.len());
            writeln!(data, "{} 0 obj\n{object}\nendobj", i + 1).unwrap();
        }
        let xref = data.len();
        writeln!(data, "xref\n0 6\n0000000000 65535 f ").unwrap();
        for offset in offsets {
            writeln!(data, "{offset:010} 00000 n ").unwrap();
        }
        writeln!(
            data,
            "trailer\n<< /Root 1 0 R /Size 6 >>\nstartxref\n{xref}\n%%EOF"
        )
        .unwrap();
        let source = std::str::from_utf8(&data).unwrap();
        let gap = source.find("/Contents <").unwrap() + "/Contents ".len();
        let after = gap + contents.len() + 2;
        let range_start = source.find(blank_range).unwrap();
        let range = format!("[0 {gap:010} {after:010} {:010}]", data.len() - after);
        data[range_start..range_start + range.len()].copy_from_slice(range.as_bytes());
        data
    }

    #[test]
    fn binds_only_signed_fields_to_exact_contents_and_complete_revisions() {
        let cms = fake_cms(false);
        let mut bytes = pdf(&cms, true);
        let signed_end = bytes.len();
        bytes.extend(b"\n% unsigned later bytes\n");
        let doc = load(&bytes).unwrap();
        let fields = fields(&doc).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "Audit");
        let value = envelope(&fields[0], &bytes).unwrap();
        assert_eq!(value.cms, cms);
        assert_eq!(value.right.end, signed_end);
        assert!(is_bound_to_revision(&bytes, &value).unwrap());
        let changed = pdf(&cms, false);
        assert!(super::fields(&load(&changed).unwrap()).unwrap().is_empty());
        assert!(!is_bound_to_revision(&changed, &value).unwrap_or(false));
    }

    #[test]
    fn refuses_attached_payload_bad_ranges_and_nonzero_padding() {
        let mut padded = fake_cms(false);
        padded.push(1);
        for cms in [fake_cms(true), padded] {
            let bytes = pdf(&cms, true);
            let doc = load(&bytes).unwrap();
            assert!(envelope(&fields(&doc).unwrap()[0], &bytes).is_err());
        }
        let bytes = pdf(&fake_cms(false), true);
        let doc = load(&bytes).unwrap();
        let field = fields(&doc).unwrap().pop().unwrap();
        let signature = envelope(&field, &bytes).unwrap();
        let mut replaced = bytes.clone();
        replaced[signature.left.end] = b'[';
        assert!(envelope(&field, &replaced).is_err());
        replaced = bytes;
        replaced.pop();
        assert!(envelope(&field, &replaced).is_err());
    }
}
