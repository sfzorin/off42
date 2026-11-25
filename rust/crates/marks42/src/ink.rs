//! Ink colours offered for signatures and stamps (Kit42 `InkColor`), with the same sRGB values.

use serde::{Deserialize, Serialize};

/// An sRGB ink colour, components 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ink {
    pub r: f64,
    pub g: f64,
    pub b: f64,
}

impl Ink {
    pub const fn rgb(r: f64, g: f64, b: f64) -> Self {
        Self { r, g, b }
    }
    pub fn to_u8(self) -> [u8; 3] {
        [self.r, self.g, self.b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum InkColor {
    Blue,
    Violet,
    Black,
    Red,
    Green,
}

impl InkColor {
    pub const ALL: [InkColor; 5] = [Self::Blue, Self::Violet, Self::Black, Self::Red, Self::Green];

    pub fn title(self) -> &'static str {
        match self {
            Self::Blue => "Blue ink",
            Self::Violet => "Violet ink",
            Self::Black => "Black ink",
            Self::Red => "Red ink",
            Self::Green => "Green ink",
        }
    }

    /// Ballpoint pen colours (signatures).
    pub fn pen(self) -> Ink {
        match self {
            Self::Blue => Ink::rgb(0.10, 0.20, 0.56),
            Self::Violet => Ink::rgb(0.30, 0.16, 0.55),
            Self::Black => Ink::rgb(0.11, 0.11, 0.13),
            Self::Red => Ink::rgb(0.72, 0.10, 0.12),
            Self::Green => Ink::rgb(0.05, 0.40, 0.22),
        }
    }

    /// Stamp-pad colours (seals): lighter and more saturated than pen ink.
    pub fn stamp_pad(self) -> Ink {
        match self {
            Self::Blue => Ink::rgb(0.16, 0.31, 0.70),
            Self::Violet => Ink::rgb(0.38, 0.25, 0.66),
            Self::Black => Ink::rgb(0.15, 0.15, 0.17),
            Self::Red => Ink::rgb(0.80, 0.13, 0.15),
            Self::Green => Ink::rgb(0.10, 0.48, 0.27),
        }
    }
}
