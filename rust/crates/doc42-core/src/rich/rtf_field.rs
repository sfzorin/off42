//! Hyperlink field instruction parsing; other field types are not reduced to text.
use super::*;
pub(super) enum Parsed {
    Hyperlink(String),
    PageNumber,
    PageCount,
}
pub(super) fn parse(bytes: &[u8]) -> Result<Parsed, Error> {
    let text = std::str::from_utf8(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
    let upper = text.to_ascii_uppercase();
    if upper.contains("HYPERLINK") {
        return target(bytes).map(Parsed::Hyperlink);
    }
    let inst = upper
        .find("\\FLDINST")
        .ok_or_else(|| Error::Unsupported("RTF field instruction".into()))?;
    let tail = upper[inst + 8..].trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '{');
    let word = tail
        .split(|c: char| !c.is_ascii_alphabetic())
        .next()
        .unwrap_or("");
    match word {
        "PAGE" => Ok(Parsed::PageNumber),
        "NUMPAGES" => Ok(Parsed::PageCount),
        _ => Err(Error::Unsupported("RTF non-page field".into())),
    }
}
pub(super) fn target(bytes: &[u8]) -> Result<String, Error> {
    let input = std::str::from_utf8(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
    let upper = input.to_ascii_uppercase();
    let start = upper
        .find("HYPERLINK")
        .ok_or_else(|| Error::Unsupported("RTF non-hyperlink field".into()))?
        + "HYPERLINK".len();
    let tail = &input[start..];
    let first = tail
        .find('"')
        .ok_or_else(|| Error::Unsupported("RTF hyperlink target".into()))?;
    let prefix = tail[..first].trim();
    let internal = matches!(prefix, "\\l" | "\\\\l");
    if !prefix.is_empty() && !internal {
        return Err(Error::Unsupported("RTF hyperlink switches".into()));
    }
    let after = &tail[first + 1..];
    let end = after
        .find('"')
        .ok_or_else(|| Error::Invalid("unterminated RTF hyperlink".into()))?;
    let url = &after[..end];
    if url.is_empty()
        || !url.is_ascii()
        || url
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | '{' | '}'))
    {
        return Err(Error::Unsupported("RTF hyperlink target".into()));
    }
    Ok(if internal {
        format!("#{url}")
    } else {
        url.to_owned()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn internal_hyperlink_switch_is_retained() {
        assert_eq!(
            target(br#"{\*\fldinst HYPERLINK \\l "Target"}"#).unwrap(),
            "#Target"
        );
    }
    #[test]
    fn hyperlink_field_survives_edit_projection() {
        let mut d = RichDocument::new();
        d.transact(|s| {
            let Block::Paragraph(p) = &mut s.blocks[0] else {
                panic!()
            };
            p.runs[0].text = "link".into();
            p.runs[0].link = Some("https://example.com/a".into());
            Ok(())
        })
        .unwrap();
        let rtf = d.rich_edit_projection().unwrap();
        let reopened = RichDocument::decode(&rtf, Format::Rtf).unwrap();
        let Block::Paragraph(p) = &reopened.state().blocks[0] else {
            panic!()
        };
        assert_eq!(p.runs[0].link.as_deref(), Some("https://example.com/a"));
        d.apply_rich_edit_projection(&rtf, (0, 0)).unwrap();
    }
}
