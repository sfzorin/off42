use win42::{CanvasScene, PrintGeometry, PrintPage, SceneFont, SceneItem, TextAlign, Viewport};
use xls42_core::{Range, Workbook};
fn contains(r: Range, row: usize, col: usize) -> bool {
    row >= r.top && row <= r.bottom && col >= r.left && col <= r.right
}
fn groups(indices: &[usize], merges: &[Range], rows: bool) -> Result<Vec<Vec<usize>>, String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < indices.len() {
        let start = indices[i];
        let mut end = start;
        loop {
            let next = merges
                .iter()
                .filter_map(|m| {
                    let (a, b) = if rows {
                        (m.top, m.bottom)
                    } else {
                        (m.left, m.right)
                    };
                    if a <= end && b >= start {
                        Some(b)
                    } else {
                        None
                    }
                })
                .max()
                .unwrap_or(end);
            if next == end {
                break;
            }
            end = next;
        }
        let mut group = Vec::new();
        while i < indices.len() && indices[i] <= end {
            group.push(indices[i]);
            i += 1
        }
        if group.len() != end - start + 1 {
            return Err("A filter or print area splits a merged range".into());
        }
        out.push(group);
    }
    Ok(out)
}
pub fn pages(book: &Workbook, visible: &[usize]) -> Result<Vec<PrintPage>, String> {
    let cfg = book.print_settings();
    let area = book.print_area();
    let geom = PrintGeometry {
        width_twips: cfg.paper_width_twips,
        height_twips: cfg.paper_height_twips,
        margin_left_twips: cfg.margin_left_twips,
        margin_top_twips: cfg.margin_top_twips,
        margin_right_twips: cfg.margin_right_twips,
        margin_bottom_twips: cfg.margin_bottom_twips,
    };
    if !geom.valid() {
        return Err("Invalid paper geometry".into());
    }
    let width = (geom.width_twips - geom.margin_left_twips - geom.margin_right_twips) as f64 / 20.0;
    let height =
        (geom.height_twips - geom.margin_top_twips - geom.margin_bottom_twips) as f64 / 20.0;
    let merges: Vec<_> = book
        .merges()
        .iter()
        .copied()
        .filter(|m| {
            m.top <= area.bottom
                && m.bottom >= area.top
                && m.left <= area.right
                && m.right >= area.left
        })
        .collect();
    if merges.iter().any(|m| {
        m.top < area.top || m.bottom > area.bottom || m.left < area.left || m.right > area.right
    }) {
        return Err("Print area splits a merged range".into());
    }
    let header_end = area
        .top
        .saturating_add(cfg.repeat_header_rows)
        .min(area.bottom + 1);
    if merges
        .iter()
        .any(|m| m.top < header_end && m.bottom >= header_end)
    {
        return Err("A merged range crosses the repeated header boundary".into());
    }
    let rows: Vec<_> = (area.top..=area.bottom)
        .filter(|r| {
            !book.sheet_view().hidden_rows.contains(r) && (*r < header_end || visible.contains(r))
        })
        .collect();
    let cols: Vec<_> = (area.left..=area.right)
        .filter(|c| !book.sheet_view().hidden_columns.contains(c))
        .collect();
    let cw = |c| book.column_width(c).unwrap_or(80) as f64 * 0.75;
    let rh = |r| book.row_height(r).unwrap_or(20) as f64 * 0.75;
    let total: f64 = cols.iter().map(|&c| cw(c)).sum();
    let scale = if cfg.fit_width {
        (width / total).min(1.0)
    } else {
        1.0
    };
    if !scale.is_finite() || scale <= 0.0 {
        return Err("Invalid print scale".into());
    }
    let col_groups = groups(&cols, &merges, false)?;
    let mut col_pages: Vec<Vec<usize>> = Vec::new();
    let mut current = Vec::new();
    let mut used = 0.0;
    for group in col_groups {
        let size: f64 = group.iter().map(|&c| cw(c) * scale).sum();
        if size > width + 0.01 {
            return Err("A merged column group is wider than the printable page".into());
        }
        if used + size > width + 0.01 && !current.is_empty() {
            col_pages.push(std::mem::take(&mut current));
            used = 0.0
        }
        current.extend(group);
        used += size;
    }
    if !current.is_empty() {
        col_pages.push(current)
    }
    let headers: Vec<_> = rows.iter().copied().filter(|r| *r < header_end).collect();
    let header_height: f64 = headers.iter().map(|&r| rh(r) * scale).sum();
    if header_height >= height {
        return Err("Repeated headers exceed printable page height".into());
    }
    let body: Vec<_> = rows.iter().copied().filter(|r| *r >= header_end).collect();
    let body_merges: Vec<_> = merges
        .iter()
        .copied()
        .filter(|m| m.top >= header_end)
        .collect();
    let row_groups = groups(&body, &body_merges, true)?;
    let mut row_pages: Vec<Vec<usize>> = Vec::new();
    let mut current = headers.clone();
    let mut used = header_height;
    let mut body_count = 0usize;
    for group in row_groups {
        if cfg
            .row_breaks
            .iter()
            .any(|b| *b > group[0] && *b <= *group.last().unwrap())
        {
            return Err("A page break splits a merged range".into());
        }
        let size: f64 = group.iter().map(|&r| rh(r) * scale).sum();
        if size > height - header_height + 0.01 {
            return Err("A merged row group is taller than the printable page".into());
        }
        if (used + size > height + 0.01 || cfg.row_breaks.contains(&group[0])) && body_count > 0 {
            row_pages.push(std::mem::replace(&mut current, headers.clone()));
            used = header_height;
            body_count = 0
        }
        body_count += group.len();
        current.extend(group);
        used += size
    }
    if !current.is_empty() {
        row_pages.push(current)
    }
    if row_pages.is_empty() || col_pages.is_empty() {
        return Err("Nothing to print".into());
    }
    if row_pages.len().saturating_mul(col_pages.len()) > 4096 {
        return Err("Print job exceeds page limit".into());
    }
    let grid = book.display_grid();
    let mut pages = Vec::new();
    for page_rows in &row_pages {
        for page_cols in &col_pages {
            let mut scene = CanvasScene {
                viewport: Viewport::default(),
                items: Vec::new(),
            };
            let mut y = 0.0;
            for &r in page_rows {
                let mut x = 0.0;
                for &c in page_cols {
                    let w = cw(c) * scale;
                    let h = rh(r) * scale;
                    let merged = merges.iter().find(|m| contains(**m, r, c));
                    if let Some(m) = merged {
                        if (r, c) != (m.top, m.left) {
                            x += w;
                            continue;
                        }
                    }
                    let (w, h) = if let Some(m) = merged {
                        (
                            (m.left..=m.right).map(|c| cw(c) * scale).sum(),
                            (m.top..=m.bottom).map(|r| rh(r) * scale).sum(),
                        )
                    } else {
                        (w, h)
                    };
                    let style = book.effective_style(r, c).ok_or("Invalid print cell")?;
                    let rgb = |c: u32| [(c >> 16) as u8, (c >> 8) as u8, c as u8];
                    scene.items.push(SceneItem::Rect {
                        x,
                        y,
                        width: w,
                        height: h,
                        fill: style.background.map(rgb).unwrap_or([255; 3]),
                        stroke: [200; 3],
                    });
                    let black = [0, 0, 0];
                    for (flag, a, b) in [
                        (1, (x, y), (x + w, y)),
                        (2, (x + w, y), (x + w, y + h)),
                        (4, (x, y + h), (x + w, y + h)),
                        (8, (x, y), (x, y + h)),
                    ] {
                        if style.borders & flag != 0 {
                            scene.items.push(SceneItem::Path {
                                points: vec![
                                    win42::Point { x: a.0, y: a.1 },
                                    win42::Point { x: b.0, y: b.1 },
                                ],
                                color: black,
                                closed: false,
                            });
                        }
                    }
                    scene.items.push(SceneItem::TextBox {
                        x: x + 2.0 * scale,
                        y: y + 1.0 * scale,
                        width: (w - 4.0 * scale).max(0.0),
                        height: (h - 2.0 * scale).max(0.0),
                        text: grid[r][c].clone(),
                        color: style.foreground.map(rgb).unwrap_or([0; 3]),
                        font: SceneFont {
                            family: if style.font.is_empty() {
                                "Arial".into()
                            } else {
                                style.font
                            },
                            points: if style.font_size == 0 {
                                10.0 * scale
                            } else {
                                style.font_size as f64 * scale
                            },
                            bold: style.bold,
                            italic: style.italic,
                            underline: style.underline,
                        },
                        align: if matches!(
                            book.content(r, c),
                            Some(
                                xls42_core::CellContent::Number(_)
                                    | xls42_core::CellContent::Date(_)
                            )
                        ) {
                            TextAlign::Right
                        } else {
                            TextAlign::Left
                        },
                        wrap: grid[r][c].contains('\n'),
                    });
                    x += cw(c) * scale;
                }
                y += rh(r) * scale;
            }
            pages.push(PrintPage {
                geometry: geom,
                scene,
            });
        }
    }
    Ok(pages)
}
#[cfg(test)]
mod tests {
    use super::*;
    use xls42_core::{CellStyle, SheetFilter};
    #[test]
    fn printed_pages_exclude_hidden_rows_and_columns() {
        let mut book = Workbook::new(4, 4).unwrap();
        book.set_cell(0, 0, "visible".into()).unwrap();
        book.set_cell(1, 0, "hidden-row".into()).unwrap();
        book.set_cell(0, 1, "hidden-col".into()).unwrap();
        let mut view = book.sheet_view().clone();
        view.hidden_rows.insert(1);
        view.hidden_columns.insert(1);
        book.set_sheet_view(view).unwrap();
        let mut layout = book.print_settings().clone();
        layout.area = Some(Range {
            top: 0,
            left: 0,
            bottom: 1,
            right: 1,
        });
        book.set_print_settings(layout).unwrap();
        let pages = pages(&book, &book.visible_rows()).unwrap();
        let text: Vec<_> = pages
            .iter()
            .flat_map(|p| &p.scene.items)
            .filter_map(|item| {
                if let SceneItem::TextBox { text, .. } = item {
                    Some(text.as_str())
                } else {
                    None
                }
            })
            .collect();
        assert!(text.contains(&"visible"));
        assert!(!text.contains(&"hidden-row"));
        assert!(!text.contains(&"hidden-col"));
    }
    #[test]
    fn paginates_all_visible_rows_with_repeated_headers_and_styling() {
        let mut b = Workbook::new(20, 3).unwrap();
        for r in 0..12 {
            b.set_cell(r, 0, format!("Row{r}")).unwrap();
        }
        b.set_style(
            Range {
                top: 0,
                left: 0,
                bottom: 0,
                right: 0,
            },
            CellStyle {
                bold: true,
                background: Some(0xff0000),
                ..Default::default()
            },
        )
        .unwrap();
        b.set_filter(Some(SheetFilter {
            range: Range {
                top: 0,
                left: 0,
                bottom: 11,
                right: 0,
            },
            column: 0,
            contains: "Row1".into(),
        }))
        .unwrap();
        let mut p = b.print_settings().clone();
        p.area = Some(Range {
            top: 0,
            left: 0,
            bottom: 11,
            right: 0,
        });
        p.paper_width_twips = 4000;
        p.paper_height_twips = 2200;
        p.margin_top_twips = 200;
        p.margin_bottom_twips = 200;
        p.margin_left_twips = 200;
        p.margin_right_twips = 200;
        p.repeat_header_rows = 1;
        p.row_breaks = vec![10];
        b.set_print_settings(p).unwrap();
        let pages = pages(&b, &b.visible_rows()).unwrap();
        assert!(pages.len() >= 2);
        let texts: Vec<_> = pages
            .iter()
            .flat_map(|p| p.scene.items.iter())
            .filter_map(|i| {
                if let SceneItem::TextBox { text, font, .. } = i {
                    Some((text.as_str(), font.bold))
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            texts.iter().filter(|(s, _)| *s == "Row0").count(),
            pages.len()
        );
        assert!(texts.contains(&("Row1", false)));
        assert!(texts.contains(&("Row10", false)));
        assert!(texts.contains(&("Row11", false)));
        assert!(!texts.iter().any(|(s, _)| *s == "Row2"));
        assert!(texts.iter().any(|(s, bold)| *s == "Row0" && *bold));
    }
}
