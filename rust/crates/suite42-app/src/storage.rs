//! Per-user local storage root shared by preferences, recent documents, recovery and libraries.
use std::path::PathBuf;
/// `%LOCALAPPDATA%/off42`, the folder the Windows apps have always used. Shells for other platforms
/// choose their own root before relying on this (see the crate README).
pub fn local_data() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("off42"))
}
