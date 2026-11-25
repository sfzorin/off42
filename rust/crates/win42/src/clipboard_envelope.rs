//! Bound typed clipboard data to the simultaneously published Unicode text.
const MAGIC: &[u8; 8] = b"S42CLP1\0";
const LIMIT: usize = 64 * 1024 * 1024;
pub(crate) fn encode(bytes: &[u8], text: &str) -> Result<Vec<u8>, &'static str> {
    let total = 24usize
        .checked_add(text.len())
        .and_then(|n| n.checked_add(bytes.len()))
        .ok_or("Clipboard data is too large")?;
    if bytes.is_empty() || total > LIMIT {
        return Err("Invalid clipboard data");
    }
    let mut result = Vec::with_capacity(total);
    result.extend_from_slice(MAGIC);
    result.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    result.extend_from_slice(&(text.len() as u64).to_le_bytes());
    result.extend_from_slice(text.as_bytes());
    result.extend_from_slice(bytes);
    Ok(result)
}
pub(crate) fn decode<'a>(data: &'a [u8], text: &str) -> Option<&'a [u8]> {
    if data.len() < 24 || data.len() > LIMIT || &data[..8] != MAGIC {
        return None;
    }
    let data_len = usize::try_from(u64::from_le_bytes(data[8..16].try_into().ok()?)).ok()?;
    let text_len = usize::try_from(u64::from_le_bytes(data[16..24].try_into().ok()?)).ok()?;
    if data_len == 0 {
        return None;
    }
    let start = 24usize.checked_add(text_len)?;
    let end = start.checked_add(data_len)?;
    if end > data.len() || data[24..start] != *text.as_bytes() {
        return None;
    }
    Some(&data[start..end])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn envelope_accepts_exact_text_and_rejects_stale_malformed_or_wrong_version() {
        let mut data = encode(b"typed payload", "a\tb\n").unwrap();
        assert_eq!(decode(&data, "a\tb\n"), Some(&b"typed payload"[..]));
        assert_eq!(decode(&data, "changed external clipboard"), None);
        let mut padded = data.clone();
        padded.extend_from_slice(&[0; 8]);
        assert_eq!(decode(&padded, "a\tb\n"), Some(&b"typed payload"[..]));
        data[6] = b'2';
        assert_eq!(decode(&data, "a\tb\n"), None);
        let mut malformed = encode(b"x", "text").unwrap();
        malformed[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
        assert_eq!(decode(&malformed, "text"), None);
        assert_eq!(decode(&[0; 23], "text"), None);
        assert!(encode(&[], "text").is_err());
        assert!(encode(&vec![0; LIMIT], "text").is_err());
    }
}
