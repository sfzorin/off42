//! Windows location of the shared signature and stamp library.
pub use suite42_app::marks::*;
#[cfg(windows)]
use std::{fs, path::PathBuf};

#[cfg(windows)]
pub fn library_directory() -> Result<PathBuf, String> {
    let local = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Windows local app data folder is unavailable")?;
    let directory = local.join("off42").join("Signatures and Stamps");
    fs::create_dir_all(&directory).map_err(|e| format!("Could not open the library: {e}"))?;
    Ok(directory)
}

