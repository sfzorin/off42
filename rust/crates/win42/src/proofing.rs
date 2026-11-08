//! Native proofing ranges use UTF-16 positions, including Rich Edit table/object delimiters.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(crate) use windows::{raw_text, show};
fn boundary(text: &[u16], at: usize) -> bool {
    at <= text.len()
        && !(at > 0
            && at < text.len()
            && (0xD800..=0xDBFF).contains(&text[at - 1])
            && (0xDC00..=0xDFFF).contains(&text[at]))
}
fn checked_word(text: &[u16], start: usize, length: usize) -> Result<String, String> {
    let end = start
        .checked_add(length)
        .filter(|end| *end <= text.len())
        .ok_or("Invalid spelling range")?;
    if length == 0 || !boundary(text, start) || !boundary(text, end) {
        return Err("Invalid spelling character boundary".into());
    }
    let word = String::from_utf16(&text[start..end]).map_err(|_| "Invalid spelling text")?;
    if word
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{fff9}'..='\u{fffc}' | '\u{fdd0}'..='\u{fdef}'))
    {
        return Err("Spelling range crosses a document structure".into());
    }
    Ok(word)
}
fn check_text(text: &[u16]) -> Vec<u16> {
    text.iter()
        .map(|&c| {
            if c < 32 && !matches!(c, 9 | 10 | 13)
                || (0xFFF9..=0xFFFC).contains(&c)
                || (0xFDD0..=0xFDEF).contains(&c)
            {
                32
            } else {
                c
            }
        })
        .collect()
}
fn chunk_end(text: &[u16], start: usize, end: usize) -> usize {
    let mut stop = end.min(start.saturating_add(32768));
    if stop < end {
        if let Some(i) = (start + 16384..stop).rev().find(|&i| text[i] <= 32) {
            stop = i + 1;
        }
    }
    if !boundary(text, stop) {
        stop -= 1
    }
    stop
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utf16_ranges_preserve_emoji_tables_objects_and_refuse_structural_or_split_words() {
        let text: Vec<_> = "😀\r\u{7}helo\u{fffc}\rworld".encode_utf16().collect();
        assert_eq!(checked_word(&text, 4, 4).unwrap(), "helo");
        assert!(checked_word(&text, 1, 1).is_err());
        assert!(checked_word(&text, 3, 5).is_err());
        assert!(checked_word(&text, 4, 5).is_err());
        assert!(checked_word(&text, usize::MAX, 2).is_err());
        let safe = check_text(&text);
        assert_eq!(safe.len(), text.len());
        assert_eq!(&safe[4..8], &text[4..8]);
        assert_eq!(safe[8], 32);
        let long: Vec<_> = ("x ".repeat(20000) + "😀").encode_utf16().collect();
        let stop = chunk_end(&long, 0, long.len());
        assert!(stop <= 32768 && boundary(&long, stop));
        assert_eq!(long[stop - 1], 32);
    }
}
