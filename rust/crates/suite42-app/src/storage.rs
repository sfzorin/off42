//! Per-user local storage root shared by preferences, recent documents, recovery and libraries.
use std::path::PathBuf;
/// `%LOCALAPPDATA%/off42`, the folder the Windows apps have always used. `None` where `LOCALAPPDATA`
/// is unset; other platforms need their own root here (see the crate README).
pub fn local_data() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("off42"))
}
