//! Pure geometry for the native variable-size grid.
use crate::GridRange;
pub(crate) fn offsets(lengths: &[i32]) -> Vec<i32> {
    let mut out = Vec::with_capacity(lengths.len() + 1);
    out.push(0i32);
    for v in lengths {
        out.push(
            out.last()
                .copied()
                .unwrap_or(0)
                .saturating_add((*v).clamp(0, 65535)),
        )
    }
    out
}
pub(crate) fn index_at(offsets: &[i32], value: i32) -> Option<usize> {
    if value < 0 || value >= *offsets.last()? {
        return None;
    }
    Some(offsets.partition_point(|v| *v <= value).saturating_sub(1))
}
pub(crate) fn position(
    offsets: &[i32],
    index: usize,
    frozen: usize,
    scroll: i32,
    header: i32,
) -> i32 {
    header + offsets[index] - if index < frozen { 0 } else { scroll }
}
pub(crate) fn index_at_view(
    offsets: &[i32],
    pixel: i32,
    frozen: usize,
    scroll: i32,
    header: i32,
) -> Option<usize> {
    let relative = pixel - header;
    if relative < 0 {
        return None;
    }
    let boundary = offsets[frozen.min(offsets.len().saturating_sub(1))];
    let index = index_at(
        offsets,
        if relative < boundary {
            relative
        } else {
            relative + scroll
        },
    )?;
    if relative < boundary && index >= frozen || relative >= boundary && index < frozen {
        None
    } else {
        Some(index)
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
    xs: &[i32],
    ys: &[i32],
    frozen: (usize, usize),
    scroll: (i32, i32),
    header: (i32, i32),
    client: (i32, i32),
    pane: (bool, bool),
) -> Option<Fragment> {
    if range.top > range.bottom
        || range.left > range.right
        || range.bottom.checked_add(1)? >= ys.len()
        || range.right.checked_add(1)? >= xs.len()
    {
        return None;
    }
    let (fr, fc) = (frozen.0.min(ys.len() - 1), frozen.1.min(xs.len() - 1));
    if (pane.0 && (fr == 0 || range.top >= fr))
        || (!pane.0 && range.bottom < fr)
        || (pane.1 && (fc == 0 || range.left >= fc))
        || (!pane.1 && range.right < fc)
    {
        return None;
    }
    let split_x = header.0 + xs[fc];
    let split_y = header.1 + ys[fr];
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
        header.0 + xs[range.left] - if pane.1 { 0 } else { scroll.0 },
        header.1 + ys[range.top] - if pane.0 { 0 } else { scroll.1 },
        header.0 + xs[range.right + 1] - if pane.1 { 0 } else { scroll.0 },
        header.1 + ys[range.bottom + 1] - if pane.0 { 0 } else { scroll.1 },
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
    xs: &[i32],
    ys: &[i32],
    scroll_x: i32,
    scroll_y: i32,
    header_x: i32,
    header_y: i32,
) -> Option<(i32, i32, i32, i32)> {
    let m = merge.unwrap_or(GridRange {
        top: row,
        left: col,
        bottom: row,
        right: col,
    });
    if m.top > m.bottom
        || m.left > m.right
        || m.bottom.checked_add(1)? >= ys.len()
        || m.right.checked_add(1)? >= xs.len()
    {
        return None;
    }
    Some((
        header_x + xs[m.left] - scroll_x,
        header_y + ys[m.top] - scroll_y,
        header_x + xs[m.right + 1] - scroll_x,
        header_y + ys[m.bottom + 1] - scroll_y,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
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
        assert!(pane_fragment(
            range,
            &xs,
            &ys,
            (1, 1),
            (200, 100),
            (48, 25),
            (200, 100),
            (false, false)
        )
        .is_none());
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
        assert_eq!(x, [0, 40, 40, 100, 150]);
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
        assert_eq!(x, [0, 50, 170, 180]);
        assert_eq!(y, [0, 20, 80, 85]);
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
        assert!(bounds(
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
        .is_none());
    }
}
