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
    if (area.bottom-area.top+1).checked_mul(area.right-area.left+1).is_none_or(|n|n>100_000){return Err("Print area exceeds 100,000 cells; choose a smaller print area".into())}
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
            !book.sheet_view().hidden_rows.contains(r) && (*r < header_end || visible.binary_search(r).is_ok())
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
    let cells = book.display_cells();
    let mut picture_bytes = 0usize;
    let decoded = book
        .images()
        .iter()
        .filter(|picture| rows.contains(&picture.row) && cols.contains(&picture.column))
        .map(|picture| {
            image::load_from_memory(&picture.data)
                .map(|data| (picture, data.into_rgba8()))
                .map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;
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
                        text: cells.get(&(r,c)).cloned().unwrap_or_default(),
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
                        wrap: cells.get(&(r,c)).is_some_and(|v|v.contains('\n')),
                    });
                    x += cw(c) * scale;
                }
                y += rh(r) * scale;
            }
            let page_width: f64 = page_cols.iter().map(|&col| cw(col) * scale).sum();
            let page_height: f64 = page_rows.iter().map(|&row| rh(row) * scale).sum();
            for (picture, rgba) in &decoded {
                let anchor_x: f64 = cols
                    .iter()
                    .take_while(|&&col| col < picture.column)
                    .map(|&col| cw(col) * scale)
                    .sum();
                let page_x: f64 = cols
                    .iter()
                    .take_while(|&&col| col < page_cols[0])
                    .map(|&col| cw(col) * scale)
                    .sum();
                let x = anchor_x - page_x + picture.offset_x * scale;
                let (y, clip_top, clip_height) = if picture.row < header_end {
                    let y: f64 = headers
                        .iter()
                        .take_while(|&&row| row < picture.row)
                        .map(|&row| rh(row) * scale)
                        .sum();
                    (y + picture.offset_y * scale, 0.0, header_height)
                } else {
                    let anchor: f64 = body
                        .iter()
                        .take_while(|&&row| row < picture.row)
                        .map(|&row| rh(row) * scale)
                        .sum();
                    let first_body = page_rows
                        .iter()
                        .find(|&&row| row >= header_end)
                        .copied()
                        .unwrap_or(header_end);
                    let page: f64 = body
                        .iter()
                        .take_while(|&&row| row < first_body)
                        .map(|&row| rh(row) * scale)
                        .sum();
                    (
                        header_height + anchor - page + picture.offset_y * scale,
                        header_height,
                        page_height - header_height,
                    )
                };
                let w = picture.width_points * scale;
                let h = picture.height_points * scale;
                if clip_height <= 0.0
                    || x >= page_width
                    || x + w <= 0.0
                    || y >= clip_top + clip_height
                    || y + h <= clip_top
                {
                    continue;
                }
                picture_bytes = picture_bytes
                    .checked_add(rgba.len())
                    .ok_or("Print image budget exceeded")?;
                if picture_bytes > 128 * 1024 * 1024 {
                    return Err("Print fewer pages at a time: image data exceeds 128 MB".into());
                }
                scene.items.push(SceneItem::ClippedOffset {
                    clip: win42::CanvasRect {
                        x: 0.0,
                        y: clip_top,
                        width: page_width,
                        height: clip_height,
                    },
                    offset: win42::Point { x: 0.0, y: 0.0 },
                    items: vec![SceneItem::ImageScaled {
                        x,
                        y,
                        width: w,
                        height: h,
                        pixel_width: picture.pixel_width,
                        pixel_height: picture.pixel_height,
                        rgba: rgba.as_raw().clone(),
                    }],
                });
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
    fn far_sparse_cell_prints_in_a_selected_area_while_oversized_area_is_refused(){
        let mut b=Workbook::new(xls42_core::MAX_ROWS,xls42_core::MAX_COLS).unwrap();
        let (r,c)=(xls42_core::MAX_ROWS-1,xls42_core::MAX_COLS-1);b.set_cell(r,c,"LAST-42".into()).unwrap();
        let visible=b.visible_rows();assert!(pages(&b,&visible).is_err());
        let mut settings=b.print_settings().clone();settings.area=Some(Range{top:r,bottom:r,left:c,right:c});b.set_print_settings(settings).unwrap();
        let printed=pages(&b,&visible).unwrap();assert_eq!(printed.len(),1);
        assert!(printed[0].scene.items.iter().any(|item|matches!(item,SceneItem::TextBox{text,..}if text=="LAST-42")));
        b.undo();assert!(pages(&b,&visible).is_err());b.redo();assert_eq!(pages(&b,&visible).unwrap().len(),1);
    }
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

#[cfg(test)]
mod picture_tests {
    use super::*;
    fn image_bytes() -> Vec<u8> {
        let mut data = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(8, 4, image::Rgba([255, 0, 0, 128]))
            .write_to(&mut data, image::ImageFormat::Png)
            .unwrap();
        data.into_inner()
    }
    #[test]
    fn picture_printing_keeps_physical_size_and_clips_across_pages() {
        let mut book = Workbook::new(20, 8).unwrap();
        book.insert_image(0, 0, image_bytes(), 180.0, 120.0, String::new())
            .unwrap();
        let mut setup = book.print_settings().clone();
        setup.paper_width_twips = 2400;
        setup.paper_height_twips = 2400;
        setup.margin_left_twips = 200;
        setup.margin_top_twips = 200;
        setup.margin_right_twips = 200;
        setup.margin_bottom_twips = 200;
        setup.fit_width = false;
        book.set_print_settings(setup).unwrap();
        let pages = pages(&book, &book.visible_rows()).unwrap();
        assert!(pages.len() >= 4);
        let mut images = 0;
        for page in &pages {
            for item in &page.scene.items {
                if let SceneItem::ClippedOffset { clip, items, .. } = item {
                    assert!(clip.width <= 100.0 && clip.height <= 100.0);
                    for image in items {
                        if let SceneItem::ImageScaled {
                            width,
                            height,
                            rgba,
                            ..
                        } = image
                        {
                            assert_eq!((*width, *height), (180.0, 120.0));
                            assert_eq!(rgba[3], 128);
                            images += 1;
                        }
                    }
                }
            }
        }
        assert!(images >= 4);
        book.set_cell(1, 0, "Visible after hiding the picture anchor".into())
            .unwrap();
        let mut view = book.sheet_view().clone();
        view.hidden_rows.insert(0);
        book.set_sheet_view(view).unwrap();
        for page in pages_without_shadow(&book) {
            assert!(!page
                .scene
                .items
                .iter()
                .any(|item| matches!(item, SceneItem::ClippedOffset { .. })));
        }
    }
    fn pages_without_shadow(book: &Workbook) -> Vec<PrintPage> {
        pages(book, &book.visible_rows()).unwrap()
    }
}
