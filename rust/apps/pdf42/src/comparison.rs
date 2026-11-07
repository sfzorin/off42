//! Page-by-page word comparison with positioned source words and bounded linear-memory LCS.
use std::{
    collections::HashSet,
    ops::Range,
    sync::atomic::{AtomicBool, Ordering},
};
use unicode_segmentation::UnicodeSegmentation;
const MAX_TEXT: usize = 4 * 1024 * 1024;
const MAX_WORDS: usize = 100_000;
const MAX_WORK: usize = 16_000_000;
#[derive(Clone, Debug, Default)]
pub struct PageText {
    pub text: String,
    /// UTF-8 ranges after decoding PDFium Unicode values; empty boxes are omitted.
    pub glyphs: Vec<(Range<usize>, [f64; 4])>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub range: Range<usize>,
    pub boxes: Vec<[f64; 4]>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Change {
    pub page: usize,
    pub old_index: usize,
    pub removed: Vec<Word>,
    pub added: Vec<String>,
}
impl Change {
    pub fn removed_text(&self) -> String {
        self.removed
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }
    pub fn added_text(&self) -> String {
        self.added.join(" ")
    }
}
struct Budget<'a> {
    steps: usize,
    cancel: &'a AtomicBool,
}
impl Budget<'_> {
    fn charge(&mut self, n: usize) -> Result<(), String> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err("PDF comparison cancelled".into());
        }
        self.steps = self
            .steps
            .checked_add(n)
            .filter(|n| *n <= MAX_WORK)
            .ok_or("PDF comparison exceeds its work limit; compare smaller documents")?;
        Ok(())
    }
}
fn words(page: &PageText) -> Result<Vec<Word>, String> {
    if page.text.len() > MAX_TEXT || page.glyphs.len() > 1_000_000 {
        return Err("PDF comparison text exceeds its limit".into());
    }
    let mut out = Vec::new();
    let mut glyph = 0;
    let mut previous = 0;
    for (range, rect) in &page.glyphs {
        if range.start < previous
            || range.start > range.end
            || range.end > page.text.len()
            || !page.text.is_char_boundary(range.start)
            || !page.text.is_char_boundary(range.end)
            || rect.iter().any(|v| !v.is_finite())
            || rect[2] <= rect[0]
            || rect[3] <= rect[1]
        {
            return Err("Invalid comparison text geometry".into());
        }
        previous = range.start;
    }
    for (start, text) in page.text.unicode_word_indices() {
        if out.len() == MAX_WORDS {
            return Err("PDF comparison exceeds 100000 words per page".into());
        }
        let range = start..start + text.len();
        while glyph < page.glyphs.len() && page.glyphs[glyph].0.end <= start {
            glyph += 1;
        }
        let mut end = glyph;
        let mut boxes = Vec::new();
        while end < page.glyphs.len() && page.glyphs[end].0.start < range.end {
            boxes.push(page.glyphs[end].1);
            end += 1;
        }
        out.push(Word {
            text: text.into(),
            range,
            boxes,
        });
        glyph = end;
    }
    Ok(out)
}
fn row(
    a: &[Word],
    b: &[Word],
    reverse: bool,
    budget: &mut Budget<'_>,
) -> Result<Vec<usize>, String> {
    budget.charge(
        a.len()
            .checked_mul(b.len())
            .ok_or("PDF comparison work overflow")?,
    )?;
    let mut result = vec![0; b.len() + 1];
    for i in 0..a.len() {
        if i % 64 == 0 {
            budget.charge(0)?;
        }
        let av = &a[if reverse { a.len() - 1 - i } else { i }].text;
        let mut diagonal = 0;
        for j in 0..b.len() {
            let old = result[j + 1];
            let bv = &b[if reverse { b.len() - 1 - j } else { j }].text;
            result[j + 1] = if av == bv {
                diagonal + 1
            } else {
                result[j].max(old)
            };
            diagonal = old;
        }
    }
    Ok(result)
}
fn matches(
    a: &[Word],
    b: &[Word],
    mut ai: usize,
    mut bi: usize,
    budget: &mut Budget<'_>,
    out: &mut Vec<(usize, usize)>,
) -> Result<(), String> {
    budget.charge(a.len().saturating_add(b.len()))?;
    let mut prefix = 0;
    while prefix < a.len().min(b.len()) && a[prefix].text == b[prefix].text {
        out.push((ai + prefix, bi + prefix));
        prefix += 1;
    }
    let a = &a[prefix..];
    let b = &b[prefix..];
    ai += prefix;
    bi += prefix;
    let mut suffix = 0;
    while suffix < a.len().min(b.len())
        && a[a.len() - 1 - suffix].text == b[b.len() - 1 - suffix].text
    {
        suffix += 1;
    }
    let (middle_a, middle_b) = (&a[..a.len() - suffix], &b[..b.len() - suffix]);
    if !middle_a.is_empty() && !middle_b.is_empty() {
        if middle_a.len() == 1 {
            if let Some(j) = middle_b.iter().position(|w| w.text == middle_a[0].text) {
                out.push((ai, bi + j));
            }
        } else {
            let set = middle_a
                .iter()
                .map(|w| w.text.as_str())
                .collect::<HashSet<_>>();
            if middle_b.iter().any(|w| set.contains(w.text.as_str())) {
                let half = middle_a.len() / 2;
                let left = row(&middle_a[..half], middle_b, false, budget)?;
                let right = row(&middle_a[half..], middle_b, true, budget)?;
                let split = (0..=middle_b.len())
                    .max_by_key(|i| (left[*i] + right[middle_b.len() - *i], std::cmp::Reverse(*i)))
                    .unwrap();
                drop(left);
                drop(right);
                matches(&middle_a[..half], &middle_b[..split], ai, bi, budget, out)?;
                matches(
                    &middle_a[half..],
                    &middle_b[split..],
                    ai + half,
                    bi + split,
                    budget,
                    out,
                )?;
            }
        }
    }
    for i in 0..suffix {
        out.push((ai + a.len() - suffix + i, bi + b.len() - suffix + i));
    }
    Ok(())
}
pub fn compare(a: &[PageText], b: &[PageText], cancel: &AtomicBool) -> Result<Vec<Change>, String> {
    if a.len().max(b.len()) > 2000
        || a.iter()
            .chain(b)
            .map(|p| p.text.len())
            .try_fold(0usize, usize::checked_add)
            .filter(|n| *n <= MAX_TEXT * 2)
            .is_none()
    {
        return Err("PDF comparison document text exceeds its limit".into());
    }
    let mut budget = Budget { steps: 0, cancel };
    let mut changes = Vec::new();
    let empty = PageText::default();
    for page in 0..a.len().max(b.len()) {
        budget.charge(0)?;
        let old = words(a.get(page).unwrap_or(&empty))?;
        let new = words(b.get(page).unwrap_or(&empty))?;
        let mut common = Vec::new();
        matches(&old, &new, 0, 0, &mut budget, &mut common)?;
        common.push((old.len(), new.len()));
        let (mut ai, mut bi) = (0, 0);
        for (x, y) in common {
            if x > ai || y > bi {
                if changes.len() >= 10_000 {
                    return Err("PDF comparison exceeds 10000 change groups".into());
                }
                changes.push(Change {
                    page,
                    old_index: ai,
                    removed: old[ai..x].to_vec(),
                    added: new[bi..y].iter().map(|w| w.text.clone()).collect(),
                });
            }
            ai = x + 1;
            bi = y + 1;
        }
    }
    Ok(changes)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn page(text: &str) -> PageText {
        PageText {
            text: text.into(),
            glyphs: text
                .char_indices()
                .map(|(i, c)| (i..i + c.len_utf8(), [i as f64, 0.0, i as f64 + 1.0, 12.0]))
                .collect(),
        }
    }
    #[test]
    fn word_changes_keep_unicode_geometry_and_page_identity() {
        let old = [
            page("New board R1 uses 10k Ω. Привет мир"),
            page("remove this page"),
        ];
        let new = [page("New board R1 uses 20k Ω. Привет плате"), page("")];
        let changes = compare(&old, &new, &AtomicBool::new(false)).unwrap();
        assert_eq!(changes.len(), 3);
        assert_eq!(changes[0].removed_text(), "10k");
        assert_eq!(changes[0].added_text(), "20k");
        assert_eq!(changes[1].removed_text(), "мир");
        assert_eq!(changes[1].added_text(), "плате");
        assert_eq!(changes[2].page, 1);
        assert!(changes[2].added.is_empty());
        for c in &changes {
            for w in &c.removed {
                assert_eq!(&old[c.page].text[w.range.clone()], w.text);
                assert!(!w.boxes.is_empty());
            }
        }
        assert!(
            compare(&[page("A, B!")], &[page("A B")], &AtomicBool::new(false))
                .unwrap()
                .is_empty()
        );
    }
    #[test]
    fn inserted_pages_repetition_empty_text_and_atomic_limits() {
        let changes = compare(
            &[page("a b a c")],
            &[page("a a b c"), page("added page")],
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(changes.last().unwrap().page, 1);
        assert!(changes.last().unwrap().removed.is_empty());
        assert_eq!(changes.last().unwrap().added_text(), "added page");
        assert!(compare(&[], &[], &AtomicBool::new(false))
            .unwrap()
            .is_empty());
        assert!(compare(&[page("a")], &[page("b")], &AtomicBool::new(true)).is_err());
        let a = (0..5000)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        let b = (0..5000)
            .rev()
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(compare(&[page(&a)], &[page(&b)], &AtomicBool::new(false)).is_err());
    }
}

pub fn line_boxes(change: &Change) -> Vec<[f64; 4]> {
    let mut out: Vec<[f64; 4]> = Vec::new();
    for b in change.removed.iter().flat_map(|w| &w.boxes) {
        if let Some(last) = out.last_mut() {
            let overlap = last[3].min(b[3]) - last[1].max(b[1]);
            let height = (last[3] - last[1]).min(b[3] - b[1]);
            let gap = (b[0] - last[2]).max(last[0] - b[2]);
            if overlap >= height * 0.5 && gap < height * 2.0 {
                last[0] = last[0].min(b[0]);
                last[1] = last[1].min(b[1]);
                last[2] = last[2].max(b[2]);
                last[3] = last[3].max(b[3]);
                continue;
            }
        }
        out.push(*b);
    }
    out
}
#[cfg(windows)]
pub struct Session {
    pub changes: Vec<Change>,
    pub source: pdf42_core::SaveState,
    pub epoch: u64,
    pub tag: u64,
    pub name: String,
    pub keeping: bool,
    pub visible: bool,
}
#[cfg(windows)]
impl Session {
    pub fn report(&self) -> win42::ReportRequest {
        win42::ReportRequest {
            title: win42::localization::text("Comparison with “%@”").replace("%@", &self.name),
            summary: if self.changes.is_empty() {
                win42::localization::text("No differences in the text")
            } else {
                win42::localization::text("%d changes. Red: removed, orange: replaced. Words that were only added are listed here.").replace("%d",&self.changes.len().to_string())
            },
            columns: vec!["Page", "Removed", "Added"],
            column_colors: vec![None, Some([190, 30, 30]), Some([0, 130, 0])],
            rows: self
                .changes
                .iter()
                .map(|c| vec![(c.page + 1).to_string(), c.removed_text(), c.added_text()])
                .collect(),
            allow_keep: self.visible && self.changes.iter().any(|c| !c.removed.is_empty()),
            keeping: self.keeping,
        }
    }
    pub fn marks(&self) -> Vec<pdf42_core::ComparisonMark> {
        self.changes
            .iter()
            .filter(|c| !c.removed.is_empty())
            .map(|c| pdf42_core::ComparisonMark {
                page: c.page,
                replaced: !c.added.is_empty(),
                boxes: line_boxes(c),
            })
            .collect()
    }
}
#[cfg(windows)]
pub struct Worker {
    pub rx: std::sync::mpsc::Receiver<Result<Vec<Change>, String>>,
    pub cancel: std::sync::Arc<AtomicBool>,
    pub progress: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    pub shown: usize,
    pub source: pdf42_core::SaveState,
    pub epoch: u64,
    pub name: String,
    pub tag: u64,
}
#[cfg(windows)]
impl Worker {
    pub fn start(
        bytes: Vec<u8>,
        pages: usize,
        source: pdf42_core::SaveState,
        epoch: u64,
        tag: u64,
        path: &std::path::Path,
    ) -> Result<Self, String> {
        let path = path.to_path_buf();
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let (tx, rx) = std::sync::mpsc::channel();
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let token = cancel.clone();
        let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = progress.clone();
        std::thread::Builder::new()
            .name("PDF text comparison".into())
            .spawn(move || {
                let result = (|| {
                    if token.load(Ordering::Relaxed) {
                        return Err("PDF comparison cancelled".into());
                    }
                    let other = pdf42_core::PdfDocument::open(&path)?;
                    let (other_bytes, _) = other.save_snapshot()?;
                    let a = crate::text_export::pdfium::positioned(&bytes, pages, &count, &token)?;
                    let b = crate::text_export::pdfium::positioned(
                        &other_bytes,
                        other.page_count(),
                        &count,
                        &token,
                    )?;
                    let changes = compare(&a, &b, &token)?;
                    let boxes = changes.iter().map(|c| line_boxes(c).len()).sum::<usize>();
                    if boxes > 10_000 {
                        return Err("PDF comparison exceeds 10000 highlight regions".into());
                    }
                    Ok(changes)
                })();
                let _ = tx.send(result);
            })
            .map_err(|e| format!("Could not start PDF comparison: {e}"))?;
        Ok(Self {
            rx,
            cancel,
            progress,
            shown: 0,
            source,
            epoch,
            name,
            tag,
        })
    }
}

/// Multiply the overlay into a disposable rendered view; never modify document pixels/content.
pub fn paint_view(
    pixels: &mut [u8],
    width: u32,
    height: u32,
    bounds: [f64; 4],
    rotation: i64,
    page: usize,
    changes: &[Change],
) {
    if pixels.len() != width as usize * height as usize * 4
        || bounds[2] <= bounds[0]
        || bounds[3] <= bounds[1]
    {
        return;
    }
    let cv = |x: f64, y: f64| {
        let (x, y) = (
            (x - bounds[0]) / (bounds[2] - bounds[0]),
            (y - bounds[1]) / (bounds[3] - bounds[1]),
        );
        let (u, v) = match rotation {
            90 => (y, x),
            180 => (1.0 - x, y),
            270 => (1.0 - y, 1.0 - x),
            _ => (x, 1.0 - y),
        };
        (u * width as f64, v * height as f64)
    };
    for change in changes.iter().filter(|c| c.page == page) {
        let color = if change.added.is_empty() {
            [255u32, 0, 0]
        } else {
            [255u32, 128, 0]
        };
        for b in line_boxes(change) {
            let b = [
                b[0].max(bounds[0]),
                b[1].max(bounds[1]),
                b[2].min(bounds[2]),
                b[3].min(bounds[3]),
            ];
            if b[2] <= b[0] || b[3] <= b[1] {
                continue;
            }
            let (a, b) = (cv(b[0], b[1]), cv(b[2], b[3]));
            let (l, r, t, b) = (
                a.0.min(b.0).floor().clamp(0.0, width as f64) as usize,
                a.0.max(b.0).ceil().clamp(0.0, width as f64) as usize,
                a.1.min(b.1).floor().clamp(0.0, height as f64) as usize,
                a.1.max(b.1).ceil().clamp(0.0, height as f64) as usize,
            );
            for y in t..b {
                for x in l..r {
                    let i = (y * width as usize + x) * 4;
                    for c in 0..3 {
                        pixels[i + c] = ((pixels[i + c] as u32 * (153 * 255 + 102 * color[c])
                            + 32512)
                            / (255 * 255)) as u8;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod view_tests {
    use super::*;
    #[test]
    fn temporary_multiply_highlights_keep_black_text_and_follow_crop_rotation() {
        let changes = vec![Change {
            page: 0,
            old_index: 0,
            removed: vec![Word {
                text: "word".into(),
                range: 0..4,
                boxes: vec![[12.0, 22.0, 16.0, 26.0]],
            }],
            added: vec!["new".into()],
        }];
        for rotation in [0, 90, 180, 270] {
            let mut rgba = vec![255; 20 * 20 * 4];
            for px in rgba.chunks_exact_mut(4).take(20) {
                px[..3].fill(0);
            }
            let original = rgba.clone();
            paint_view(
                &mut rgba,
                20,
                20,
                [10.0, 20.0, 30.0, 40.0],
                rotation,
                0,
                &changes,
            );
            assert_eq!(&rgba[..80], &original[..80]);
            let colored = rgba
                .chunks_exact(4)
                .filter(|p| p[..3] == [255, 204, 153])
                .count();
            assert_eq!(colored, 16, "rotation{rotation}");
            let (left, top) = match rotation {
                0 => (2, 14),
                90 => (2, 2),
                180 => (14, 2),
                _ => (14, 14),
            };
            for y in 0..20 {
                for x in 0..20 {
                    let index = (y * 20 + x) * 4;
                    let expected = if (left..left + 4).contains(&x) && (top..top + 4).contains(&y) {
                        vec![255, 204, 153, 255]
                    } else {
                        original[index..index + 4].to_vec()
                    };
                    assert_eq!(
                        &rgba[index..index + 4],
                        expected,
                        "rotation{rotation} at{x},{y}"
                    );
                }
            }
        }
        let mut other_page = vec![255; 20 * 20 * 4];
        paint_view(
            &mut other_page,
            20,
            20,
            [10.0, 20.0, 30.0, 40.0],
            0,
            1,
            &changes,
        );
        assert!(other_page.iter().all(|p| *p == 255));
    }
}

#[cfg(test)]
mod mac_reference_tests {
    use super::*;
    #[test]
    fn actual_foundation_word_comparison_reference_and_reconstruction() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../fixtures/macos-comparison-cases.json")).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 10);
        for case in cases {
            let read = |key: &str| {
                case[key]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| PageText {
                        text: s.as_str().unwrap().into(),
                        glyphs: Vec::new(),
                    })
                    .collect::<Vec<_>>()
            };
            let (old, new) = (read("old"), read("new"));
            let tokens = |pages: &[PageText]| {
                pages
                    .iter()
                    .map(|p| {
                        words(p)
                            .unwrap()
                            .iter()
                            .map(|w| w.text.clone())
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>()
            };
            let (old_words, new_words) = (tokens(&old), tokens(&new));
            assert_eq!(
                serde_json::json!(old_words),
                case["old_words"],
                "{}",
                case["name"]
            );
            assert_eq!(
                serde_json::json!(new_words),
                case["new_words"],
                "{}",
                case["name"]
            );
            let changes = compare(&old, &new, &AtomicBool::new(false)).unwrap();
            let actual=changes.iter().map(|c|serde_json::json!({"page":c.page,"removed":c.removed.iter().map(|w|w.text.clone()).collect::<Vec<_>>(),"added":c.added})).collect::<Vec<_>>();
            assert_eq!(
                serde_json::json!(actual),
                case["changes"],
                "{}",
                case["name"]
            );
            for page in 0..old.len().max(new.len()) {
                let original = old_words.get(page).cloned().unwrap_or_default();
                let mut rebuilt = Vec::new();
                let mut cursor = 0;
                for change in changes.iter().filter(|c| c.page == page) {
                    assert!(change.old_index >= cursor);
                    rebuilt.extend_from_slice(&original[cursor..change.old_index]);
                    assert_eq!(
                        &original[change.old_index..change.old_index + change.removed.len()],
                        change
                            .removed
                            .iter()
                            .map(|w| w.text.clone())
                            .collect::<Vec<_>>()
                    );
                    rebuilt.extend(change.added.iter().cloned());
                    cursor = change.old_index + change.removed.len();
                }
                rebuilt.extend_from_slice(&original[cursor..]);
                assert_eq!(
                    rebuilt,
                    new_words.get(page).cloned().unwrap_or_default(),
                    "{} page{}",
                    case["name"],
                    page + 1
                );
            }
        }
    }
}
