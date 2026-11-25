//! RTF PNG/JPEG picture destination, using explicit physical dimensions.
use super::*;
/// Picture groups may contain a BLIP UID group and length-prefixed binary bytes.
pub(super) fn group_end(bytes: &[u8], start: usize) -> Result<usize, Error> {
    let mut depth = 0usize;
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                if depth > 128 {
                    return Err(Error::Limit);
                }
            }
            b'}' => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| Error::Invalid("picture group".into()))?;
                if depth == 0 {
                    return Ok(i + 1);
                }
            }
            b'\\' if i + 1 < bytes.len() => {
                i += 1;
                if bytes[i].is_ascii_alphabetic() {
                    let word = i;
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let binary = &bytes[word..i] == b"bin";
                    let number = i;
                    if i < bytes.len() && bytes[i] == b'-' {
                        i += 1;
                    }
                    while i < bytes.len() && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                    let end = i;
                    if i < bytes.len() && bytes[i] == b' ' {
                        i += 1;
                    }
                    if binary {
                        let length = std::str::from_utf8(&bytes[number..end])
                            .ok()
                            .and_then(|v| v.parse::<usize>().ok())
                            .ok_or_else(|| Error::Invalid("picture binary length".into()))?;
                        i = i
                            .checked_add(length)
                            .filter(|end| *end <= bytes.len())
                            .ok_or_else(|| {
                                Error::Invalid("truncated picture binary data".into())
                            })?;
                    }
                    continue;
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err(Error::Invalid("unterminated picture".into()))
}
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
    parse_inner(bytes, id, false)
}
pub(super) fn parse_projection(bytes: &[u8], id: u64) -> Result<Image, Error> {
    parse_inner(bytes, id, true)
}
fn projection_properties(bytes: &[u8]) -> Result<(), Error> {
    let mut properties = bytes
        .strip_prefix(br"{\*\picprop")
        .and_then(|v| v.strip_suffix(b"}"))
        .ok_or_else(|| Error::Unsupported("RTF picture properties".into()))?
        .trim_ascii();
    while !properties.is_empty() {
        let end = group_end(properties, 0)?;
        let property = properties[..end]
            .strip_prefix(br"{\sp")
            .and_then(|v| v.strip_suffix(b"}"))
            .ok_or_else(|| Error::Unsupported("RTF picture property".into()))?
            .trim_ascii();
        let name_end = group_end(property, 0)?;
        let name = property[..name_end]
            .strip_prefix(br"{\sn ")
            .and_then(|v| v.strip_suffix(b"}"))
            .ok_or_else(|| Error::Unsupported("RTF picture property name".into()))?;
        let value = property[name_end..].trim_ascii();
        let value_end = group_end(value, 0)?;
        if !value[value_end..].trim_ascii().is_empty() {
            return Err(Error::Unsupported("RTF picture property tail".into()));
        }
        let value = value[..value_end]
            .strip_prefix(br"{\sv ")
            .and_then(|v| v.strip_suffix(b"}"))
            .ok_or_else(|| Error::Unsupported("RTF picture property value".into()))?;
        // Native inline objects add a placeholder description and the default vertical anchor.
        if name != b"wzDescription" && !(name == b"posv" && value.trim_ascii() == b"1") {
            return Err(Error::Unsupported("RTF picture placement property".into()));
        }
        properties = properties[end..].trim_ascii();
    }
    Ok(())
}
fn parse_inner(bytes: &[u8], id: u64, projection: bool) -> Result<Image, Error> {
    if !bytes.starts_with(b"{\\pict") || !bytes.ends_with(b"}") {
        return Err(Error::Invalid("invalid picture destination".into()));
    }
    let mut i = 6usize;
    let mut width = None;
    let mut height = None;
    let mut mime = None;
    let mut hex = Vec::new();
    let mut binary = None;
    while i + 1 < bytes.len() {
        let b = bytes[i];
        i += 1;
        if b.is_ascii_whitespace() {
            continue;
        }
        if b == b'{' {
            let start = i - 1;
            let end = group_end(bytes, start)?;
            let uid = bytes[start..end]
                .strip_prefix(br"{\*\blipuid ")
                .and_then(|v| v.strip_suffix(b"}"));
            if projection && bytes[start..end].starts_with(br"{\*\picprop") {
                projection_properties(&bytes[start..end])?;
            } else if !uid
                .is_some_and(|value| value.len() == 32 && value.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(Error::Unsupported("RTF picture metadata group".into()));
            }
            i = end;
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
                "bin" => {
                    let length = arg
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| Error::Invalid("picture binary length".into()))?;
                    let end = i
                        .checked_add(length)
                        .filter(|end| *end < bytes.len())
                        .ok_or_else(|| Error::Invalid("truncated picture binary data".into()))?;
                    if binary.is_some() || !hex.is_empty() {
                        return Err(Error::Unsupported("mixed RTF picture encodings".into()));
                    }
                    binary = Some(bytes[i..end].to_vec());
                    i = end;
                }
                _ => return Err(Error::Unsupported(format!("RTF picture control {word}"))),
            }
        } else if b.is_ascii_hexdigit() {
            if binary.is_some() {
                return Err(Error::Unsupported("mixed RTF picture encodings".into()));
            }
            hex.push(b)
        } else {
            return Err(Error::Unsupported("RTF picture data".into()));
        }
    }
    if hex.len() % 2 != 0 {
        return Err(Error::Invalid("odd picture hex".into()));
    }
    let mut data = binary.unwrap_or_else(|| Vec::with_capacity(hex.len() / 2));
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_blip_uid_does_not_truncate_or_join_picture_payload_with_following_text() {
        let bytes=br"{\rtf1\ansi{\pict\pngblip\picwgoal240\pichgoal240{\*\blipuid 0123456789abcdef0123456789abcdef}89504e470d0a1a0a0000}\par After\par}";
        let document = RichDocument::decode(bytes, Format::Rtf).unwrap();
        let [Block::Image(image), Block::Paragraph(paragraph)] = &document.state().blocks[..]
        else {
            panic!("picture and body order")
        };
        assert_eq!(image.data, b"\x89PNG\r\n\x1a\n\0\0");
        assert_eq!(paragraph.runs[0].text, "After");
    }
    #[test]
    fn binary_picture_braces_backslashes_and_zero_bytes_are_payload_and_truncation_is_rejected() {
        let data = b"\x89PNG\r\n\x1a\n{}\\\0";
        let mut group = format!(
            "{{\\pict\\pngblip\\picwgoal240\\pichgoal240\\bin{} ",
            data.len()
        )
        .into_bytes();
        group.extend_from_slice(data);
        group.push(b'}');
        let end = group_end(&group, 0).unwrap();
        assert_eq!(end, group.len());
        assert_eq!(parse(&group, 1).unwrap().data, data);
        for value in [
            br"{\pict\pngblip\picwgoal240\pichgoal240\bin999 abc}".as_slice(),
            br"{\pict\pngblip\picwgoal240\pichgoal240\bin-1 abc}".as_slice(),
        ] {
            assert!(group_end(value, 0).is_err());
            assert!(parse(value, 1).is_err());
        }
        let mut rtf = br"{\rtf1\ansi ".to_vec();
        rtf.extend_from_slice(&group);
        rtf.extend_from_slice(br"\par tail\par}");
        let document = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        assert!(matches!(
            &document.state().blocks[..],
            [Block::Image(_), Block::Paragraph(_)]
        ));
    }
}
