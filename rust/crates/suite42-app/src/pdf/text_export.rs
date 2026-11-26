//! Text exports use a committed PDF snapshot, never the original source bytes.
use doc42_core::rich::{Block, Format, Paragraph, RichDocument, Run, TextStyle};
use std::{io::Write, path::Path};

#[derive(Clone, Copy)]
pub enum Output {
    Text,
    Rtf,
    Html,
    Docx,
}
pub struct TextBuilder {
    blocks: Vec<Block>,
    current: Paragraph,
    bytes: usize,
    runs: usize,
    previous_cr: bool,
}
impl TextBuilder {
    pub fn new() -> Self {
        Self {
            blocks: vec![],
            current: Paragraph {
                id: 1,
                style: Default::default(),
                runs: vec![],
            },
            bytes: 0,
            runs: 0,
            previous_cr: false,
        }
    }
    fn paragraph(&mut self) -> Result<(), String> {
        if self.blocks.len() >= 99_999 {
            return Err("PDF text exceeds 100000 paragraphs".into());
        }
        let next = Paragraph {
            id: self.current.id + 1,
            style: Default::default(),
            runs: vec![],
        };
        self.blocks
            .push(Block::Paragraph(std::mem::replace(&mut self.current, next)));
        Ok(())
    }
    pub fn page_break(&mut self) -> Result<(), String> {
        self.paragraph()?;
        self.current.style.page_break_before = true;
        self.previous_cr = false;
        Ok(())
    }
    pub fn push(&mut self, character: char, style: TextStyle) -> Result<(), String> {
        self.bytes += character.len_utf8();
        if self.bytes > 4 * 1024 * 1024 {
            return Err("PDF text exceeds 4 MB; export fewer pages".into());
        }
        if character == '\n' && self.previous_cr {
            self.previous_cr = false;
            return Ok(());
        }
        self.previous_cr = character == '\r';
        if matches!(character, '\r' | '\n' | '\u{2028}' | '\u{2029}') {
            return self.paragraph();
        }
        if character == '\u{c}' {
            return self.page_break();
        }
        // Preserve an explicit indication of an unmapped glyph rather than silently deleting it.
        let character = if character == '\0' || (character.is_control() && character != '\t') {
            '\u{fffd}'
        } else {
            character
        };
        if let Some(last) = self
            .current
            .runs
            .last_mut()
            .filter(|run| run.style == style)
        {
            last.text.push(character);
        } else {
            self.runs += 1;
            if self.runs > 100_000 {
                return Err("PDF text exceeds 100000 style runs".into());
            }
            self.current.runs.push(Run {
                text: character.to_string(),
                style,
                link: None,
            });
        }
        Ok(())
    }
    pub fn finish(mut self) -> Result<RichDocument, String> {
        self.paragraph()?;
        let mut doc = RichDocument::new();
        doc.transact(|state| {
            state.blocks = self.blocks;
            state.next_id = state.blocks.len() as u64 + 1;
            Ok(())
        })
        .map_err(|e| e.to_string())?;
        Ok(doc)
    }
}
pub fn encode(doc: &RichDocument, output: Output) -> Result<Vec<u8>, String> {
    if matches!(output, Output::Text) {
        let mut text = String::new();
        for (index, block) in doc.state().blocks.iter().enumerate() {
            let Block::Paragraph(paragraph) = block else {
                return Err("Unexpected PDF text block".into());
            };
            if index > 0 {
                text.push(if paragraph.style.page_break_before {
                    '\u{c}'
                } else {
                    '\n'
                });
            }
            for run in &paragraph.runs {
                text.push_str(&run.text);
            }
        }
        return Ok(text.into_bytes());
    }
    doc.encode(match output {
        Output::Text => unreachable!(),
        Output::Rtf => Format::Rtf,
        Output::Html => Format::Html,
        Output::Docx => Format::Docx,
    })
    .map_err(|e| e.to_string())
}
pub fn publish(
    data: &[u8],
    destination: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    if cancel.load(Ordering::Relaxed) {
        return Err("PDF text export cancelled".into());
    }
    let folder = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::NamedTempFile::new_in(
        suite42_core::win32_path(folder).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    temporary
        .write_all(data)
        .and_then(|_| temporary.flush())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|e| e.to_string())?;
    if cancel.load(Ordering::Relaxed) {
        return Err("PDF text export cancelled".into());
    }
    temporary
        .persist_noclobber(suite42_core::win32_path(destination).map_err(|e| e.to_string())?)
        .map_err(|e| e.error.to_string())?;
    Ok(())
}

pub struct Worker {
    receiver: std::sync::mpsc::Receiver<Result<(), String>>,
    pub progress: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub total: usize,
    reported: usize,
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}
impl Worker {
    pub fn start(
        bytes: Vec<u8>,
        pages: Vec<usize>,
        destination: std::path::PathBuf,
        output: Output,
        engine: &'static dyn crate::platform::PdfText,
    ) -> Result<Self, String> {
        use std::sync::{
            atomic::{AtomicBool, AtomicUsize},
            mpsc, Arc,
        };
        if bytes.len() > 256 * 1024 * 1024 || pages.is_empty() || pages.len() > 1000 {
            return Err("PDF text export exceeds document limits".into());
        }
        let (tx, receiver) = mpsc::channel();
        let progress = Arc::new(AtomicUsize::new(0));
        let cancel = Arc::new(AtomicBool::new(false));
        let total = pages.len();
        let (worker_progress, worker_cancel) = (progress.clone(), cancel.clone());
        std::thread::Builder::new()
            .name("off42-pdf-text-export".into())
            .spawn(move || {
                let result = engine
                    .rich_text(&bytes, &pages, &worker_progress, &worker_cancel)
                    .and_then(|document| encode(&document, output))
                    .and_then(|data| publish(&data, &destination, &worker_cancel));
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            receiver,
            progress,
            cancel,
            total,
            reported: 0,
        })
    }
    pub fn poll(&self) -> Option<Result<(), String>> {
        match self.receiver.try_recv() {
            Ok(value) => Some(value),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(_) => Some(Err("PDF text export worker stopped".into())),
        }
    }
    pub fn progress_changed(&mut self) -> bool {
        let value = self.progress.load(std::sync::atomic::Ordering::Relaxed);
        let changed = self.reported != value;
        self.reported = value;
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn four_formats_preserve_unicode_styles_page_boundaries_and_escape_markup() {
        let mut builder = TextBuilder::new();
        let style = TextStyle {
            bold: true,
            italic: true,
            size_half_points: 36,
            color: Some(0x123456),
            ..Default::default()
        };
        for c in "Привет 😀 <script>&\r\nline".chars() {
            builder.push(c, style.clone()).unwrap();
        }
        builder.page_break().unwrap();
        builder.page_break().unwrap(); // Blank page remains a page boundary.
        for c in "日本語".chars() {
            builder.push(c, Default::default()).unwrap();
        }
        let document = builder.finish().unwrap();
        let plain = String::from_utf8(encode(&document, Output::Text).unwrap()).unwrap();
        assert_eq!(plain, "Привет 😀 <script>&\nline\u{c}\u{c}日本語");
        for (output, format) in [
            (Output::Rtf, Format::Rtf),
            (Output::Html, Format::Html),
            (Output::Docx, Format::Docx),
        ] {
            let data = encode(&document, output).unwrap();
            let reopened = RichDocument::decode(&data, format).unwrap();
            let first = match &reopened.state().blocks[0] {
                Block::Paragraph(p) => p,
                _ => panic!(),
            };
            assert_eq!(first.runs[0].text, "Привет 😀 <script>&");
            assert!(first.runs[0].style.bold && first.runs[0].style.italic);
            assert_eq!(first.runs[0].style.color, Some(0x123456));
            assert_eq!(
                reopened
                    .state()
                    .blocks
                    .iter()
                    .filter(|b| matches!(b, Block::Paragraph(p) if p.style.page_break_before))
                    .count(),
                2
            );
            if matches!(output, Output::Html) {
                assert!(!String::from_utf8(data).unwrap().contains("<script>"));
            }
        }
    }
    #[test]
    fn publication_refuses_existing_destination_and_cancel_without_leaving_output() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("text.txt");
        let cancel = AtomicBool::new(true);
        assert!(publish(b"private", &destination, &cancel).is_err());
        assert!(!destination.exists());
        cancel.store(false, std::sync::atomic::Ordering::Relaxed);
        publish(b"first", &destination, &cancel).unwrap();
        assert!(publish(b"replacement", &destination, &cancel).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), b"first");
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 1);
    }
}
