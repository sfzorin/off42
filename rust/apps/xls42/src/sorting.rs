//! Advanced Sort command wiring; the form and option parsing live in `suite42_app::sheet::sorting`.
use super::*;
pub(super) use suite42_app::sheet::sorting::{choices, parse, COMMAND};
pub(super) fn request(app: &Xls) -> suite42_app::InputRequest {
    suite42_app::sheet::sorting::request(app.col)
}

#[cfg(test)]
mod tests {
    use super::*;
    use suite42_app::App;
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
