//! RTF PNG/JPEG picture destination, using explicit physical dimensions.
use super::*;
pub(super) fn write(image: &Image) -> Result<String, Error> {
    use std::fmt::Write;
    if !image.alt.is_empty() {
        return Err(Error::Unsupported("RTF picture alt text".into()));
    }
    let kind = match image.mime.as_str() {
        "image/png" => "pngblip",
        "image/jpeg" => "jpegblip",
        _ => return Err(Error::Unsupported("RTF picture encoding".into())),
    };
    let mut out = format!(
        "{{\\pict\\{kind}\\picwgoal{}\\pichgoal{} ",
        image.width_twips, image.height_twips
    );
    for byte in &image.data {
        write!(out, "{byte:02x}").unwrap()
    }
    out.push('}');
    Ok(out)
}
pub(super) fn parse(bytes: &[u8], id: u64) -> Result<Image, Error> {
    if !bytes.starts_with(b"{\\pict") || !bytes.ends_with(b"}") {
        return Err(Error::Invalid("invalid picture destination".into()));
    }
    let mut i = 6usize;
    let mut width = None;
    let mut height = None;
    let mut mime = None;
    let mut hex = Vec::new();
    while i + 1 < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'\\' {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                i += 1
            }
            if i == start {
                return Err(Error::Unsupported("RTF picture control symbol".into()));
            }
            let word =
                std::str::from_utf8(&bytes[start..i]).map_err(|e| Error::Invalid(e.to_string()))?;
            let nstart = i;
            if i < bytes.len() && bytes[i] == b'-' {
                i += 1
            }
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1
            }
            let arg = if i > nstart {
                Some(
                    std::str::from_utf8(&bytes[nstart..i])
                        .map_err(|e| Error::Invalid(e.to_string()))?
                        .parse::<i64>()
                        .map_err(|e| Error::Invalid(e.to_string()))?,
                )
            } else {
                None
            };
            if i < bytes.len() && bytes[i] == b' ' {
                i += 1
            }
            match word {
                "pngblip" => mime = Some("image/png"),
                "jpegblip" => mime = Some("image/jpeg"),
                "picwgoal" => width = arg.and_then(|n| u32::try_from(n).ok()),
                "pichgoal" => height = arg.and_then(|n| u32::try_from(n).ok()),
                "picw" | "pich" | "bliptag" | "picscalex" | "picscaley" => {}
                _ => return Err(Error::Unsupported(format!("RTF picture control {word}"))),
            }
        } else if b.is_ascii_hexdigit() {
            hex.push(b)
        } else {
            return Err(Error::Unsupported("RTF picture data".into()));
        }
    }
    if hex.len() % 2 != 0 {
        return Err(Error::Invalid("odd picture hex".into()));
    }
    let mut data = Vec::with_capacity(hex.len() / 2);
    for pair in hex.chunks_exact(2) {
        let text = std::str::from_utf8(pair).map_err(|e| Error::Invalid(e.to_string()))?;
        data.push(u8::from_str_radix(text, 16).map_err(|e| Error::Invalid(e.to_string()))?)
    }
    let mime = mime.ok_or_else(|| Error::Unsupported("RTF picture encoding".into()))?;
    if (mime == "image/png" && !data.starts_with(b"\x89PNG\r\n\x1a\n"))
        || (mime == "image/jpeg" && !data.starts_with(&[0xff, 0xd8, 0xff]))
    {
        return Err(Error::Invalid("invalid picture bytes".into()));
    }
    Ok(Image {
        id,
        mime: mime.into(),
        data,
        width_twips: width.ok_or_else(|| Error::Invalid("picture width".into()))?,
        height_twips: height.ok_or_else(|| Error::Invalid("picture height".into()))?,
        alt: String::new(),
    })
}
