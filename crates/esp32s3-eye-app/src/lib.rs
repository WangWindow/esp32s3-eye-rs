#![no_std]
//! ESP32-S3-EYE application library: UI, face/palm algorithms and app state.
//!
//! Hardware access is provided by `esp32s3_eye_board::Board`.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub w: u16,
    pub h: u16,
}

impl Rect {
    pub const fn new(x: u16, y: u16, w: u16, h: u16) -> Self {
        Self { x, y, w, h }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: u16,
    pub y: u16,
}

// Tiny 5×7 bitmap font for the 240×240 status/standby screens.
//
// Each glyph is seven rows, with the five pixels stored in bits 4..0.
// Unknown characters are rendered as a filled box.

pub const GLYPH_WIDTH: usize = 5;
pub const GLYPH_HEIGHT: usize = 7;
/// One pixel of spacing between characters.
pub const GLYPH_ADVANCE: usize = GLYPH_WIDTH + 1;

/// Return the seven rows of a glyph. Lower-case input is folded to upper-case.
pub const fn glyph(character: u8) -> [u8; GLYPH_HEIGHT] {
    let c = match character {
        b'a'..=b'z' => character - 32,
        _ => character,
    };

    match c {
        b'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        b'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        b'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        b'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        b'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01111,
        ],
        b'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        b'I' => [
            0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'J' => [
            0b00111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        b'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        b'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        b'M' => [
            0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001,
        ],
        b'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        b'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        b'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        b'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        b'S' => [
            0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        b'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        b'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        b'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b10101, 0b01010,
        ],
        b'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        b'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        b'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],

        b'0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        b'1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        b'2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        b'3' => [
            0b11111, 0b00010, 0b00100, 0b00010, 0b00001, 0b10001, 0b01110,
        ],
        b'4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        b'5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        b'6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        b'7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        b'8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        b'9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],

        b' ' => [0; 7],
        b':' => [
            0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000,
        ],
        b'-' => [
            0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000,
        ],
        b'.' => [
            0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00110, 0b00110,
        ],
        b'/' => [
            0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000,
        ],
        b'!' => [
            0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00100,
        ],
        b'?' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b00000, 0b00100,
        ],
        b'+' => [
            0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000,
        ],
        b'*' => [
            0b00000, 0b10101, 0b01110, 0b11111, 0b01110, 0b10101, 0b00000,
        ],
        b'%' => [
            0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}

/// Width in pixels of `text` at the given integer scale.
pub const fn text_width(text: &str, scale: usize) -> usize {
    let bytes = text.as_bytes();
    if bytes.is_empty() {
        return 0;
    }
    bytes.len() * GLYPH_ADVANCE * scale - scale
}

// Demo modes. Feature-gated algorithms determine which entries exist.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Live camera preview.
    Camera,
    /// Live preview plus a skin-colour face candidate box.
    #[cfg(feature = "face")]
    Face,
    /// Palm ROI preview and palmprint recognition.
    #[cfg(feature = "palm")]
    Palm,
    /// SD card information and read/write self-test.
    Storage,
    /// Static information screen; the camera is stopped.
    Standby,
}

impl Mode {
    pub const COUNT: u8 = 3 + cfg!(feature = "face") as u8 + cfg!(feature = "palm") as u8;

    pub const fn from_u8(value: u8) -> Self {
        let mut index = value % Self::COUNT;
        if index == 0 {
            return Self::Camera;
        }
        index -= 1;

        #[cfg(feature = "face")]
        {
            if index == 0 {
                return Self::Face;
            }
            index -= 1;
        }

        #[cfg(feature = "palm")]
        {
            if index == 0 {
                return Self::Palm;
            }
            index -= 1;
        }

        if index == 0 {
            Self::Storage
        } else {
            Self::Standby
        }
    }

    pub const fn next(self) -> Self {
        Self::from_u8(self as u8 + 1)
    }

    pub const fn previous(self) -> Self {
        Self::from_u8((self as u8 + Self::COUNT - 1) % Self::COUNT)
    }

    /// Short label shown in the status bar.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Camera => "CAM",
            #[cfg(feature = "face")]
            Self::Face => "FACE",
            #[cfg(feature = "palm")]
            Self::Palm => "PALM",
            Self::Storage => "SD",
            Self::Standby => "STANDBY",
        }
    }

    /// Accent colour (RGB565) for the mode indicator.
    pub const fn accent(self) -> u16 {
        match self {
            Self::Camera => 0x07E0, // green
            #[cfg(feature = "face")]
            Self::Face => 0xFD20, // orange
            #[cfg(feature = "palm")]
            Self::Palm => 0xF81F, // magenta
            Self::Storage => 0xFFE0, // yellow
            Self::Standby => 0x8410, // grey
        }
    }
}

pub mod app;
#[cfg(any(feature = "face", feature = "palm"))]
mod detect;
pub mod display;
#[cfg(feature = "face")]
pub mod face;
#[cfg(feature = "palm")]
pub mod palm;
