pub mod ocr;
pub mod recovery;
mod sparse_table;
pub use sparse_table::SparseTable;
mod system_font;
pub use system_font::program as system_font_program;
#[cfg(any(windows,test))]
mod proofing;
#[cfg(any(windows,test))]
mod pdf_print_layout;
#[cfg(windows)]
pub mod pdfium;
#[cfg(windows)]
mod pdf_preview;
pub mod localization;
pub use label42::{Format as BarcodeFormat, TechnicalElement};
use std::path::{Path, PathBuf};
pub struct TechnicalTarget {
    pub id: u64,
    pub title: String,
    pub element: TechnicalElement,
}
impl TechnicalTarget {
    pub fn new(id: u64, element: TechnicalElement) -> Self {
        let title = element
            .symbol_id
            .as_deref()
            .and_then(|id| label42::symbols().iter().find(|symbol| symbol.id == id))
            .map(|symbol| symbol.title.to_owned())
            .unwrap_or_else(|| {
                element
                    .value
                    .chars()
                    .take(80)
                    .map(|c| if c.is_control() { ' ' } else { c })
                    .collect()
            });
        Self { id, title, element }
    }
}
pub struct TechnicalRaster {
    pub element: TechnicalElement,
    pub png: Vec<u8>,
    pub width_points: f64,
    pub height_points: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: std::sync::Arc<[u8]>,
    pub vector: label42::TechnicalVector,
}
impl TechnicalRaster {
    pub fn render(element: TechnicalElement) -> Result<Self, String> {
        let vector = element.vector()?;
        let png = element.png()?;
        let decoded = image::load_from_memory(&png)
            .map_err(|e| e.to_string())?
            .into_rgba8();
        let width_points = vector.width_points;
        let height_points = vector.height_points;
        Ok(Self {
            element,
            png,
            width_points,
            height_points,
            pixel_width: decoded.width(),
            pixel_height: decoded.height(),
            rgba: decoded.into_raw().into(),
            vector,
        })
    }
}

/// Application commands must not overlap host menus or native control notifications.
pub fn application_command_id(id:u16)->bool{(10..62000).contains(&id)&&!(100..=102).contains(&id)}
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[derive(Clone)]
pub struct InputRequest {
    pub title: &'static str,
    pub fields: Vec<(&'static str, String)>,
}
#[derive(Clone, Copy, Debug)]
pub struct FileAction {
    pub id: u16,
    pub title: &'static str,
    pub save: bool,
    pub extension: &'static str,
    pub format: &'static str,
    pub pattern: &'static str,
}

/// Shared host action requested after a document gesture has released mouse capture.
pub const CREATE_LABEL_SHEET_COMMAND: u16 = 65518;

pub enum LabelSource {
    Text(String),
    /// Keep other source choices usable if an individual artwork capture fails.
    Unavailable(String),
    /// Independent single-page PDF; optional region is in its unrotated PDF coordinates.
    PdfArtwork { bytes:Vec<u8>, region:Option<[f64;4]> },
    /// Encoded PNG/JPEG; decode only the source actually selected for label generation.
    PictureArtwork { bytes:Vec<u8>, width_points:f64, height_points:f64 },
    /// Original one-based row number and rendered values from the selected columns.
    Rows(Vec<(usize, Vec<String>)>),
}

/// Portable contract between a document model and the native Windows host.
pub trait App {
    /// Native top-level window handle, when a Windows host is available.
    fn set_native_window(&mut self, _window: usize) {}
    /// Continue a command after its settings dialog has closed.
    fn take_pending_command(&mut self) -> Option<u16> {
        None
    }
    /// Open a completed, explicitly requested output in a separate process/window.
    fn take_pending_new_window(&mut self)->Option<std::path::PathBuf>{None}
    /// Request an ordinary document replacement after an app command.
    fn take_pending_open(&mut self) -> Option<PathBuf> {
        None
    }
    /// Begin an asynchronous replacement, or return false to use `open`.
    /// The old model remains intact until `commit_async_open` succeeds.
    fn begin_async_open(&mut self, _path: &Path) -> Result<bool, String> {
        Ok(false)
    }
    /// A worker returns the identity of the exact bytes it prepared; it must
    /// retain that prepared model until commit or cancellation.
    fn take_async_open_result(
        &mut self,
    ) -> Option<Result<suite42_core::session::FileIdentity, String>> {
        None
    }
    /// Request a masked credential while an asynchronous open remains pending.
    fn take_async_open_password_request(&mut self) -> Option<InputRequest> {
        None
    }
    /// Resume the same pending open with a credential, or cancel it.
    fn submit_async_open_password(&mut self, _password: Option<&str>) -> Result<(), String> {
        Err("This open does not accept a password".into())
    }
    /// Atomically install the prepared model and return its clean revision.
    /// On error the previously open model must remain unchanged.
    fn commit_async_open(&mut self) -> Result<u64, String> {
        Err("No prepared document".into())
    }
    /// Signal cancellation and discard prepared state without joining the worker.
    fn cancel_async_open(&mut self) {}
    fn poll_background(&mut self) -> bool {
        false
    }
    fn background_poll_ms(&self) -> u32 {
        100
    }
    /// Consume a finished background print render on the UI thread.
    fn take_pending_print_job(&mut self) -> Option<Result<PrintJob, String>> {
        None
    }
    /// Opt-in serialized snapshot for host-managed, conflict-aware saves.
    /// Legacy adapters retain their existing save-as-only behavior.
    fn save_snapshot(&mut self, _format: &str) -> Result<Option<(u64, Vec<u8>)>, String> {
        Ok(None)
    }
    /// Whether ordinary Save may overwrite the current session source.
    fn can_save_in_place(&self) -> bool {
        true
    }
    /// Encode with the concrete destination; `is_current` means the session source path.
    fn save_snapshot_to(
        &mut self,
        format: &str,
        _path: &Path,
        _is_current: bool,
    ) -> Result<Option<(u64, Vec<u8>)>, String> {
        self.save_snapshot(format)
    }
    /// Capture a main file and optional immutable sidecars for a concrete destination.
    fn save_bundle_to(
        &mut self,
        format: &str,
        path: &Path,
        is_current: bool,
    ) -> Result<Option<(u64, suite42_core::session::SaveBundle)>, String> {
        Ok(self
            .save_snapshot_to(format, path, is_current)?
            .map(|(revision, main)| {
                (
                    revision,
                    suite42_core::session::SaveBundle {
                        main,
                        sidecars: Vec::new(),
                    },
                )
            }))
    }
    /// Mark precisely the published revision as saved, even if editing continued.
    fn saved_snapshot(&mut self, _revision: u64) {}
    fn open_formats(&self) -> Vec<FileFormat> {
        vec![]
    }
    /// Readonly backup encoding; never marks a user revision as saved.
    fn recovery_snapshot(&self)->Result<Option<recovery::Snapshot>,String>{Ok(None)}
    fn supports_recovery(&self)->bool{false}
    /// Private journal encodings need not appear in normal Open/Save dialogs.
    fn recovery_extensions(&self)->&'static[&'static str]{&[]}
    fn save_formats(&self) -> Vec<FileFormat> {
        vec![]
    }
    fn command_specs(&self) -> Vec<CommandSpec> {
        vec![]
    }
    /// Keep less frequent menu commands out of the primary toolbar.
    fn toolbar_command_visible(&self, _id:u16)->bool {true}
    fn toolbar_command_toggle(&self,id:u16)->bool{
        use RichFormatCommand::*;
        matches!(self.rich_format_command(id),Some(Bold|Italic|Underline|AlignLeft|AlignCenter|AlignRight|Justify))
    }

    fn command_state(&self, id: u16) -> CommandState {
        CommandState {
            enabled: self.enabled(id),
            checked: false,
        }
    }
    fn canvas_scene(&self) -> Option<CanvasScene> {
        None
    }
    fn canvas_pointer_modifiers(&self) -> bool {
        false
    }
    /// A RichEdit overlay in canvas document coordinates; None uses normal layout.
    fn inline_rich_rect(&self) -> Option<CanvasRect> {
        None
    }
    fn finish_inline_rich(&mut self, _commit: bool) -> Result<(), String> {
        Ok(())
    }
    /// Logical scene dimensions while presenting fullscreen, or None in editing mode.
    fn presentation_size(&self) -> Option<(f64, f64)> {
        None
    }
    /// Consumed only after an explicit application command.
    fn take_external_url(&mut self) -> Option<String> {
        None
    }
    fn canvas_event(&mut self, _event: CanvasEvent) -> Result<(), String> {
        Ok(())
    }
    fn name(&self) -> &'static str;
    fn extension(&self) -> &'static str;
    fn format_name(&self) -> &'static str;
    fn actions(&self) -> &'static [(u16, &'static str)];
    fn supports_new(&self) -> bool {
        false
    }
    fn new_document(&mut self) -> Result<(), String> {
        Err("New document unavailable".into())
    }
    fn file_actions(&self) -> &'static [FileAction] {
        &[]
    }
    fn supports_production(&self) -> bool {
        false
    }
    fn supports_technical_elements(&self) -> bool {
        false
    }
    fn can_insert_technical_element(&self) -> bool {
        self.supports_technical_elements()
    }
    fn technical_targets(&self) -> Vec<TechnicalTarget> {
        Vec::new()
    }
    fn selected_technical_target(&self) -> Option<u64> {
        None
    }
    fn apply_technical_element(
        &mut self,
        _id: Option<u64>,
        _raster: TechnicalRaster,
    ) -> Result<(), String> {
        Err("Technical elements are unavailable".into())
    }
    /// Consume a one-shot immutable source selected by an explicit document gesture.
    fn take_label_source_override(&mut self) -> Result<Option<LabelSource>, String> {
        Ok(None)
    }
    fn label_sources(&self) -> Result<Vec<(String, LabelSource)>, String> {
        Ok(vec![("Document".into(), self.label_source()?)])
    }
    /// Width available to an independently rendered native rich-text selection, in twips.
    fn label_rich_selection_width(&self)->Option<u32>{None}
    fn label_source(&self) -> Result<LabelSource, String> {
        Ok(LabelSource::Text(String::new()))
    }
    fn supports_marks(&self) -> bool {
        self.mark_file_action().is_some()
    }
    fn mark_canvas_size(&self) -> Option<(u32, u32)> {
        None
    }
    fn place_mark(
        &mut self,
        _width: u32,
        _height: u32,
        _rgba: &[u8],
        _rect: [u32; 4],
    ) -> Result<(), String> {
        Err("Image marks are unavailable in this document.".into())
    }
    /// Existing import action that can insert a saved PNG signature or stamp.
    fn mark_file_action(&self) -> Option<u16> {
        None
    }
    /// Optional export settings collected without changing the document. Both
    /// this prompt and the file picker must succeed before dispatch with values.
    fn file_input_request(&self, _id: u16) -> Option<InputRequest> {
        None
    }
    /// Resolve the export filter after collecting settings and before showing
    /// the picker. This does not mutate the document or start an operation.
    fn configured_file_action(&self, id: u16, _values: &[String]) -> Result<FileAction, String> {
        self.file_actions()
            .iter()
            .find(|action| action.id == id)
            .copied()
            .ok_or_else(|| "Unsupported file action".into())
    }
    fn file_action_with_input(
        &mut self,
        _id: u16,
        _path: &Path,
        _values: &[String],
    ) -> Result<(), String> {
        Err("Unsupported file action settings".into())
    }
    fn file_action(&mut self, _id: u16, _path: &Path) -> Result<(), String> {
        Err("Unsupported file action".into())
    }
    /// A command may supply pre-rendered pages for the native print dialog.
    fn clipboard_request(&self, _id: u16) -> Option<ClipboardRequest> {
        None
    }
    fn clipboard_data(&mut self, _id: u16, _bytes: Vec<u8>) -> Result<(), String> {
        Err("Clipboard action unavailable".into())
    }
    fn clipboard_text(&mut self, _id: u16, _text: String) -> Result<(), String> {
        Err("Clipboard action unavailable".into())
    }
    /// Print using explicit paper geometry. Scene coordinates are points from the content origin.
    /// Ask the native host for a paginated RichEdit preview; geometry is in twips.
    fn rich_preview(&self, _id: u16) -> Option<RichPrint> {
        None
    }
    /// Ordinary document printing, shared by File > Print and Ctrl+P.
    fn default_print_command(&self)->Option<u16>{None}
    /// Use the native selection and insertion-point formatting rules for this command.
    fn rich_format_command(&self, _id: u16) -> Option<RichFormatCommand> { None }
    fn print_job(&self, _id: u16) -> Option<PrintJob> {
        None
    }
    fn take_pending_report(&mut self)->Option<(u16,ReportRequest)>{None}
    fn report_event(&mut self,_id:u16,_event:ReportEvent)->Result<(),String>{Err("Report action unavailable".into())}
    fn dialog_request(&self, _id: u16) -> Option<DialogRequest> {
        None
    }
    fn dialog_result(&mut self, _id: u16, _result: DialogResult) -> Result<(), String> {
        Err("Unsupported dialog action".into())
    }
    /// Use a multiline first field in the native input dialog.
    fn multiline_input(&self, _id: u16) -> bool {
        false
    }
    /// Multiline fields in the native input dialog; existing first-field forms retain their behavior.
    fn input_multiline_fields(&self, id:u16)->Vec<usize> {
        if self.multiline_input(id) { vec![0] } else { vec![] }
    }
    /// Mask the first native input field and clear the returned buffers after dispatch.
    fn password_input(&self, _id: u16) -> bool {
        false
    }
    /// Pick folders for the specified input field indices before displaying the form.
    fn input_folder_fields(&self, _id: u16) -> Vec<(usize, &'static str)> {
        vec![]
    }
    fn input_request(&self, _id: u16) -> Option<InputRequest> {
        None
    }
    /// Stable original values for native translated choice fields.
    fn input_choices(&self, _id: u16) -> Vec<(usize, Vec<String>)> {
        vec![]
    }
    fn input_action(&mut self, _id: u16, _values: &[String]) -> Result<(), String> {
        Err("Unsupported input action".into())
    }
    fn supports_find_replace(&self) -> bool {
        false
    }
    fn find_next(
        &self,
        _query: &str,
        _start_utf16: usize,
    ) -> Result<Option<(usize, usize)>, String> {
        Err("Find unavailable".into())
    }
    fn replace_all(&mut self, _query: &str, _replacement: &str) -> Result<usize, String> {
        Err("Replace unavailable".into())
    }
    fn view_actions(&self) -> &'static [(u16, &'static str)] {
        &[]
    }
    fn items(&self) -> Vec<String>;
    fn text(&self) -> String;
    fn status(&self) -> String;
    fn dirty(&self) -> bool;
    fn can_undo(&self) -> bool;
    fn can_redo(&self) -> bool;
    fn open(&mut self, path: &Path) -> Result<(), String>;
    fn save(&mut self, path: &Path) -> Result<(), String>;
    fn edit(&mut self, text: String) -> Result<(), String>;
    fn select(&mut self, index: usize);
    fn action(&mut self, id: u16) -> Result<(), String>;
    fn open_filter(&self) -> String {
        format!("*.{}", self.extension())
    }
    fn open_is_add(&self) -> bool {
        false
    }
    fn opens_as_import(&self, _path: &Path) -> bool {
        self.open_is_add()
    }
    fn import_open(&mut self, path: &Path) -> Result<(), String> {
        self.open(path)
    }
    fn selected(&self) -> usize {
        0
    }
    fn enabled(&self, _id: u16) -> bool {
        true
    }
    /// Opt in to the variable-geometry grid instead of the legacy list view.
    fn uses_custom_grid(&self) -> bool {
        false
    }
    fn grid_column_width(&self, _column: usize) -> u16 {
        80
    }
    fn grid_row_height(&self, _row: usize) -> u16 {
        20
    }
    fn resize_grid_column(&mut self, _column: usize, _pixels: u16) -> Result<(), String> {
        Err("Column resizing is unavailable".into())
    }
    fn resize_grid_row(&mut self, _row: usize, _pixels: u16) -> Result<(), String> {
        Err("Row resizing is unavailable".into())
    }
    fn grid_row_label(&self, row: usize) -> String {
        (row + 1).to_string()
    }
    fn grid_clipboard_commands(&self) -> Option<(u16, u16)> {
        None
    }
    /// Sheet names and selected index for grid-based applications.
    fn grid_sheets(&self) -> Option<(Vec<String>, usize)> {
        None
    }
    fn select_grid_sheet(&mut self, _index: usize) {}
    fn grid_selection(&self) -> Option<GridRange> {
        None
    }
    fn grid_frozen_panes(&self) -> (usize, usize) {
        (0, 0)
    }
    fn select_grid_range(&mut self, _range: GridRange) {}
    fn select_grid_range_with_focus(&mut self, range: GridRange, _focus: (usize, usize)) {
        self.select_grid_range(range)
    }
    fn grid_pictures(&self) -> Vec<GridPicture> {
        Vec::new()
    }
    fn select_grid_picture(&mut self, _id: Option<u64>) {}
    fn move_grid_picture(&mut self, _id: u64, _dx: f64, _dy: f64) -> Result<(), String> {
        Err("Picture movement unavailable".into())
    }
    fn grid_picture_commands(&self) -> Option<(u16, u16)> {
        None
    }
    fn grid_cell_style(&self, _row: usize, _column: usize) -> Option<GridCellStyle> {
        None
    }
    /// Preferred immutable text projection for the custom grid; avoids allocating empty worksheet cells.
    fn sparse_table(&self)->Option<SparseTable>{None}
    fn table(&self) -> Option<Vec<Vec<String>>> {
        None
    }
    fn select_cell(&mut self, _row: usize, _col: usize) {}
    fn selected_cell(&self) -> Option<(usize, usize)> {
        None
    }
    fn image_zoom(&self) -> Option<f64> {
        None
    }
    fn can_save(&self) -> bool {
        true
    }
    fn image(&self) -> Option<(u32, u32, Vec<u8>)> {
        None
    }
    fn list_visible(&self) -> bool {
        true
    }
    /// Page and paragraph ruler geometry, in twips; None disables rulers.
    fn rich_ruler(&self) -> Option<RichRuler> {
        None
    }
    fn rich_ruler_action(&mut self, _action: RichRulerAction) -> Result<(), String> {
        Err("Ruler action unavailable".into())
    }
    fn uses_rich_edit(&self) -> bool {
        false
    }
    fn rich_text(&self) -> Option<Vec<u8>> {
        None
    }
    fn edit_rich_text(
        &mut self,
        _rtf: &[u8],
        _selection_utf16: (usize, usize),
    ) -> Result<(), String> {
        Err("Rich text editing unavailable".into())
    }
    /// Prepare a word correction without importing native fallback fonts or changing live document history.
    fn prepare_rich_word(&mut self,_native:&[u16],_range:(usize,usize),_replacement:&str)->Result<Vec<u8>,String>{
        Err("Word correction unavailable".into())
    }
    /// Commit only after the host loads and validates the prepared projection; false discards it.
    fn finish_rich_word(&mut self,_commit:bool)->Result<(),String>{Err("Word correction unavailable".into())}
    fn set_rich_selection(&mut self, _selection_utf16: (usize, usize)) {}
    /// Selection captured by the document history, restored after Undo or Redo.
    /// Select an app-requested rich-text range after a command refresh.
    fn take_requested_rich_selection(&mut self) -> Option<(usize, usize)> {
        None
    }
    fn rich_selection_to_restore(&self) -> Option<(usize, usize)> {
        None
    }
    fn text_readonly(&self) -> bool {
        false
    }
}
#[cfg(windows)]
mod clipboard;
#[cfg(any(windows, test))]
mod clipboard_envelope;
#[cfg(windows)]
mod dialogs;
#[cfg(windows)]
mod grid;
#[cfg(any(windows, test))]
mod grid_geometry;
#[cfg(any(windows, test))]
mod label_preferences;
#[cfg(any(windows, test))]
mod marks;
#[cfg(any(windows, test))]
mod recents;
#[cfg(any(windows, test))]
mod window_layout;
#[cfg(windows)]
mod native;
#[cfg(windows)]
mod report;
#[cfg(any(windows, test))]
mod note_pagination;
#[cfg(any(windows, test))]
mod print_calibration;
#[cfg(any(windows, test))]
mod production;
#[cfg(windows)]
mod prompt;
#[cfg(windows)]
mod rich_edit;
#[cfg(windows)]
mod rich_format;
#[cfg(windows)]
mod rich_notes;
#[cfg(windows)]
mod rich_preview;
#[cfg(windows)]
mod rich_artwork;
#[cfg(windows)]
mod rich_print;
#[cfg(windows)]
mod rich_ruler;
#[cfg(windows)]
mod scene_transform;
#[cfg(windows)]
pub use native::run;
#[cfg(not(windows))]
pub fn run(app: Box<dyn App>) {
    eprintln!("{} requires Windows.", app.name());
}

/// Convert between a model string and the CRLF representation used by EDIT.
/// Mixed newlines, lone CR and NUL cannot be represented reversibly here.
pub fn validate_editable(text: &str) -> Result<(), String> {
    let canonical = text.replace("\r\n", "\n");
    if text.contains('\0')
        || canonical.contains('\r')
        || (text.contains("\r\n") && text.replace("\r\n", "").contains('\n'))
    {
        return Err("Text contains NUL, lone CR, or mixed newline styles that this editor cannot preserve. File was not opened.".into());
    }
    if canonical.encode_utf16().count() + canonical.matches('\n').count() > 16_777_216 {
        return Err("Text exceeds the native editor limit. File was not opened.".into());
    }
    Ok(())
}
pub fn to_native_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}
pub fn from_native_text(text: &str) -> String {
    text.replace("\r\n", "\n")
}
#[cfg(test)]
mod tests {
    #[test]
    fn a4_page_fits_or_reflows_onto_letter_paper() {
        use super::PrintGeometry;
        let a4 = PrintGeometry {
            width_twips: 11906,
            height_twips: 16838,
            margin_left_twips: 1440,
            margin_top_twips: 1440,
            margin_right_twips: 1440,
            margin_bottom_twips: 1440,
        };
        let letter = PrintGeometry {
            width_twips: 12240,
            height_twips: 15840,
            margin_left_twips: 0,
            margin_top_twips: 0,
            margin_right_twips: 0,
            margin_bottom_twips: 0,
        };
        assert!(a4.fits_paper(a4));
        assert!(!a4.fits_paper(letter));
        let reflowed = a4.reflowed_onto(PrintGeometry {
            margin_left_twips: 2000,
            ..letter
        });
        assert_eq!((reflowed.width_twips, reflowed.height_twips), (12240, 15840));
        assert_eq!((reflowed.margin_left_twips, reflowed.margin_top_twips), (2000, 1440));
        assert!(reflowed.valid());
    }
    #[test]
    fn print_content_keeps_place_when_it_fits_and_shrinks_centred_otherwise() {
        assert_eq!(
            super::fit_print_content((500.0, 400.0), (100, 50), (1000, 1000)),
            (1.0, 100, 50)
        );
        // 16:9 slide wider than the printable width: scaled to the width, centred vertically.
        let (s, x, y) = super::fit_print_content((2000.0, 1125.0), (0, 0), (1000, 1300));
        assert!((s - 0.5).abs() < 1e-9);
        assert_eq!((x, y), (0, 369));
        // Never enlarged; moved inside when its preferred origin is off the printable area.
        assert_eq!(
            super::fit_print_content((300.0, 300.0), (-20, 0), (1000, 1000)),
            (1.0, 350, 350)
        );
    }
    #[test]
    fn native_text_round_trip_and_rejections() {
        for s in ["first\nsecond 😀", "first\r\nsecond 😀", "one line"] {
            assert!(super::validate_editable(s).is_ok());
            assert_eq!(
                super::from_native_text(&super::to_native_text(s)),
                s.replace("\r\n", "\n")
            );
        }
        assert!(super::validate_editable("a\0b").is_err());
        assert!(super::validate_editable("a\rb").is_err());
        assert!(super::validate_editable("a\r\nb\nc").is_err());
        assert!(super::validate_editable(&"x".repeat(16_777_217)).is_err());
    }
}

/// Native table operations needed to move between two rectangular presentations.
#[derive(Debug, PartialEq, Eq)]
pub enum GridTransition {
    Rebuild { selection: Option<(usize, usize)> },
    Update { cells: Vec<(usize, usize)> },
}
/// Compare presentations once, leaving unchanged native controls and view state intact.
pub fn grid_transition(
    old: Option<&[Vec<String>]>,
    new: &[Vec<String>],
    selection: Option<(usize, usize)>,
) -> GridTransition {
    let cols = new.first().map_or(0, Vec::len);
    let shape_matches = old.is_some_and(|rows| {
        rows.len() == new.len() && rows.iter().zip(new).all(|(a, b)| a.len() == b.len())
    });
    if !shape_matches {
        let selection = selection.and_then(|(row, col)| {
            if new.is_empty() || cols == 0 {
                None
            } else {
                Some((row.min(new.len() - 1), col.min(cols - 1)))
            }
        });
        return GridTransition::Rebuild { selection };
    }
    let old = old.unwrap();
    let cells = old
        .iter()
        .zip(new)
        .enumerate()
        .flat_map(|(r, (a, b))| {
            a.iter()
                .zip(b)
                .enumerate()
                .filter_map(move |(c, (x, y))| (x != y).then_some((r, c)))
        })
        .collect();
    GridTransition::Update { cells }
}
#[cfg(test)]
mod grid_tests {
    use super::*;
    #[test]
    fn selection_events_keep_the_model_authoritative() {
        assert_eq!(
            grid_selection_event(Some((4, 3)), Some(5), None, false),
            Some((5, 3))
        );
        assert_eq!(
            grid_selection_event(Some((4, 3)), Some(4), Some(1), false),
            Some((4, 1))
        );
        assert_eq!(grid_selection_event(Some((4, 3)), None, None, false), None);
        assert_eq!(
            grid_selection_event(Some((4, 3)), Some(4), None, false),
            None
        );
        assert_eq!(
            grid_selection_event(Some((4, 3)), Some(1), None, true),
            None
        );
    }
    #[test]
    fn sparse_changes_and_structure_selection() {
        let old = vec![vec![String::new(); 100]; 1000];
        let mut next = old.clone();
        next[0][0] = "1".into();
        next[999][99] = "2".into();
        assert_eq!(
            grid_transition(Some(&old), &next, Some((999, 99))),
            GridTransition::Update {
                cells: vec![(0, 0), (999, 99)]
            }
        );
        next.truncate(3);
        next.iter_mut().for_each(|r| r.truncate(2));
        assert_eq!(
            grid_transition(Some(&old), &next, Some((999, 99))),
            GridTransition::Rebuild {
                selection: Some((2, 1))
            }
        );
        assert_eq!(
            grid_transition(Some(&old), &[], Some((0, 0))),
            GridTransition::Rebuild { selection: None }
        );
    }
}

/// Resolve a selected native row or clicked cell against the current model selection.
/// Deselect-only and programmatic notifications are ignored.
pub fn grid_selection_event(
    current: Option<(usize, usize)>,
    row: Option<usize>,
    clicked_col: Option<usize>,
    programmatic: bool,
) -> Option<(usize, usize)> {
    if programmatic {
        return None;
    }
    let row = row?;
    let col = clicked_col.or_else(|| current.map(|(_, col)| col))?;
    let next = (row, col);
    (Some(next) != current).then_some(next)
}

/// Format offered by an Open, Save As, Import or Export dialog.
#[derive(Clone, Debug)]
pub struct FileFormat {
    pub id: &'static str,
    pub title: &'static str,
    pub extensions: &'static [&'static str],
}
/// One definition shared by menus, toolbars and accelerators.
#[derive(Clone, Debug)]
pub struct CommandSpec {
    pub id: u16,
    pub title: &'static str,
    pub accelerator: Option<&'static str>,
    pub group: &'static str,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CommandState {
    pub enabled: bool,
    pub checked: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Viewport {
    pub zoom: f64,
    pub pan: Point,
}
impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Point::default(),
        }
    }
}
impl Viewport {
    pub fn document_to_view(self, p: Point) -> Point {
        Point {
            x: p.x * self.zoom + self.pan.x,
            y: p.y * self.zoom + self.pan.y,
        }
    }
    pub fn view_to_document(self, p: Point) -> Option<Point> {
        if !self.zoom.is_finite() || self.zoom <= 0.0 {
            return None;
        }
        Some(Point {
            x: (p.x - self.pan.x) / self.zoom,
            y: (p.y - self.pan.y) / self.zoom,
        })
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CanvasEvent {
    PointerDown(Point),
    PointerDownModified { point: Point, modifiers: u32 },
    DoubleClick(Point),
    PointerMove(Point),
    PointerUp(Point),
    Cancel,
    Key { code: u32, modifiers: u32 },
}

/// Device-independent scene in document coordinates. Colors are RGB bytes.
#[derive(Clone, Debug)]
pub enum SceneItem {
    /// Composite a bounded opaque scene over its destination at constant opacity.
    OpacityLayer {
        bounds: CanvasRect,
        opacity: u8,
        items: Vec<SceneItem>,
    },
    /// Clip to a logical rectangle, translating all nested items by offset.
    ClippedOffset {
        clip: CanvasRect,
        offset: Point,
        items: Vec<SceneItem>,
    },
    /// Rotate nested scene items clockwise around `center` in logical scene units.
    Transform {
        center: Point,
        degrees: f64,
        items: Vec<SceneItem>,
    },
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill: [u8; 3],
        stroke: [u8; 3],
    },
    Ellipse {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        fill: Option<[u8; 3]>,
        stroke: Option<[u8; 3]>,
    },
    Text {
        x: f64,
        y: f64,
        text: String,
        color: [u8; 3],
    },
    TextBox {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        text: String,
        color: [u8; 3],
        font: SceneFont,
        align: TextAlign,
        wrap: bool,
    },
    RichText {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        paragraphs: Vec<SceneParagraph>,
    },
    VectorPath {
        segments: Vec<PathSegment>,
        fill: Option<[u8; 3]>,
        stroke: Option<[u8; 3]>,
    },
    Path {
        points: Vec<Point>,
        color: [u8; 3],
        closed: bool,
    },
    Image {
        x: f64,
        y: f64,
        width: u32,
        height: u32,
        rgba: Vec<u8>,
    },
    /// Pixel dimensions are independent from the destination rectangle in scene units.
    ImageScaled {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        pixel_width: u32,
        pixel_height: u32,
        rgba: Vec<u8>,
    },
}
/// Curves remain vectors at screen and printer resolution. Compound paths use even-odd filling.
#[derive(Clone, Debug)]
pub enum PathSegment {
    Move(Point),
    Line(Point),
    Cubic(Point, Point, Point),
    Close,
}
impl PathSegment {
    pub fn map_points(&mut self, mut f: impl FnMut(Point) -> Point) {
        match self {
            Self::Move(p) | Self::Line(p) => *p = f(*p),
            Self::Cubic(a, b, c) => {
                *a = f(*a);
                *b = f(*b);
                *c = f(*c);
            }
            Self::Close => {}
        }
    }
}
#[derive(Clone, Debug)]
pub struct SceneFont {
    pub family: String,
    pub points: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}
#[derive(Clone, Debug)]
pub struct SceneRun {
    pub text: String,
    pub font: SceneFont,
    pub color: [u8; 3],
}
#[derive(Clone, Debug)]
pub struct SceneParagraph {
    pub runs: Vec<SceneRun>,
    pub align: TextAlign,
    pub bullet: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParagraphRuler {
    pub left_twips: i32,
    pub right_twips: i32,
    pub first_line_twips: i32,
    pub tabs_twips: Vec<i32>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RichRuler {
    pub page: PrintGeometry,
    pub zoom_percent: u16,
    pub paragraph: Option<ParagraphRuler>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichRulerAction {
    LeftMargin(i32),
    RightMargin(i32),
    TopMargin(i32),
    BottomMargin(i32),
    LeftIndent(i32),
    RightIndent(i32),
    FirstIndent(i32),
    AddTab(i32),
    MoveTab { from: i32, to: i32 },
    RemoveTab(i32),
}
/// Paper dimensions and margins in twips (1/1440 inch), independent of printer DPI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PrintGeometry {
    pub width_twips: i32,
    pub height_twips: i32,
    pub margin_left_twips: i32,
    pub margin_top_twips: i32,
    pub margin_right_twips: i32,
    pub margin_bottom_twips: i32,
}
impl PrintGeometry {
    pub fn valid(self) -> bool {
        self.width_twips > 0
            && self.height_twips > 0
            && [
                self.margin_left_twips,
                self.margin_top_twips,
                self.margin_right_twips,
                self.margin_bottom_twips,
            ]
            .iter()
            .all(|x| *x >= 0)
            && i64::from(self.margin_left_twips) + i64::from(self.margin_right_twips)
                < i64::from(self.width_twips)
            && i64::from(self.margin_top_twips) + i64::from(self.margin_bottom_twips)
                < i64::from(self.height_twips)
    }
    /// Whether this page prints unchanged on `paper` (the printer's sheet, with its
    /// unprintable edges as margins): same size within 60 twips, margins outside those edges.
    pub fn fits_paper(self, paper: PrintGeometry) -> bool {
        (self.width_twips - paper.width_twips).abs() <= 60
            && (self.height_twips - paper.height_twips).abs() <= 60
            && self.margin_left_twips >= paper.margin_left_twips
            && self.margin_top_twips >= paper.margin_top_twips
            && self.margin_right_twips >= paper.margin_right_twips
            && self.margin_bottom_twips >= paper.margin_bottom_twips
    }
    /// This page reflowed onto `paper`: the paper's size, keeping this page's margins
    /// but never inside the paper's unprintable edges.
    pub fn reflowed_onto(self, paper: PrintGeometry) -> PrintGeometry {
        PrintGeometry {
            width_twips: paper.width_twips,
            height_twips: paper.height_twips,
            margin_left_twips: self.margin_left_twips.max(paper.margin_left_twips),
            margin_top_twips: self.margin_top_twips.max(paper.margin_top_twips),
            margin_right_twips: self.margin_right_twips.max(paper.margin_right_twips),
            margin_bottom_twips: self.margin_bottom_twips.max(paper.margin_bottom_twips),
        }
    }
}
/// Scale (at most 1) and origin placing content of `content` device pixels in a printable
/// area of `area` pixels: unchanged at `preferred` when it fits there, otherwise shrunk
/// to fit and centred, keeping its aspect ratio.
pub fn fit_print_content(
    content: (f64, f64),
    preferred: (i32, i32),
    area: (i32, i32),
) -> (f64, i32, i32) {
    let (w, h) = (content.0.max(1.0), content.1.max(1.0));
    let (aw, ah) = (f64::from(area.0.max(1)), f64::from(area.1.max(1)));
    let (px, py) = (f64::from(preferred.0), f64::from(preferred.1));
    if px >= 0.0 && py >= 0.0 && px + w <= aw + 0.5 && py + h <= ah + 0.5 {
        return (1.0, preferred.0, preferred.1);
    }
    let scale = (aw / w).min(ah / h).min(1.0);
    let x = ((aw - w * scale) / 2.0).round() as i32;
    let y = ((ah - h * scale) / 2.0).round() as i32;
    (scale, x.max(0), y.max(0))
}
#[derive(Clone, Debug)]
pub struct PrintPage {
    pub geometry: PrintGeometry,
    pub scene: CanvasScene,
}
#[derive(Clone, Debug)]
pub enum PrintText {
    Literal(String),
    PageNumber,
    PageCount,
}
#[derive(Clone, Debug)]
pub struct PrintFragment {
    pub text: PrintText,
    pub font: SceneFont,
    pub color: [u8; 3],
}
#[derive(Clone, Debug)]
pub struct PrintParagraph {
    pub align: TextAlign,
    pub fragments: Vec<PrintFragment>,
}
#[derive(Clone, Debug)]
pub struct RichPrintNote {
    pub marker: String,
    pub label: String,
    pub endnote: bool,
    pub paragraphs: Vec<PrintParagraph>,
}
#[derive(Clone, Debug)]
pub struct RichPrint {
    pub geometry: PrintGeometry,
    pub header_distance_twips: i32,
    pub footer_distance_twips: i32,
    pub header: Vec<PrintParagraph>,
    pub footer: Vec<PrintParagraph>,
    /// Optional print-only RTF body with note markers; never an editing authority.
    pub body_rtf: Option<Vec<u8>>,
    pub notes: Vec<RichPrintNote>,
}
#[derive(Clone, Debug)]
pub enum PrintJob {
    /// Reuse immutable PDF bytes, rendering each page directly to the selected printer DC.
    ActualSizePdf(std::sync::Arc<[u8]>),
    /// Center on the physical sheet and shrink oversized pages, retaining native PDF paths.
    FitPdf(std::sync::Arc<[u8]>),
    Scenes(Vec<PrintPage>),
    ActualSizeScenes(Vec<PrintPage>),
    RichText(RichPrint),
}
#[derive(Clone, Debug, Default)]
pub struct CanvasScene {
    pub viewport: Viewport,
    pub items: Vec<SceneItem>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridRange {
    pub top: usize,
    pub left: usize,
    pub bottom: usize,
    pub right: usize,
}
#[derive(Clone)]
pub struct GridPicture {
    pub selected: bool,
    pub id: u64,
    pub row: usize,
    pub column: usize,
    pub offset_x: f64,
    pub offset_y: f64,
    pub width: f64,
    pub height: f64,
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: std::sync::Arc<[u8]>,
}

#[derive(Clone, Debug, Default)]
pub struct GridCellStyle {
    pub foreground: Option<[u8; 3]>,
    pub background: Option<[u8; 3]>,
    pub bold: bool,
    pub italic: bool,
    pub merged: Option<GridRange>,
    pub borders: u8,
}

#[derive(Clone, Debug)]
pub struct FontChoice {
    pub family: String,
    pub points: f64,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub color: [u8; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RichFormatCommand { Bold, Italic, Underline, Font, Color, AlignLeft, AlignCenter, AlignRight, Justify }
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
#[derive(Clone, Debug)]
pub enum DialogRequest {
    Confirmation { title: &'static str, text: String },
    Report(ReportRequest),
    CombineFiles{input:FileAction,output:FileAction},
    BatchFolder {
        input: FileAction,
        title: &'static str,
    },
    Information { title: &'static str, text: String },
    Color([u8; 3]),
    Font,
    PageSetup(PageMargins),
}
#[derive(Clone, Debug)]
pub enum DialogResult {
    Confirmed(bool),
    Files{inputs:Vec<std::path::PathBuf>,output:std::path::PathBuf},
    Color([u8; 3]),
    Font(FontChoice),
    PageSetup(PageMargins),
}

#[derive(Clone, Debug)]
pub enum ClipboardRequest {
    ReadText,
    WriteText(String),
    ReadData(String),
    WriteData {
        format: String,
        bytes: Vec<u8>,
    },
    WriteDataAndText {
        format: String,
        bytes: Vec<u8>,
        text: String,
    },
    ReadDataOrText(String),
}

/// Paper and margins in hundredths of a millimetre.
#[derive(Clone, Copy, Debug, Default)]
pub struct PageMargins {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub paper_width: i32,
    pub paper_height: i32,
}

#[cfg(test)]
mod app_command_namespace_tests{
    #[test]
    fn host_menus_and_control_notifications_are_reserved(){
        for id in[0,1,4,9,100,101,102,62000,64000,65507]{assert!(!super::application_command_id(id),"{id}");}
        for id in[10,18,97,110,111,200,360]{assert!(super::application_command_id(id),"{id}");}
    }
}

/// Explorer's natural filename ordering for numbered PDF batches.
pub fn logical_file_order(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> std::cmp::Ordering {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let a: Vec<_> = a.encode_wide().chain(Some(0)).collect();
        let b: Vec<_> = b.encode_wide().chain(Some(0)).collect();
        return unsafe { windows_sys::Win32::UI::Shell::StrCmpLogicalW(a.as_ptr(), b.as_ptr()) }
            .cmp(&0);
    }
    #[cfg(not(windows))]
    {
        // Portable test runner: compare digit runs without fixed-width integer overflow.
        let (a, b) = (
            a.to_string_lossy().to_lowercase(),
            b.to_string_lossy().to_lowercase(),
        );
        let (a, b) = (a.as_bytes(), b.as_bytes());
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
                let (start, end) = (i, j);
                while i < a.len() && a[i].is_ascii_digit() {
                    i += 1;
                }
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
                let (mut x, mut y) = (start, end);
                while x < i && a[x] == b'0' {
                    x += 1;
                }
                while y < j && b[y] == b'0' {
                    y += 1;
                }
                let order = (i - x).cmp(&(j - y)).then_with(|| a[x..i].cmp(&b[y..j]));
                if !order.is_eq() {
                    return order;
                }
            } else {
                let order = a[i].cmp(&b[j]);
                if !order.is_eq() {
                    return order;
                }
                i += 1;
                j += 1;
            }
        }
        (a.len() - i).cmp(&(b.len() - j))
    }
}
