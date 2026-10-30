#![no_std]
//! Board support for the Espressif / 源地工作室 ESP32-S3-EYE.
//!
//! The crate is split into small, reusable drivers:
//!
//! * [`display`] – ST7789 240×240 SPI panel and drawing helpers
//! * [`camera`] – OV2640 + LCD_CAM/DVP capture pipeline
//! * [`buttons`] – ADC-ladder buttons and the BOOT key
//! * [`graphics`] – tiny 5×7 bitmap font used by the on-screen UI
//! * [`modes`] – demo modes shared by the UI

pub mod board;
pub mod buttons;
pub mod camera;
pub mod display;
pub mod graphics;
pub mod modes;
