//! Windows location of the shared recovery journal; the format and store live in `suite42_app`.
pub use suite42_app::recovery::*;
use std::path::PathBuf;
pub fn directory() -> Option<PathBuf> {
    Some(
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
            .join("off42")
            .join("Recovery"),
    )
}
