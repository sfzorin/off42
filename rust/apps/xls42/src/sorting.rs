//! The basic sort remains compact; advanced options live in one additional menu dialog.
use super::*;
pub(super) const COMMAND: u16 = 85;
pub(super) fn request(app: &Xls) -> win42::InputRequest {
    win42::InputRequest {
        title: "Advanced Sort",
        fields: vec![
            (
                "Keys (one per line: index asc/desc)",
                format!("{} asc", app.col + 1),
            ),
            ("Direction", "Rows".into()),
            ("Header (yes/no)", "yes".into()),
            ("Case sensitive (yes/no)", "no".into()),
            ("Natural numbers in text (yes/no)", "no".into()),
            ("Custom priority list (semicolon separated)", String::new()),
        ],
    }
}
pub(super) fn choices() -> Vec<(usize, Vec<String>)> {
    vec![
        (1, vec!["Rows".into(), "Columns".into()]),
        (2, vec!["yes".into(), "no".into()]),
        (3, vec!["no".into(), "yes".into()]),
        (4, vec!["no".into(), "yes".into()]),
    ]
}
pub(super) fn parse(
    values: &[String],
) -> Result<(Vec<xls42_core::SortKey>, xls42_core::SortOptions), String> {
    let [keys, direction, header, case, natural, custom] = values else {
        return Err("Invalid sort options".into());
    };
    let mut parsed = Vec::new();
    for line in keys.lines().filter(|line| !line.trim().is_empty()) {
        let words = line
            .split(|c: char| c.is_whitespace() || c == ',')
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>();
        if words.is_empty() || words.len() > 2 {
            return Err("Use one key per line: 1 asc or 2 desc".into());
        }
        let index = words[0]
            .parse::<usize>()
            .ok()
            .and_then(|v| v.checked_sub(1))
            .ok_or("Enter positive key indexes")?;
        let descending = match words.get(1).map(|v| v.to_ascii_lowercase()).as_deref() {
            None | Some("asc") => false,
            Some("desc") => true,
            _ => return Err("Use asc or desc after each key index".into()),
        };
        parsed.push(xls42_core::SortKey { index, descending });
        if parsed.len() > 64 {
            return Err("Use up to 64 sort keys".into());
        }
    }
    if parsed.is_empty() {
        return Err("Enter at least one sort key".into());
    }
    let yes = |text: &str| match text.to_ascii_lowercase().as_str() {
        "yes" => Ok(true),
        "no" => Ok(false),
        _ => Err("Choose yes or no"),
    };
    let options = xls42_core::SortOptions {
        left_to_right: match direction.as_str() {
            "Rows" => false,
            "Columns" => true,
            _ => return Err("Choose Rows or Columns".into()),
        },
        header: yes(header)?,
        case_sensitive: yes(case)?,
        natural: yes(natural)?,
        custom_list: custom
            .split(';')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned)
            .collect(),
    };
    Ok((parsed, options))
}

#[cfg(test)]
mod tests {
    use super::*;
    use win42::App;
    fn app() -> Xls {
        Xls {
            book: Workbook::new(10, 5).unwrap(),
            goal_seek: Default::default(),
            solver: Default::default(),
            viewport: Default::default(),
            row: 0,
            col: 0,
            preview: None,
            visible_cache: Default::default(),
            picture_cache: Default::default(),
            selected_picture: None,
        }
    }
    #[test]
    fn advanced_menu_multiline_choices_and_normal_action_sort_complete_rows_with_one_history_step()
    {
        let mut app = app();
        assert!(app.command_specs().iter().any(|c| c.id == COMMAND));
        assert_eq!(app.input_multiline_fields(COMMAND), vec![0]);
        assert_eq!(app.input_choices(COMMAND).len(), 4);
        for (r, priority, part) in [
            (0, "Priority", "Part"),
            (1, "low", "R10"),
            (2, "HIGH", "R2"),
            (3, "low", "R2"),
        ] {
            app.book.set_cell(r, 0, priority.into()).unwrap();
            app.book.set_cell(r, 1, part.into()).unwrap();
            app.book.set_cell(r, 2, format!("id{r}")).unwrap();
        }
        let range = Range {
            top: 0,
            left: 0,
            bottom: 9,
            right: 2,
        };
        app.choose_range(range, (0, 0)).unwrap();
        let before = app.book.project_bytes().unwrap();
        let values = vec![
            "1 asc\n2 asc".into(),
            "Rows".into(),
            "yes".into(),
            "no".into(),
            "yes".into(),
            "high; low".into(),
        ];
        app.input_action(COMMAND, &values).unwrap();
        assert_eq!(
            [
                app.book.cell(1, 2),
                app.book.cell(2, 2),
                app.book.cell(3, 2)
            ],
            [Some("id2"), Some("id3"), Some("id1")]
        );
        assert_eq!(app.book.cell(0, 0), Some("Priority"));
        let changed = app.book.project_bytes().unwrap();
        app.action(4).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), before);
        app.action(5).unwrap();
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        let mut invalid = values.clone();
        invalid[0] = "0 desc".into();
        assert!(app.input_action(COMMAND, &invalid).is_err());
        assert_eq!(app.book.project_bytes().unwrap(), changed);
        app.book.set_protected(true);
        assert!(!app.enabled(COMMAND));
        let protected = app.book.project_bytes().unwrap();
        assert!(app.input_action(COMMAND, &values).is_err());
        assert_eq!(app.book.project_bytes().unwrap(), protected);
    }
}
