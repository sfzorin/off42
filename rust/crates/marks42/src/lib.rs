//! Generated handwritten signatures and stamp impressions for the Windows apps.
//!
//! A port of the macOS Kit42 generators (`SignatureGenerator`, `PenAlphabet`, `Kinematics`, `PenRenderer`,
//! `InkRendering`, `SignatureWarp`, `SealGenerator`, `SealCatalog`). Everything here is platform-independent and
//! renders straight to RGBA pixels with tiny-skia, so it is tested on any host and shipped inside the Windows apps.
//! The same seeds give the same designs as on macOS wherever the algorithms are shared.

pub mod ink;
pub mod rng;
pub mod seal;
pub mod signature;
pub mod texture;

pub use ink::{Ink, InkColor};
pub use rng::SeededRandom;

/// Encodes an RGBA image as PNG (the shared signature and stamp library stores PNG files).
pub fn png(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(out.into_inner())
}
