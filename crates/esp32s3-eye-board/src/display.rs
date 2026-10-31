//! Board ST7789 panel: SPI/pin setup and low-level drawing primitives.
//!
//! Fonts, overlays and the camera window layout live in the application.

use esp_hal::{Blocking, delay::Delay, gpio::Output, spi::master::Spi};
use st7789::{Config as PanelConfig, St7789};

use crate::{LCD_HEIGHT, LCD_ROW_BYTES, LCD_WIDTH};

pub struct Panel<'d> {
    lcd: St7789<Spi<'d, Blocking>, Output<'d>, Output<'d>, Output<'d>, Delay>,
    row: [u8; LCD_ROW_BYTES],
}

impl<'d> Panel<'d> {
    pub fn new(spi: Spi<'d, Blocking>, cs: Output<'d>, dc: Output<'d>, bl: Output<'d>) -> Self {
        Self {
            lcd: St7789::new(
                spi,
                cs,
                dc,
                bl,
                Delay::new(),
                PanelConfig {
                    width: LCD_WIDTH as u16,
                    height: LCD_HEIGHT as u16,
                    backlight_active_low: true,
                    invert: true,
                    madctl: 0x00,
                },
            ),
            row: [0; LCD_ROW_BYTES],
        }
    }

    pub fn init(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.lcd.init()
    }

    pub fn set_backlight(&mut self, on: bool) {
        self.lcd.set_backlight(on);
    }

    pub fn set_window(
        &mut self,
        x: u16,
        y: u16,
        w: u16,
        h: u16,
    ) -> Result<(), esp_hal::spi::Error> {
        self.lcd.set_window(x, y, w, h)
    }

    pub fn begin_pixels(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.lcd.begin_pixels()
    }

    pub fn write_pixels(&mut self, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.lcd.write_pixels(data)
    }

    pub fn end_pixels(&mut self) {
        self.lcd.end_pixels();
    }

    pub fn fill_screen(&mut self, color: u16) {
        self.fill_rect(0, 0, LCD_WIDTH as u16, LCD_HEIGHT as u16, color);
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16) {
        if w == 0 || h == 0 || x >= LCD_WIDTH as u16 || y >= LCD_HEIGHT as u16 {
            return;
        }
        let w = w.min(LCD_WIDTH as u16 - x);
        let h = h.min(LCD_HEIGHT as u16 - y);
        let pixel = color.to_be_bytes();
        for i in 0..w as usize {
            self.row[i * 2] = pixel[0];
            self.row[i * 2 + 1] = pixel[1];
        }

        self.set_window(x, y, w, h).ok();
        self.begin_pixels().ok();
        for _ in 0..h {
            self.lcd.write_pixels(&self.row[..w as usize * 2]).ok();
        }
        self.end_pixels();
    }

    pub fn draw_rect(&mut self, x: u16, y: u16, w: u16, h: u16, color: u16, thickness: u16) {
        if w == 0 || h == 0 {
            return;
        }
        let t = thickness.max(1);
        self.fill_rect(x, y, w, t, color);
        self.fill_rect(x, y + h.saturating_sub(t), w, t, color);
        self.fill_rect(x, y, t, h, color);
        self.fill_rect(x + w.saturating_sub(t), y, t, h, color);
    }
}
