use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageLabelStyle {
    Decimal,
    UpperRoman,
    LowerRoman,
    UpperLetters,
    LowerLetters,
    PrefixOnly,
}
impl PageLabelStyle {
    fn key(self) -> Option<&'static str> {
        match self {
            Self::Decimal => Some("D"),
            Self::UpperRoman => Some("R"),
            Self::LowerRoman => Some("r"),
            Self::UpperLetters => Some("A"),
            Self::LowerLetters => Some("a"),
            Self::PrefixOnly => None,
        }
    }
}
fn number(value: i64, style: &[u8]) -> String {
    match style {
        b"R" | b"r" if (1..5000).contains(&value) => {
            let mut remaining = value;
            let mut text = String::new();
            for (amount, glyph) in [
                (1000, "M"),
                (900, "CM"),
                (500, "D"),
                (400, "CD"),
                (100, "C"),
                (90, "XC"),
                (50, "L"),
                (40, "XL"),
                (10, "X"),
                (9, "IX"),
                (5, "V"),
                (4, "IV"),
                (1, "I"),
            ] {
                while remaining >= amount {
                    text.push_str(glyph);
                    remaining -= amount;
                }
            }
            if style == b"r" {
                text.to_lowercase()
            } else {
                text
            }
        }
        b"A" | b"a" if value > 0 && (value - 1) / 26 < 4096 => {
            // PDF repeats the same letter: A…Z, AA, BB…ZZ, AAA…
            let initial = if style == b"a" { b'a' } else { b'A' };
            char::from(initial + ((value - 1) % 26) as u8)
                .to_string()
                .repeat(((value - 1) / 26 + 1) as usize)
        }
        b"" => String::new(),
        _ => value.to_string(),
    }
}
impl PdfDocument {
    /// Effective labels in current page order, bounded even for extreme source /St values.
    pub fn page_labels(&self) -> Result<Option<Vec<String>>, String> {
        let Some(rules) = self.page_label_rules()? else {
            return Ok(None);
        };
        self.history
            .current()
            .pages
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let Some(rule) = rules.get(page) else {
                    return Ok((index + 1).to_string());
                };
                let prefix = rule
                    .get(b"P")
                    .ok()
                    .map(lopdf::decode_text_string)
                    .transpose()
                    .map_err(|e| e.to_string())?
                    .unwrap_or_default();
                if prefix.len() > 32768 {
                    return Err("Page-label prefix exceeds display limit".into());
                }
                let style = rule
                    .get(b"S")
                    .ok()
                    .and_then(|o| o.as_name().ok())
                    .unwrap_or(b"");
                let value = rule
                    .get(b"St")
                    .ok()
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(1);
                Ok(format!("{prefix}{}", number(value, style)))
            })
            .collect::<Result<Vec<_>, String>>()
            .map(Some)
    }
    /// Update a finite inclusive range as one history transaction; preserve other effective labels.
    pub fn set_page_labels(
        &mut self,
        first: usize,
        last: usize,
        style: PageLabelStyle,
        prefix: &str,
        start: i64,
    ) -> Result<(), String> {
        if first > last
            || last >= self.page_count()
            || start < 1
            || start.checked_add((last - first) as i64).is_none()
            || prefix.len() > 512
            || prefix.chars().any(char::is_control)
        {
            return Err("Invalid page-label range, prefix or starting number".into());
        }
        let mut rules = self.page_label_rules()?.unwrap_or_default();
        let mut state = self.history.current().clone();
        for (index, &page) in state.pages.iter().enumerate() {
            rules
                .entry(page)
                .or_insert_with(|| lopdf::dictionary! {"S"=>"D", "St"=>(index+1) as i64});
        }
        for index in first..=last {
            let mut rule = lopdf::Dictionary::new();
            if let Some(key) = style.key() {
                rule.set("S", Object::Name(key.as_bytes().to_vec()));
                rule.set("St", start + (index - first) as i64);
            }
            if !prefix.is_empty() {
                rule.set("P", lopdf::text_string(prefix));
            }
            rules.insert(state.pages[index], rule);
        }
        state.page_labels = Some(rules);
        self.apply_edit(state)?;
        Ok(())
    }
    pub fn clear_page_labels(&mut self) {
        let mut state = self.history.current().clone();
        state.page_labels = Some(HashMap::new());
        let _ = self.apply_edit(state);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn label_numbering_uses_pdf_letter_rules_and_bounded_extreme_values() {
        assert_eq!(number(27, b"A"), "AA");
        assert_eq!(number(28, b"a"), "bb");
        assert_eq!(number(53, b"A"), "AAA");
        assert_eq!(number(49, b"R"), "XLIX");
        assert_eq!(number(4999, b"r"), "mmmmcmxcix");
        assert_eq!(number(5000, b"R"), "5000");
        assert_eq!(number(i64::MAX, b"a"), i64::MAX.to_string());
        assert_eq!(number(1, b""), "");
    }
}
