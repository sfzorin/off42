//! Portable application services for the off42 shells (Win32, egui/Linux, CLI, WebAssembly).
//!
//! Everything here is free of platform UI APIs. Shells own windows, dialogs, the clipboard and
//! platform storage locations; this crate owns the logic and data formats they share.
pub mod grid_axis;
pub mod grid_geometry;
pub mod host;
pub mod clipboard_envelope;
pub mod generator;
pub mod input;
pub mod label_preferences;
pub mod localization;
pub mod marks;
pub mod note_pagination;
pub mod pdf;
pub mod pdf_print_layout;
pub mod platform;
pub mod print_calibration;
pub mod production;
pub mod recents;
pub mod scan;
pub mod report;
pub mod rich_pages;
pub mod recovery;
pub mod storage;
pub mod window_layout;
pub mod sheet;
pub mod slides;
pub mod sparse_table;
pub use grid_axis::GridAxis;
pub use grid_geometry::GridRange;
pub use host::*;
pub use input::InputRequest;
pub use production::LabelSource;
pub use sparse_table::SparseTable;
