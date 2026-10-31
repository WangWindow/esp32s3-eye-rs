//! ESP32-S3 LCD_CAM + GDMA DVP capture driver.
//!
//! The driver is sensor-agnostic: it receives a DVP byte stream configured by
//! the caller (RGB565, YUV422 or JPEG) and exposes a continuous DMA ring with
//! VSYNC/EOF-aligned frame assembly.

#![no_std]
#![allow(clippy::collapsible_if, clippy::unnecessary_cast)]

mod capture;
mod driver;

pub use capture::*;
pub use driver::*;
