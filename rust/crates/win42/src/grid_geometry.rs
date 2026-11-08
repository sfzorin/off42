//! Pure 64-bit geometry for the native variable-size grid.
use crate::{GridAxis, GridRange};
fn pixel(value: i64) -> i32 {
    value.clamp(-1_000_000_000, 1_000_000_000) as i32
}
#[cfg(test)]
fn offsets(lengths: &[i32]) -> GridAxis {
    GridAxis::new(
        lengths.len(),
        0,
        lengths
            .iter()
            .enumerate()
            .map(|(i, v)| (i, (*v).clamp(0, 65535) as u16))
            .collect(),
    )
    .unwrap()
}
pub(crate) fn index_at(axis: &GridAxis, value: i64) -> Option<usize> {
    axis.index_at(value)
}
pub(crate) fn position(
    axis: &GridAxis,
    index: usize,
    frozen: usize,
    scroll: i64,
    header: i32,
) -> i32 {
    pixel(i64::from(header) + axis.offset(index) - if index < frozen { 0 } else { scroll })
}
pub(crate) fn index_at_view(
    axis: &GridAxis,
    value: i32,
    frozen: usize,
    scroll: i64,
    header: i32,
) -> Option<usize> {
    let relative = i64::from(value) - i64::from(header);
    if relative < 0 {
        return None;
    }
    let boundary = axis.offset(frozen.min(axis.len()));
    let index = axis.index_at(relative + if relative < boundary { 0 } else { scroll })?;
    if relative < boundary && index >= frozen || relative >= boundary && index < frozen {
        None
    } else {
        Some(index)
    }
}
pub(crate) fn visible(
    axis: &GridAxis,
    frozen: usize,
    scroll: i64,
    header: i32,
    client: i32,
) -> Vec<usize> {
    let extent = (i64::from(client) - i64::from(header)).max(0);
    let boundary = axis.offset(frozen.min(axis.len()));
    axis.intersecting(0, boundary.min(extent))
        .filter(|i| *i < frozen)
        .chain(
            axis.intersecting(boundary + scroll, extent + scroll)
                .filter(|i| *i >= frozen),
        )
        .collect()
}
/// The OS scrollbar remains 32-bit; the worksheet origin never loses its exact 64-bit coordinate.
pub(crate) struct ScrollRange {
    pub total: i64,
    pub page: i64,
}
impl ScrollRange {
    pub fn maximum(&self) -> i64 {
        (self.total - self.page).max(0)
    }
    pub fn native_total(&self) -> i32 {
        self.total.clamp(1, i64::from(i32::MAX) - 1) as i32
    }
    pub fn native_page(&self) -> u32 {
        (i128::from(self.page.max(1)) * i128::from(self.native_total())
            / i128::from(self.total.max(1)))
        .clamp(1, i128::from(self.native_total())) as u32
    }
    pub fn native_position(&self, pos: i64) -> i32 {
        let n = i64::from(self.native_total()) - i64::from(self.native_page());
        if self.maximum() == 0 {
            0
        } else {
            (i128::from(pos.clamp(0, self.maximum())) * i128::from(n) / i128::from(self.maximum()))
                as i32
        }
    }
    pub fn logical_position(&self, pos: i32) -> i64 {
        let n = i64::from(self.native_total()) - i64::from(self.native_page());
        if n <= 0 {
            0
        } else {
            (i128::from(pos.max(0)) * i128::from(self.maximum()) / i128::from(n))
                .clamp(0, i128::from(self.maximum())) as i64
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Fragment {
    pub rect: (i32, i32, i32, i32),
    pub clip: (i32, i32, i32, i32),
    pub visible: (i32, i32, i32, i32),
}
pub(crate) fn pane_fragment(
    range: GridRange,
    xs: &GridAxis,
    ys: &GridAxis,
    frozen: (usize, usize),
    scroll: (i64, i64),
    header: (i32, i32),
    client: (i32, i32),
    pane: (bool, bool),
) -> Option<Fragment> {
    if range.top > range.bottom
        || range.left > range.right
        || range.bottom >= ys.len()
        || range.right >= xs.len()
    {
        return None;
    }
    let (fr, fc) = (frozen.0.min(ys.len()), frozen.1.min(xs.len()));
    if (pane.0 && (fr == 0 || range.top >= fr))
        || (!pane.0 && range.bottom < fr)
        || (pane.1 && (fc == 0 || range.left >= fc))
        || (!pane.1 && range.right < fc)
    {
        return None;
    }
    let split_x = position(xs, fc, usize::MAX, 0, header.0);
    let split_y = position(ys, fr, usize::MAX, 0, header.1);
    let clip = (
        if pane.1 { header.0 } else { split_x },
        if pane.0 { header.1 } else { split_y },
        if pane.1 {
            split_x.min(client.0)
        } else {
            client.0
        },
        if pane.0 {
            split_y.min(client.1)
        } else {
            client.1
        },
    );
    let rect = (
        pixel(i64::from(header.0) + xs.offset(range.left) - if pane.1 { 0 } else { scroll.0 }),
        pixel(i64::from(header.1) + ys.offset(range.top) - if pane.0 { 0 } else { scroll.1 }),
        pixel(i64::from(header.0) + xs.offset(range.right + 1) - if pane.1 { 0 } else { scroll.0 }),
        pixel(
            i64::from(header.1) + ys.offset(range.bottom + 1) - if pane.0 { 0 } else { scroll.1 },
        ),
    );
    let visible = (
        rect.0.max(clip.0),
        rect.1.max(clip.1),
        rect.2.min(clip.2),
        rect.3.min(clip.3),
    );
    if visible.0 >= visible.2 || visible.1 >= visible.3 {
        None
    } else {
        Some(Fragment {
            rect,
            clip,
            visible,
        })
    }
}
#[cfg(test)]
pub(crate) fn bounds(
    row: usize,
    col: usize,
    merge: Option<GridRange>,
    xs: &GridAxis,
    ys: &GridAxis,
    scroll_x: i64,
    scroll_y: i64,
    header_x: i32,
    header_y: i32,
) -> Option<(i32, i32, i32, i32)> {
    let m = merge.unwrap_or(GridRange {
        top: row,
        left: col,
        bottom: row,
        right: col,
    });
    if m.top > m.bottom || m.left > m.right || m.bottom >= ys.len() || m.right >= xs.len() {
        return None;
    }
    Some((
        pixel(i64::from(header_x) + xs.offset(m.left) - scroll_x),
        pixel(i64::from(header_y) + ys.offset(m.top) - scroll_y),
        pixel(i64::from(header_x) + xs.offset(m.right + 1) - scroll_x),
        pixel(i64::from(header_y) + ys.offset(m.bottom + 1) - scroll_y),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn far_scrolled_cells_are_visible_hittable_and_scroll_thumb_reaches_the_end() {
        let rows = GridAxis::new(1_048_576, 65535, Default::default()).unwrap();
        let cols = GridAxis::new(16_384, 65535, Default::default()).unwrap();
        let y = rows.offset(1_048_575);
        let x = cols.offset(16_383);
        assert_eq!(visible(&rows, 0, y, 25, 725), [1_048_575]);
        assert_eq!(index_at_view(&rows, 26, 0, y, 25), Some(1_048_575));
        assert_eq!(position(&rows, 1_048_575, 0, y, 25), 25);
        let fragment = pane_fragment(
            GridRange {
                top: 1_048_575,
                bottom: 1_048_575,
                left: 16_383,
                right: 16_383,
            },
            &cols,
            &rows,
            (0, 0),
            (x, y),
            (48, 25),
            (1200, 725),
            (false, false),
        )
        .unwrap();
        assert_eq!(fragment.visible, (48, 25, 1200, 725));
        let scroll = ScrollRange {
            total: rows.total(),
            page: 700,
        };
        let end = scroll.native_total() - scroll.native_page() as i32;
        assert_eq!(scroll.native_position(scroll.maximum()), end);
        assert_eq!(scroll.logical_position(end), scroll.maximum());
        assert_eq!(scroll.logical_position(0), 0);
        assert!(scroll.logical_position(end / 2) > i64::from(i32::MAX));
        let ordinary = ScrollRange {
            total: 20000,
            page: 700,
        };
        assert_eq!(ordinary.native_position(12345), 12345);
        assert_eq!(ordinary.logical_position(12345), 12345);
        assert_eq!(ordinary.native_page(), 700);
        let rows = GridAxis::new(1_048_576, 20, Default::default()).unwrap();
        let y = rows.offset(1_048_570);
        assert_eq!(visible(&rows, 2, y, 25, 105), [0, 1, 1_048_572, 1_048_573]);
    }
    #[test]
    fn merged_cell_spanning_both_freezes_has_four_scrolled_fragments() {
        let xs = offsets(&[50, 50, 50]);
        let ys = offsets(&[20, 20, 20]);
        let range = GridRange {
            top: 0,
            left: 0,
            bottom: 2,
            right: 2,
        };
        let get = |pane| {
            pane_fragment(
                range,
                &xs,
                &ys,
                (1, 1),
                (20, 10),
                (48, 25),
                (200, 100),
                pane,
            )
            .unwrap()
        };
        assert_eq!(get((true, true)).visible, (48, 25, 98, 45));
        assert_eq!(get((true, false)).visible, (98, 25, 178, 45));
        assert_eq!(get((false, true)).visible, (48, 45, 98, 75));
        assert_eq!(get((false, false)).visible, (98, 45, 178, 75));
        assert_eq!(get((false, false)).rect, (28, 15, 178, 75));
        assert!(
            pane_fragment(
                range,
                &xs,
                &ys,
                (1, 1),
                (200, 100),
                (48, 25),
                (200, 100),
                (false, false)
            )
            .is_none()
        );
        assert_eq!(
            pane_fragment(
                range,
                &xs,
                &ys,
                (1, 1),
                (200, 100),
                (48, 25),
                (200, 100),
                (true, true)
            )
            .unwrap()
            .visible,
            (48, 25, 98, 45)
        );
        assert_eq!(index_at_view(&xs, 110, 1, 20, 48), Some(1));
        assert_eq!(index_at_view(&ys, 50, 1, 10, 25), Some(1));
    }
    #[test]
    fn frozen_panes_and_zero_width_cells_map_hits_correctly() {
        let x = offsets(&[40, 0, 60, 50]);
        assert_eq!(
            (0..=x.len()).map(|i| x.offset(i)).collect::<Vec<_>>(),
            [0, 40, 40, 100, 150]
        );
        assert_eq!(position(&x, 0, 2, 30, 48), 48);
        assert_eq!(position(&x, 2, 2, 30, 48), 58);
        assert_eq!(index_at_view(&x, 60, 2, 30, 48), Some(0));
        assert_eq!(index_at_view(&x, 90, 2, 30, 48), Some(2));
        assert_eq!(index_at_view(&x, 158, 2, 30, 48), Some(3));
    }
    #[test]
    fn variable_offsets_hit_and_scrolled_merged_bounds() {
        let x = offsets(&[50, 120, 10]);
        let y = offsets(&[20, 60, 5]);
        assert_eq!(
            (0..=x.len()).map(|i| x.offset(i)).collect::<Vec<_>>(),
            [0, 50, 170, 180]
        );
        assert_eq!(
            (0..=y.len()).map(|i| y.offset(i)).collect::<Vec<_>>(),
            [0, 20, 80, 85]
        );
        assert_eq!(index_at(&x, 49), Some(0));
        assert_eq!(index_at(&x, 50), Some(1));
        assert_eq!(index_at(&y, 79), Some(1));
        assert_eq!(index_at(&y, 85), None);
        let merge = GridRange {
            top: 0,
            left: 1,
            bottom: 1,
            right: 2,
        };
        assert_eq!(
            bounds(0, 1, Some(merge), &x, &y, 15, 10, 48, 25),
            Some((83, 15, 213, 95))
        );
        assert!(
            bounds(
                0,
                0,
                Some(GridRange {
                    top: 0,
                    left: 0,
                    bottom: 99,
                    right: 0
                }),
                &x,
                &y,
                0,
                0,
                48,
                25
            )
            .is_none()
        );
    }
}
