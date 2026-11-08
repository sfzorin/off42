//! Recognition output uses source pixel coordinates; adapters own document geometry and history.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
