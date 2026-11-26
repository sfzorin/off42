//! Table report shown by a shell (comparison results, batch outcomes); the shell draws it.
#[derive(Clone, Debug)]
pub struct ReportRequest {
    pub title: String,
    pub summary: String,
    pub columns: Vec<&'static str>,
    pub column_colors: Vec<Option<[u8; 3]>>,
    pub rows: Vec<Vec<String>>,
    pub highlight_controls: bool,
    pub compact_first_column: bool,
    pub allow_keep: bool,
    pub keeping: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum ReportEvent {
    Select(usize),
    Keep(bool),
    Clear,
    Closed { keep: bool },
}
