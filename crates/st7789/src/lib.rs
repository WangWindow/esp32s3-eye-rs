//! Generic ST7789 SPI display driver.
//!
//! This crate only knows the ST7789 controller: SPI command/data framing,
//! power-on sequence, address window and pixel writes. Board geometry, fonts,
//! overlays and camera cropping belong to the application.

#![no_std]

use embedded_hal::{delay::DelayNs, digital::OutputPin, spi::SpiBus};

/// Panel/controller configuration.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    pub width: u16,
    pub height: u16,
    /// The ESP32-S3-EYE backlight transistor is active-low.
    pub backlight_active_low: bool,
    /// Most 1.3" 240×240 panels need display inversion.
    pub invert: bool,
    /// MADCTL value (mirror/rotate/colour order).
    pub madctl: u8,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 240,
            height: 240,
            backlight_active_low: true,
            invert: true,
            madctl: 0x00,
        }
    }
}

pub struct St7789<SPI, CS, DC, BL, D> {
    spi: SPI,
    cs: CS,
    dc: DC,
    bl: BL,
    delay: D,
    config: Config,
}

impl<SPI, CS, DC, BL, D> St7789<SPI, CS, DC, BL, D>
where
    SPI: SpiBus<u8>,
    CS: OutputPin,
    DC: OutputPin,
    BL: OutputPin,
    D: DelayNs,
{
    pub fn new(spi: SPI, cs: CS, dc: DC, bl: BL, delay: D, config: Config) -> Self {
        Self {
            spi,
            cs,
            dc,
            bl,
            delay,
            config,
        }
    }

    pub fn config(&self) -> Config {
        self.config
    }

    /// Run the ST7789 power-on sequence and enable the backlight.
    pub fn init(&mut self) -> Result<(), SPI::Error> {
        let _ = self.cs.set_high();
        let _ = self.dc.set_high();
        self.delay.delay_ms(10);

        self.write_command(0x01, &[])?; // software reset
        self.delay.delay_ms(150);
        self.write_command(0x11, &[])?; // sleep out
        self.delay.delay_ms(120);

        // Porch / gate / VCOM / power / gamma for the common 1.3" 240×240 panel.
        self.write_command(0xB2, &[0x0C, 0x0C, 0x00, 0x33, 0x33])?;
        self.write_command(0xB7, &[0x35])?;
        self.write_command(0xBB, &[0x19])?;
        self.write_command(0xC0, &[0x2C])?;
        self.write_command(0xC2, &[0x01])?;
        self.write_command(0xC3, &[0x12])?;
        self.write_command(0xC4, &[0x20])?;
        self.write_command(0xC6, &[0x0F])?;
        self.write_command(0xD0, &[0xA4, 0xA1])?;
        self.write_command(
            0xE0,
            &[
                0xD0, 0x04, 0x0D, 0x11, 0x13, 0x2B, 0x3F, 0x54, 0x4C, 0x18, 0x0D, 0x0B, 0x1F, 0x23,
            ],
        )?;
        self.write_command(
            0xE1,
            &[
                0xD0, 0x04, 0x0C, 0x11, 0x13, 0x2C, 0x3F, 0x44, 0x51, 0x2F, 0x1F, 0x1F, 0x20, 0x23,
            ],
        )?;

        self.write_command(0x3A, &[0x55])?; // 16 bits/pixel
        self.write_command(0x36, &[self.config.madctl])?;
        if self.config.invert {
            self.write_command(0x21, &[])?;
        }
        self.write_command(0x13, &[])?; // normal display mode
        self.write_command(0x29, &[])?; // display on
        self.delay.delay_ms(20);

        self.set_backlight(true);
        Ok(())
    }

    pub fn set_backlight(&mut self, on: bool) {
        let level_high = if self.config.backlight_active_low {
            !on
        } else {
            on
        };
        let _ = if level_high {
            self.bl.set_high()
        } else {
            self.bl.set_low()
        };
    }

    /// Set the pixel window. Coordinates are inclusive in the ST7789 protocol.
    pub fn set_window(&mut self, x: u16, y: u16, w: u16, h: u16) -> Result<(), SPI::Error> {
        let x1 = x + w - 1;
        let y1 = y + h - 1;
        self.write_command(
            0x2A,
            &[
                x.to_be_bytes()[0],
                x.to_be_bytes()[1],
                x1.to_be_bytes()[0],
                x1.to_be_bytes()[1],
            ],
        )?;
        self.write_command(
            0x2B,
            &[
                y.to_be_bytes()[0],
                y.to_be_bytes()[1],
                y1.to_be_bytes()[0],
                y1.to_be_bytes()[1],
            ],
        )
    }

    /// Start a pixel stream into the current window.
    pub fn begin_pixels(&mut self) -> Result<(), SPI::Error> {
        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        self.spi.write(&[0x2C])?;
        let _ = self.dc.set_high();
        Ok(())
    }

    /// Write big-endian RGB565 pixel data into the active stream.
    pub fn write_pixels(&mut self, data: &[u8]) -> Result<(), SPI::Error> {
        self.spi.write(data)
    }

    pub fn end_pixels(&mut self) {
        let _ = self.cs.set_high();
    }

    fn write_command(&mut self, command: u8, data: &[u8]) -> Result<(), SPI::Error> {
        let _ = self.cs.set_low();
        let _ = self.dc.set_low();
        self.spi.write(&[command])?;
        if !data.is_empty() {
            let _ = self.dc.set_high();
            self.spi.write(data)?;
        }
        let _ = self.cs.set_high();
        Ok(())
    }
}
