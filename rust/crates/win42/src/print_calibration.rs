//! Windows location of printer calibration profiles; the correction model lives in suite42_app.
pub use suite42_app::print_calibration::*;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
pub fn directory() -> Result<PathBuf, String> {
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or("Windows local app data folder is unavailable")?;
    Ok(root.join("off42").join("Printer Calibration"))
}

