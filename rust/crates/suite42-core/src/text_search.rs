use regex::{Captures, Regex, RegexBuilder};
use std::ops::Range;

const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_WORK: usize = 64 * 1024 * 1024;
const MAX_MATCHES: usize = 100_000;

#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Limit(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => f.write_str(message),
            Self::Limit(message) => f.write_str(message),
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    pub match_case: bool,
    pub whole_words: bool,
    pub regex: bool,
}

#[derive(Default)]
pub struct Budget {
    work: usize,
    matches: usize,
    output: usize,
}

pub struct Replacement {
    pub range: Range<usize>,
    pub text: String,
}

pub struct Search {
    expression: Regex,
    options: Options,
}

impl Search {
    pub fn new(query: &str, options: Options) -> Result<Option<Self>, Error> {
        if query.is_empty() {
            return Ok(None);
        }
        if query.len() > 4096 || query.contains('\0') {
            return Err(Error::Limit("Search text exceeds the supported limit"));
        }
        let pattern = if options.regex {
            query.to_owned()
        } else {
            regex::escape(query)
        };
        let expression = RegexBuilder::new(&pattern)
            .case_insensitive(!options.match_case)
            .size_limit(512 * 1024)
            .build()
            .map_err(|e| Error::Invalid(format!("Invalid regular expression: {e}")))?;
        Ok(Some(Self {
            expression,
            options,
        }))
    }

    fn scan(
        &self,
        text: &str,
        start: usize,
        budget: &mut Budget,
        mut visit: impl FnMut(&Captures<'_>) -> Result<bool, Error>,
    ) -> Result<(), Error> {
        let mut cursor = start;
        while cursor <= text.len() {
            if self.options.regex {
                // Bound the aggregate suffix length examined by repeated regex searches.
                budget.work = budget.work.saturating_add(text.len() - cursor);
                if budget.work > MAX_WORK {
                    return Err(Error::Limit(
                        "Regular expression search exceeds the supported work limit",
                    ));
                }
            }
            let Some(captures) = self.expression.captures_at(text, cursor) else {
                break;
            };
            let found = captures.get(0).expect("capture zero");
            let nonempty = found.start() != found.end();
            if nonempty && self.accepts(text, found.range()) {
                if !visit(&captures)? {
                    break;
                }
                cursor = found.end();
            } else {
                let Some(c) = text[found.start()..].chars().next() else {
                    break;
                };
                cursor = found.start() + c.len_utf8();
            }
        }
        Ok(())
    }

    fn accepts(&self, text: &str, range: Range<usize>) -> bool {
        if text[range.clone()]
            .chars()
            .any(|c| matches!(c, '\u{fff9}'..='\u{fffc}' | '\u{fdd0}'..='\u{fdef}'))
        {
            return false;
        }
        fn word(c: char) -> bool {
            c == '_' || c.is_alphanumeric()
        }
        !self.options.whole_words
            || (!text[..range.start].chars().next_back().is_some_and(word)
                && !text[range.end..].chars().next().is_some_and(word))
    }

    pub fn find(
        &self,
        text: &str,
        selection: (usize, usize),
        backwards: bool,
    ) -> Result<Option<(usize, usize)>, Error> {
        let (start, end) = selection;
        let mut budget = Budget::default();
        let mut result = None;
        if backwards {
            let before = byte_at_utf16(text, start);
            let mut wrapped = None;
            self.scan(text, 0, &mut budget, |captures| {
                let found = captures.get(0).expect("capture zero").range();
                if found.end <= before {
                    result = Some(found.clone());
                }
                wrapped = Some(found);
                Ok(true)
            })?;
            result = result.or(wrapped);
        } else {
            let after = byte_at_utf16(text, end);
            for offset in [after, 0] {
                self.scan(text, offset, &mut budget, |captures| {
                    result = Some(captures.get(0).expect("capture zero").range());
                    Ok(false)
                })?;
                if result.is_some() || offset == 0 {
                    break;
                }
            }
        }
        Ok(result.map(|range| {
            let start = text[..range.start].encode_utf16().count();
            (start, start + text[range].encode_utf16().count())
        }))
    }

    fn expand(&self, captures: &Captures<'_>, template: &str) -> Result<String, Error> {
        if !self.options.regex {
            return Ok(template.to_owned());
        }
        let dollars = template.bytes().filter(|&byte| byte == b'$').count();
        let whole = captures.get(0).expect("capture zero").len();
        if dollars.saturating_mul(whole).saturating_add(template.len()) > MAX_BYTES {
            return Err(Error::Limit(
                "Expanded replacement exceeds the supported limit",
            ));
        }
        let mut result = String::new();
        captures.expand(template, &mut result);
        Ok(result)
    }

    pub fn selected(
        &self,
        text: &str,
        selection: (usize, usize),
        template: &str,
    ) -> Result<Option<String>, Error> {
        let range = byte_at_utf16(text, selection.0)..byte_at_utf16(text, selection.1);
        if range.is_empty() {
            return Ok(None);
        }
        let Some(captures) = self.expression.captures_at(text, range.start) else {
            return Ok(None);
        };
        let found = captures.get(0).expect("capture zero").range();
        if found == range && self.accepts(text, range) {
            return self.expand(&captures, template).map(Some);
        }
        Ok(None)
    }

    pub fn replacements(
        &self,
        text: &str,
        template: &str,
        budget: &mut Budget,
    ) -> Result<Vec<Replacement>, Error> {
        let mut count = budget.matches;
        let mut output = budget.output;
        let mut edits = Vec::new();
        self.scan(text, 0, budget, |captures| {
            count += 1;
            if count > MAX_MATCHES {
                return Err(Error::Limit("Too many search matches"));
            }
            let text = self.expand(captures, template)?;
            output = output.saturating_add(text.len());
            if output > MAX_BYTES {
                return Err(Error::Limit("Replacement text exceeds the supported limit"));
            }
            edits.push(Replacement {
                range: captures.get(0).expect("capture zero").range(),
                text,
            });
            Ok(true)
        })?;
        budget.matches = count;
        budget.output = output;
        Ok(edits)
    }
}

fn byte_at_utf16(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += ch.len_utf16();
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_uses_utf16_wraps_and_skips_rich_editor_markers() {
        let search = Search::new("😀", Options::default()).unwrap().unwrap();
        let text = "😀 a\r\n😀 a";
        assert_eq!(search.find(text, (0, 0), false).unwrap(), Some((0, 2)));
        assert_eq!(search.find(text, (2, 2), false).unwrap(), Some((6, 8)));
        assert_eq!(search.find(text, (8, 8), false).unwrap(), Some((0, 2)));
        assert_eq!(search.find(text, (0, 2), true).unwrap(), Some((6, 8)));
        let search = Search::new("😀z\r\n", Options::default()).unwrap().unwrap();
        for caret in [3, 4] {
            assert_eq!(
                search.find("x😀z\r\nend", (caret, caret), false).unwrap(),
                Some((1, 6))
            );
        }
        let regex = Search::new(
            ".",
            Options {
                regex: true,
                ..Options::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            regex.find("\u{fff9}\u{fffc}x", (0, 0), false).unwrap(),
            Some((2, 3))
        );
    }

    #[test]
    fn whole_words_and_capture_replacements_use_surrounding_unicode_text() {
        let search = Search::new(
            r"кот-(\d+)",
            Options {
                regex: true,
                whole_words: true,
                match_case: false,
            },
        )
        .unwrap()
        .unwrap();
        let text = "xКОТ-11 Кот-22 _кот-33 кот-44";
        assert_eq!(search.selected(text, (1, 7), "$1").unwrap(), None);
        assert_eq!(
            search.selected(text, (8, 14), "ID-$1").unwrap(),
            Some("ID-22".into())
        );
        let edits = search
            .replacements(text, "ID-$1", &mut Budget::default())
            .unwrap();
        assert_eq!(
            edits
                .iter()
                .map(|edit| edit.text.as_str())
                .collect::<Vec<_>>(),
            ["ID-22", "ID-44"]
        );
        let literal = Search::new(
            "Кот",
            Options {
                match_case: true,
                ..Options::default()
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            literal.selected("Кот", (0, 3), "$1").unwrap(),
            Some("$1".into())
        );
        assert_eq!(literal.find("КОТ", (0, 0), false).unwrap(), None);
        assert!(Search::new(
            "[",
            Options {
                regex: true,
                ..Options::default()
            }
        )
        .is_err());
    }
}
