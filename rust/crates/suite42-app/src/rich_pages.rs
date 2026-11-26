//! Page canvas geometry for paginated rich editors: page breaks, desk margins and the
//! current page. Pagination reuses the print paginator, so on-screen page numbers
//! and counts match Print Preview and printed output for the same geometry.
//!
//! Print Layout shows separate sheets. The Rich Edit control stays the only
//! editing engine: a desk window draws every sheet (body pages rendered with
//! EM_FORMATRANGE from the live editor, plus header/footer bands), and the live
//! editor is placed over the body area of the caret's sheet, clipped to exactly
//! that page's lines. Typing, selection, Undo/Redo and saving therefore keep
//! working on the one unmodified document; nothing is inserted to make the gaps.

/// Measured layout of the live editor, in editor character positions.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageLayout {
    /// First character of every page; the first entry is always 0.
    pub starts: Vec<i32>,
    /// Printed page count (notes can add pages beyond the live body breaks).
    pub count: usize,
}

impl PageLayout {
    /// One-based page holding `caret`, clamped to the printed count.
    pub fn page_of(&self, caret: usize) -> usize {
        let caret = i32::try_from(caret).unwrap_or(i32::MAX);
        let index = self.starts.partition_point(|start| *start <= caret).max(1);
        index.min(self.count.max(1))
    }
}

/// Status-bar fragment for the caret's page, or None before the first measurement.
pub fn status(layout: Option<&PageLayout>, caret: usize) -> Option<String> {
    let layout = layout.filter(|layout| layout.count > 0)?;
    Some(format!(
        "{} {} / {}",
        crate::localization::text("Page"),
        layout.page_of(caret),
        layout.count
    ))
}

/// Sheet positions of Print Layout in desk pixels (unscrolled).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sheets {
    pub count: usize,
    pub left: i32,
    pub width: i32,
    pub height: i32,
    pub gap: i32,
}

impl Sheets {
    /// Top edge of sheet `index`; sheets are stacked with a gap above each one.
    pub fn top(&self, index: usize) -> i32 {
        let step = i64::from(self.height) + i64::from(self.gap);
        (i64::from(self.gap) + step * index as i64).clamp(0, i64::from(i32::MAX)) as i32
    }

    /// Height of the whole desk including the gap below the last sheet.
    pub fn total(&self) -> i32 {
        self.top(self.count)
    }

    /// Sheet nearest to desk coordinate `y`; a gap is split between its two sheets.
    pub fn hit(&self, y: i32) -> usize {
        let step = (i64::from(self.height) + i64::from(self.gap)).max(1);
        let index = (i64::from(y) - i64::from(self.gap) / 2).max(0) / step;
        (index as usize).min(self.count.saturating_sub(1))
    }

    /// Sheets intersecting desk rows `[from, to)`.
    pub fn visible(&self, from: i32, to: i32) -> std::ops::Range<usize> {
        if self.count == 0 || to <= from {
            return 0..0;
        }
        let first = self.hit(from);
        let mut end = first;
        while end < self.count && self.top(end) < to {
            end += 1;
        }
        first..end.max(first + 1).min(self.count)
    }
}

/// Zero-based page whose range holds `caret`, given page start positions.
pub fn page_index(starts: &[i32], caret: usize) -> usize {
    let caret = i32::try_from(caret).unwrap_or(i32::MAX);
    starts.partition_point(|start| *start <= caret).saturating_sub(1)
}

/// Scroll position that keeps desk rows `[top, bottom)` inside a `view`-high viewport.
pub fn reveal(scroll: i32, view: i32, top: i32, bottom: i32) -> i32 {
    if top < scroll {
        top
    } else if bottom > scroll.saturating_add(view) {
        bottom.saturating_sub(view).min(top)
    } else {
        scroll
    }
}

/// Clamp a scroll position to the desk height.
pub fn clamp_scroll(scroll: i32, view: i32, total: i32) -> i32 {
    scroll.clamp(0, total.saturating_sub(view).max(0))
}

/// Accessible desk caption, e.g. "Print Layout: 2 / 5" (page being edited / sheets).
pub fn caption(page: usize, count: usize) -> String {
    format!("{}: {} / {}", crate::localization::text("Print Layout"), page + 1, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caret_maps_to_its_page_and_clamps_to_the_printed_count() {
        let layout = PageLayout { starts: vec![0, 120, 300], count: 4 };
        assert_eq!(layout.page_of(0), 1);
        assert_eq!(layout.page_of(119), 1);
        assert_eq!(layout.page_of(120), 2);
        assert_eq!(layout.page_of(5000), 3);
        let notes = PageLayout { starts: vec![0, 10, 20], count: 2 };
        assert_eq!(notes.page_of(25), 2);
        assert_eq!(status(None, 0), None);
        assert!(status(Some(&layout), 130).unwrap().ends_with(" 2 / 4"));
    }

    #[test]
    fn sheets_stack_with_gaps_and_map_rows_back_to_pages() {
        let sheets = Sheets { count: 3, left: 40, width: 816, height: 1056, gap: 16 };
        assert_eq!(sheets.top(0), 16);
        assert_eq!(sheets.top(1), 16 + 1056 + 16);
        assert_eq!(sheets.total(), 16 + 3 * (1056 + 16));
        assert_eq!(sheets.hit(0), 0);
        assert_eq!(sheets.hit(sheets.top(1) - 9), 0);
        assert_eq!(sheets.hit(sheets.top(1) - 8), 1);
        assert_eq!(sheets.hit(sheets.top(1)), 1);
        assert_eq!(sheets.hit(i32::MAX), 2);
        assert_eq!(sheets.visible(0, 500), 0..1);
        assert_eq!(sheets.visible(1000, 1200), 0..2);
        assert_eq!(sheets.visible(0, sheets.total()), 0..3);
        assert_eq!(sheets.visible(10, 10), 0..0);
        let empty = Sheets { count: 0, ..sheets };
        assert_eq!(empty.visible(0, 100), 0..0);
        assert_eq!(empty.hit(50), 0);
    }

    #[test]
    fn caret_page_reveal_and_scroll_limits() {
        let starts = [0, 120, 300];
        assert_eq!(page_index(&starts, 0), 0);
        assert_eq!(page_index(&starts, 119), 0);
        assert_eq!(page_index(&starts, 120), 1);
        assert_eq!(page_index(&starts, 9999), 2);
        assert_eq!(page_index(&[], 5), 0);
        assert_eq!(reveal(100, 400, 150, 170), 100);
        assert_eq!(reveal(100, 400, 40, 60), 40);
        assert_eq!(reveal(100, 400, 600, 640), 240);
        assert_eq!(reveal(100, 50, 600, 900), 600);
        assert_eq!(clamp_scroll(-5, 400, 3000), 0);
        assert_eq!(clamp_scroll(9000, 400, 3000), 2600);
        assert_eq!(clamp_scroll(10, 400, 300), 0);
        assert!(caption(1, 5).ends_with(": 2 / 5"));
    }
}
