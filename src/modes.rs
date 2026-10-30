//! Demo modes. The order is also the order used by the UP/DOWN buttons.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Mode {
    /// Live camera preview.
    Camera = 0,
    /// Live preview plus frame-difference motion indicator.
    Motion = 1,
    /// Live preview plus a skin-colour face candidate box.
    Face = 2,
    /// Static information screen; the camera is stopped.
    Standby = 3,
}

impl Mode {
    pub const COUNT: u8 = 4;

    pub const fn from_u8(value: u8) -> Self {
        match value % Self::COUNT {
            0 => Self::Camera,
            1 => Self::Motion,
            2 => Self::Face,
            _ => Self::Standby,
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
            Self::Motion => "MOTION",
            Self::Face => "FACE",
            Self::Standby => "STANDBY",
        }
    }

    /// Accent colour (RGB565) for the mode indicator.
    pub const fn accent(self) -> u16 {
        match self {
            Self::Camera => 0x07E0,  // green
            Self::Motion => 0x07FF,  // cyan
            Self::Face => 0xFD20,    // orange
            Self::Standby => 0x8410, // grey
        }
    }
}
