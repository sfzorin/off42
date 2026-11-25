use super::*;

/// Export the explicitly supported RTF subset; reject data that would be lost.
pub(super) fn rtf_export(s: &State) -> Result<Vec<u8>, Error> {
    use std::fmt::Write;
    if !s.docx_parts.is_empty()
        || s.sections.len() != 1
        || !s.comments.is_empty()
        || !s.footnotes.is_empty()
        || !s.revisions.is_empty()
        || s.blocks
            .iter()
            .any(|b| !matches!(b, Block::Paragraph(_) | Block::Image(_) | Block::Table(_)))
    {
        return Err(Error::Unsupported(
            "RTF export does not yet support tables, images, sections or annotations".into(),
        ));
    }
    let header_ids: std::collections::HashSet<_> = s.sections[0]
        .header
        .iter()
        .chain(&s.sections[0].footer)
        .map(|p| p.id)
        .collect();
    if s.fields.iter().any(|f| {
        !header_ids.contains(&f.at.paragraph)
            || !matches!(f.kind, FieldKind::PageNumber | FieldKind::PageCount)
    }) {
        return Err(Error::Unsupported("RTF body or custom fields".into()));
    }
    let mut fonts = Vec::<String>::new();
    let mut colors = Vec::<u32>::new();
    for p in s
        .blocks
        .iter()
        .flat_map(|b| match b {
            Block::Paragraph(p) => vec![p],
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|r| r.iter().flat_map(|c| c.paragraphs.iter()))
                .collect(),
            _ => vec![],
        })
        .chain(s.sections[0].header.iter())
        .chain(s.sections[0].footer.iter())
    {
        if p.style.source_style.is_some() || !p.style.odt_extra.is_empty() {
            return Err(Error::Unsupported("RTF named paragraph styles".into()));
        }
        for r in &p.runs {
            if r.style.source_style.is_some()
                || r.style.theme_color.is_some()
                || !r.style.odt_extra.is_empty()
            {
                return Err(Error::Unsupported("RTF named character styles".into()));
            }
            if r.link.as_ref().is_some_and(|link| {
                link.is_empty()
                    || !link.is_ascii()
                    || link
                        .chars()
                        .any(|c| c.is_control() || matches!(c, '\\' | '{' | '}' | '"'))
            }) {
                return Err(Error::Unsupported("RTF hyperlink target".into()));
            }
            for color in [r.style.color, r.style.highlight].into_iter().flatten() {
                if color > 0xffffff {
                    return Err(Error::Invalid("invalid color".into()));
                }
                if !colors.contains(&color) {
                    colors.push(color);
                }
            }
            if r.style.font.contains(';') || r.style.font.chars().any(char::is_control) {
                return Err(Error::Unsupported("RTF font name".into()));
            }
            if !fonts.contains(&r.style.font) {
                fonts.push(r.style.font.clone());
            }
        }
    }
    for color in s
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Table(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .flat_map(|t| t.rows.iter().flatten().filter_map(|c| c.fill))
    {
        if color > 0xffffff {
            return Err(Error::Invalid("invalid cell color".into()));
        }
        if !colors.contains(&color) {
            colors.push(color);
        }
    }
    for border in s
        .blocks
        .iter()
        .filter_map(|b| {
            if let Block::Table(t) = b {
                Some(t)
            } else {
                None
            }
        })
        .flat_map(|t| t.rows.iter().flatten().filter_map(|c| c.border.as_ref()))
    {
        if border.color > 0xffffff || border.width_twips == 0 {
            return Err(Error::Invalid("invalid cell border".into()));
        }
        if !colors.contains(&border.color) {
            colors.push(border.color);
        }
    }
    let mut out = String::from("{\\rtf1\\ansi\\deff0\\uc1{\\fonttbl");
    for (i, font) in fonts.iter().enumerate() {
        write!(out, "{{\\f{i} ").unwrap();
        rtf_text(&mut out, font);
        out.push_str(";}");
    }
    out.push('}');
    {
        out.push_str("{\\colortbl;");
        for c in &colors {
            write!(
                out,
                "\\red{}\\green{}\\blue{};",
                (c >> 16) & 255,
                (c >> 8) & 255,
                c & 255
            )
            .unwrap();
        }
        out.push('}');
    }
    out.push_str(&super::rtf_lists::tables(&s.blocks)?);
    let page = &s.sections[0].page;
    write!(
        out,
        "\\paperw{}\\paperh{}\\margl{}\\margr{}\\margt{}\\margb{}\\headery{}\\footery{}",
        page.width_twips,
        page.height_twips,
        page.margin_left,
        page.margin_right,
        page.margin_top,
        page.margin_bottom,
        page.header_distance_twips,
        page.footer_distance_twips
    )
    .unwrap();
    if page.columns != 1 {
        return Err(Error::Unsupported("RTF columns".into()));
    }
    for (name, paras) in [
        ("header", &s.sections[0].header),
        ("footer", &s.sections[0].footer),
    ] {
        if !paras.is_empty() {
            write!(out, "{{\\{name} ").unwrap();
            for p in paras {
                if p.style.list.is_some() {
                    return Err(Error::Unsupported("RTF list in header/footer".into()));
                }
                write_paragraph(&mut out, p, &fonts, &colors, false, &s.fields)?;
            }
            out.push('}');
        }
    }
    for b in &s.blocks {
        match b {
            Block::Image(image) => {
                out.push_str("\\pard ");
                out.push_str(&super::rtf_picture::write(image)?);
                out.push_str("\\par\n");
            }
            Block::Paragraph(p) => write_paragraph(&mut out, p, &fonts, &colors, false, &s.fields)?,
            Block::Table(t) => {
                let widths = super::table_edit::column_widths(t);
                let layout = super::table_edit::table_layout(t)?;
                for (row_index, row) in t.rows.iter().enumerate() {
                    out.push_str("\\trowd");
                    let mut boundary = 0u32;
                    for (i, cell) in row.iter().enumerate() {
                        let (owner_row, anchor) = layout[row_index][i];
                        let visual = &t.rows[owner_row][anchor];
                        if cell.paragraphs.is_empty() {
                            return Err(Error::Unsupported(
                                "RTF merged or styled table cells".into(),
                            ));
                        }
                        if let Some(border) = &visual.border {
                            let index = colors
                                .iter()
                                .position(|v| *v == border.color)
                                .ok_or_else(|| Error::Invalid("missing border color".into()))?
                                + 1;
                            let end = anchor + visual.col_span as usize - 1;
                            let bottom = owner_row + visual.row_span as usize - 1;
                            for edge in ["t", "l", "b", "r"] {
                                if edge == "l" && i != anchor
                                    || edge == "r" && i != end
                                    || edge == "t" && row_index != owner_row
                                    || edge == "b" && row_index != bottom
                                {
                                    continue;
                                }
                                write!(
                                    out,
                                    "\\clbrdr{edge}\\brdrs\\brdrw{}\\brdrcf{index}",
                                    border.width_twips
                                )
                                .unwrap();
                            }
                        }
                        if let Some(fill) = visual.fill {
                            let index = colors
                                .iter()
                                .position(|v| *v == fill)
                                .ok_or_else(|| Error::Invalid("missing cell color".into()))?
                                + 1;
                            write!(out, "\\clcbpat{index}").unwrap();
                        }
                        if visual.col_span > 1 {
                            out.push_str(if i == anchor { "\\clmgf" } else { "\\clmrg" });
                        }
                        if visual.row_span > 1 {
                            out.push_str(if row_index == owner_row {
                                "\\clvmgf"
                            } else {
                                "\\clvmrg"
                            });
                        }
                        boundary = boundary
                            .checked_add(*widths.get(i).ok_or_else(|| {
                                Error::Invalid("missing table column width".into())
                            })?)
                            .ok_or(Error::Limit)?;
                        if boundary > i32::MAX as u32 {
                            return Err(Error::Limit);
                        }
                        write!(out, "\\cellx{boundary}").unwrap();
                    }
                    out.push(' ');
                    for cell in row {
                        for p in &cell.paragraphs {
                            write_paragraph(&mut out, p, &fonts, &colors, true, &s.fields)?;
                        }
                        out.push_str("\\cell ");
                    }
                    out.push_str("\\row\n");
                }
            }
            _ => return Err(Error::Unsupported("RTF section break".into())),
        }
    }
    out.push('}');
    Ok(out.into_bytes())
}
fn write_paragraph(
    out: &mut String,
    p: &Paragraph,
    fonts: &[String],
    colors: &[u32],
    in_table: bool,
    fields: &[Field],
) -> Result<(), Error> {
    use std::fmt::Write;
    let a = match p.style.alignment {
        Alignment::Left => "ql",
        Alignment::Center => "qc",
        Alignment::Right => "qr",
        Alignment::Justify => "qj",
    };
    write!(
        out,
        "\\pard\\{a}\\li{}\\ri{}\\fi{}\\sb{}\\sa{}\\sl{} ",
        p.style.left_twips,
        p.style.right_twips,
        p.style.first_line_twips,
        p.style.before_twips,
        p.style.after_twips,
        p.style.line_twips
    )
    .unwrap();
    for tab in &p.style.tabs_twips {
        write!(out, "\\tx{tab} ").unwrap();
    }
    if p.style.keep_with_next {
        out.push_str("\\keepn ");
    }
    if p.style.keep_lines {
        out.push_str("\\keep ");
    }
    if p.style.page_break_before {
        out.push_str("\\pagebb ");
    }
    if let Some(level) = p.style.outline_level {
        write!(out, "\\outlinelevel{level} ").unwrap();
    }
    if in_table {
        out.push_str("\\intbl ");
    }
    if let Some(list) = &p.style.list {
        write!(out, "\\ls{}\\ilvl{} ", list.id, list.level).unwrap();
    }
    let mut markers = fields
        .iter()
        .filter(|f| f.at.paragraph == p.id)
        .collect::<Vec<_>>();
    markers.sort_by_key(|f| f.at.offset);
    let mut marker_index = 0;
    let mut base = 0usize;
    for r in &p.runs {
        let end = base + r.text.len();
        let mut cursor = base;
        while marker_index < markers.len() && markers[marker_index].at.offset <= end {
            let field = markers[marker_index];
            if field.at.offset < cursor {
                return Err(Error::Invalid("field ordering".into()));
            }
            let piece = r
                .text
                .get(cursor - base..field.at.offset - base)
                .ok_or_else(|| Error::Invalid("field text boundary".into()))?;
            write_run(out, r, piece, fonts, colors);
            write_field(out, &field.kind)?;
            cursor = field.at.offset;
            marker_index += 1;
        }
        let piece = r
            .text
            .get(cursor - base..)
            .ok_or_else(|| Error::Invalid("field text boundary".into()))?;
        write_run(out, r, piece, fonts, colors);
        base = end;
    }
    while marker_index < markers.len() {
        if markers[marker_index].at.offset != base {
            return Err(Error::Invalid("field outside paragraph".into()));
        }
        write_field(out, &markers[marker_index].kind)?;
        marker_index += 1;
    }
    out.push_str("\\par\n");
    Ok(())
}
fn write_field(out: &mut String, kind: &FieldKind) -> Result<(), Error> {
    match kind {
        FieldKind::PageNumber => out.push_str("\\chpgn "),
        FieldKind::PageCount => out.push_str("{\\field{\\*\\fldinst NUMPAGES}{\\fldrslt 1}}"),
        _ => return Err(Error::Unsupported("RTF field".into())),
    }
    Ok(())
}
fn write_run(out: &mut String, r: &Run, text: &str, fonts: &[String], colors: &[u32]) {
    use std::fmt::Write;
    if text.is_empty() {
        return;
    }
    if let Some(link) = &r.link {
        write!(
            out,
            "{{\\field{{\\*\\fldinst HYPERLINK \"{link}\"}}{{\\fldrslt "
        )
        .unwrap();
    }
    let f = fonts.iter().position(|x| *x == r.style.font).unwrap();
    let t = &r.style;
    write!(
        out,
        "{{\\f{f}\\fs{}\\b{}\\i{}\\ul{}\\strike{}\\cf{}\\highlight{} ",
        t.size_half_points,
        u8::from(t.bold),
        u8::from(t.italic),
        u8::from(t.underline),
        u8::from(t.strike),
        t.color
            .map(|c| colors.iter().position(|x| *x == c).unwrap() + 1)
            .unwrap_or(0),
        t.highlight
            .map(|c| colors.iter().position(|x| *x == c).unwrap() + 1)
            .unwrap_or(0)
    )
    .unwrap();
    rtf_text(out, text);
    out.push('}');
    if r.link.is_some() {
        out.push_str("}}");
    }
}
fn rtf_text(out: &mut String, text: &str) {
    use std::fmt::Write;
    for ch in text.chars() {
        match ch {
            '\\' | '{' | '}' => {
                out.push('\\');
                out.push(ch);
            }
            '\t' => out.push_str("\\tab "),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for u in ch.encode_utf16(&mut units) {
                    write!(out, "\\u{}?", *u as i16).unwrap();
                }
            }
        }
    }
}
