//! The basic sort remains compact; advanced options live in one additional menu dialog.
use crate::InputRequest;
pub const COMMAND: u16 = 85;
/// Advanced Sort form, with the key starting at the zero-based `column`.
pub fn request(column: usize) -> InputRequest {
    InputRequest {
        title: "Advanced Sort",
        fields: vec![
            (
                "Keys (one per line: index asc/desc)",
                format!("{} asc", column + 1),
            ),
            ("Direction", "Rows".into()),
            ("Header (yes/no)", "yes".into()),
            ("Case sensitive (yes/no)", "no".into()),
            ("Natural numbers in text (yes/no)", "no".into()),
            ("Custom priority list (semicolon separated)", String::new()),
        ],
    }
}
pub fn choices() -> Vec<(usize, Vec<String>)> {
    vec![
        (1, vec!["Rows".into(), "Columns".into()]),
        (2, vec!["yes".into(), "no".into()]),
        (3, vec!["no".into(), "yes".into()]),
        (4, vec!["no".into(), "yes".into()]),
    ]
}
pub fn parse(
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

