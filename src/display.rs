//! ST7789 240×240 SPI panel driver and the small UI used by the demo.

use esp_hal::{Blocking, delay::Delay, gpio::Output, spi::master::Spi};

use crate::{
    board::{
        CAM_ROW_BYTES, CAM_VFLIP, CAM_VIEW_HEIGHT, CAM_Y_OFFSET, LCD_HEIGHT, LCD_ROW_BYTES,
        LCD_WIDTH, STATUS_BAR_HEIGHT,
    },
    graphics::{GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_WIDTH, glyph, text_width},
    modes::Mode,
};

pub const BLACK: u16 = 0x0000;
pub const WHITE: u16 = 0xFFFF;
pub const BLUE: u16 = 0x001F;
pub const CYAN: u16 = 0x07FF;
pub const GREEN: u16 = 0x07E0;
pub const YELLOW: u16 = 0xFFE0;
pub const ORANGE: u16 = 0xFD20;
pub const GRAY: u16 = 0x8410;
pub const DARK_GRAY: u16 = 0x4208;

/// A rectangle in screen coordinates.
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

/// Overlays composited into each camera scanline.
#[derive(Clone, Copy, Debug, Default)]
pub struct Overlay {
    pub motion: bool,
    pub face: Option<Rect>,
}

/// Write the motion square / face rectangle directly into one RGB565 row.
fn apply_overlay(line: &mut [u8; LCD_ROW_BYTES], screen_y: usize, overlay: Overlay) {
    let mut put = |x: usize, color: u16| {
        if x < LCD_WIDTH {
            let bytes = color.to_be_bytes();
            line[x * 2] = bytes[0];
            line[x * 2 + 1] = bytes[1];
        }
    };

    if overlay.motion && (22..40).contains(&screen_y) {
        for x in 4..22 {
            put(x, BLUE);
        }
    }

    if let Some(rect) = overlay.face {
        let y = screen_y as u16;
        if y >= rect.y && y < rect.y.saturating_add(rect.h) {
            let left = rect.x as usize;
            let right = (rect.x as usize + rect.w as usize).min(LCD_WIDTH);
            let top_border = y < rect.y.saturating_add(2);
            let bottom_border = y.saturating_add(2) >= rect.y.saturating_add(rect.h);

            if top_border || bottom_border {
                for x in left..right {
                    put(x, BLUE);
                }
            } else {
                for x in left..(left + 2).min(right) {
                    put(x, BLUE);
                }
                for x in right.saturating_sub(2)..right {
                    put(x, BLUE);
                }
            }
        }
    }
}

/// Owns the LCD SPI bus and its three control pins.
pub struct Display<'d> {
    spi: Spi<'d, Blocking>,
    cs: Output<'d>,
    dc: Output<'d>,
    bl: Output<'d>,
    delay: Delay,
}

impl<'d> Display<'d> {
    pub fn new(spi: Spi<'d, Blocking>, cs: Output<'d>, dc: Output<'d>, bl: Output<'d>) -> Self {
        Self {
            spi,
            cs,
            dc,
            bl,
            delay: Delay::new(),
        }
    }

    /// Run the ST7789 power-on sequence and turn the backlight on.
    pub fn init(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.cs.set_high();
        self.dc.set_high();
        self.delay.delay_millis(10);

        // Software reset, then wait for the controller.
        self.write_command(0x01, &[])?;
        self.delay.delay_millis(150);

        self.write_command(0x11, &[])?; // Sleep out
        self.delay.delay_millis(120);

        // Porch / gate / VCOM / power / gamma settings for the 1.3" 240×240 panel.
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
        self.write_command(0x36, &[0x00])?; // no mirror, no swap
        self.write_command(0x21, &[])?; // display inversion on (BSP default)
        self.write_command(0x13, &[])?; // normal display mode
        self.write_command(0x29, &[])?; // display on
        self.delay.delay_millis(20);

        // The LCD backlight transistor is active-low on this board. esp-bsp
        // also inverts the LEDC duty (100 % brightness == 0 % high time).
        self.bl.set_low();
        Ok(())
    }

    pub fn set_backlight(&mut self, on: bool) {
        if on {
            self.bl.set_low();
        } else {
            self.bl.set_high();
        }
    }

    fn write_command(&mut self, command: u8, data: &[u8]) -> Result<(), esp_hal::spi::Error> {
        self.cs.set_low();
        self.dc.set_low();
        self.spi.write(&[command])?;
        if !data.is_empty() {
            self.dc.set_high();
            self.spi.write(data)?;
        }
        self.cs.set_high();
        Ok(())
    }

    fn set_window(&mut self, x: u16, y: u16, w: u16, h: u16) -> Result<(), esp_hal::spi::Error> {
        let x0 = x;
        let x1 = x + w - 1;
        let y0 = y;
        let y1 = y + h - 1;

        self.write_command(
            0x2A,
            &[
                x0.to_be_bytes()[0],
                x0.to_be_bytes()[1],
                x1.to_be_bytes()[0],
                x1.to_be_bytes()[1],
            ],
        )?;
        self.write_command(
            0x2B,
            &[
                y0.to_be_bytes()[0],
                y0.to_be_bytes()[1],
                y1.to_be_bytes()[0],
                y1.to_be_bytes()[1],
            ],
        )?;
        Ok(())
    }

    fn begin_pixels(&mut self) -> Result<(), esp_hal::spi::Error> {
        self.cs.set_low();
        self.dc.set_low();
        self.spi.write(&[0x2C])?; // RAMWR
        self.dc.set_high();
        Ok(())
    }

    fn end_pixels(&mut self) {
        self.cs.set_high();
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

        let mut row = [0u8; LCD_ROW_BYTES];
        let pixel = color.to_be_bytes();
        for i in 0..w as usize {
            row[i * 2] = pixel[0];
            row[i * 2 + 1] = pixel[1];
        }

        self.set_window(x, y, w, h).ok();
        self.begin_pixels().ok();
        for _ in 0..h {
            self.spi.write(&row[..w as usize * 2]).ok();
        }
        self.end_pixels();
    }

    pub fn draw_rect(&mut self, rect: Rect, color: u16, thickness: u16) {
        if rect.w == 0 || rect.h == 0 {
            return;
        }
        let t = thickness.max(1);
        self.fill_rect(rect.x, rect.y, rect.w, t, color);
        self.fill_rect(rect.x, rect.y + rect.h.saturating_sub(t), rect.w, t, color);
        self.fill_rect(rect.x, rect.y, t, rect.h, color);
        self.fill_rect(rect.x + rect.w.saturating_sub(t), rect.y, t, rect.h, color);
    }

    pub fn draw_char(&mut self, x: i16, y: i16, character: u8, fg: u16, bg: u16, scale: usize) {
        let scale = scale.max(1);
        let full_width = GLYPH_WIDTH * scale;
        let full_height = GLYPH_HEIGHT * scale;
        let mut buffer = [0u8; 1120]; // 20 × 28 pixels maximum

        let rows = glyph(character);
        for (row_index, bits) in rows.iter().enumerate() {
            for sy in 0..scale {
                let py = row_index * scale + sy;
                for col in 0..GLYPH_WIDTH {
                    let on = bits & (1 << (GLYPH_WIDTH - 1 - col)) != 0;
                    let color = if on { fg } else { bg }.to_be_bytes();
                    for sx in 0..scale {
                        let px = col * scale + sx;
                        let offset = (py * full_width + px) * 2;
                        buffer[offset] = color[0];
                        buffer[offset + 1] = color[1];
                    }
                }
            }
        }

        if x < 0 || y < 0 {
            return;
        }
        let x = x as u16;
        let y = y as u16;
        if x >= LCD_WIDTH as u16 || y >= LCD_HEIGHT as u16 {
            return;
        }
        let clip_width = (full_width as u16).min(LCD_WIDTH as u16 - x);
        let clip_height = (full_height as u16).min(LCD_HEIGHT as u16 - y);

        self.set_window(x, y, clip_width, clip_height).ok();
        self.begin_pixels().ok();
        // Re-pack the visible part of each row when the glyph is clipped.
        let mut row = [0u8; LCD_ROW_BYTES];
        for py in 0..clip_height as usize {
            let source = py * full_width * 2;
            row[..clip_width as usize * 2]
                .copy_from_slice(&buffer[source..source + clip_width as usize * 2]);
            self.spi.write(&row[..clip_width as usize * 2]).ok();
        }
        self.end_pixels();
    }

    pub fn draw_text(&mut self, x: i16, y: i16, text: &str, fg: u16, bg: u16, scale: usize) {
        let scale = scale.max(1);
        let advance = (GLYPH_ADVANCE * scale) as i16;
        for (index, character) in text.bytes().enumerate() {
            let cx = x + index as i16 * advance;
            if cx >= LCD_WIDTH as i16 {
                break;
            }
            self.draw_char(cx, y, character, fg, bg, scale);
        }
    }

    pub fn draw_text_centered(&mut self, y: i16, text: &str, fg: u16, bg: u16, scale: usize) {
        let width = text_width(text, scale) as i16;
        self.draw_text((LCD_WIDTH as i16 - width) / 2, y, text, fg, bg, scale);
    }

    /// Stream a QVGA YUV422 (YUYV) frame into the area below the status bar.
    ///
    /// The status bar is *not* touched. Motion/face overlays are composited
    /// into the same scanline before it is sent, so every pixel is written
    /// exactly once per frame and there is no separate overlay flash.
    ///
    /// `line` must be [`LCD_ROW_BYTES`] long. The conversion is done one row at
    /// a time, so the frame buffer can live in the camera DMA path without a
    /// second full-size RGB565 buffer.
    pub fn draw_yuv422_frame(
        &mut self,
        frame: &[u8],
        line: &mut [u8; LCD_ROW_BYTES],
        overlay: Overlay,
    ) {
        self.set_window(
            0,
            STATUS_BAR_HEIGHT as u16,
            LCD_WIDTH as u16,
            CAM_VIEW_HEIGHT as u16,
        )
        .ok();
        self.begin_pixels().ok();

        for screen_y in STATUS_BAR_HEIGHT..LCD_HEIGHT {
            let view_row = screen_y - STATUS_BAR_HEIGHT;
            let source_y = if CAM_VFLIP {
                CAM_Y_OFFSET + (CAM_VIEW_HEIGHT - 1 - view_row)
            } else {
                CAM_Y_OFFSET + view_row
            };
            let row_start = source_y * CAM_ROW_BYTES + crate::board::CROP_OFFSET_BYTES;
            let src = &frame[row_start..row_start + LCD_ROW_BYTES];

            // 240 pixels = 120 YUYV pairs.
            for pair in 0..(LCD_WIDTH / 2) {
                let i = pair * 4;
                let (y0, cb, y1, cr) = (src[i], src[i + 1], src[i + 2], src[i + 3]);
                let p0 = yuv_to_rgb565(y0, cb, cr).to_be_bytes();
                let p1 = yuv_to_rgb565(y1, cb, cr).to_be_bytes();
                line[i] = p0[0];
                line[i + 1] = p0[1];
                line[i + 2] = p1[0];
                line[i + 3] = p1[1];
            }

            apply_overlay(line, screen_y, overlay);
            self.spi.write(&line[..]).ok();
        }

        self.end_pixels();
    }

    /// Draw the status bar text/accent. Only needed at startup and on a mode
    /// change; the camera preview never writes rows in this area.
    pub fn draw_status_bar(&mut self, mode: Mode, fps: u32) {
        self.fill_rect(0, 0, LCD_WIDTH as u16, STATUS_BAR_HEIGHT as u16, BLACK);
        self.fill_rect(3, 4, 4, 10, mode.accent());
        self.draw_text(11, 3, mode.label(), WHITE, BLACK, 1);
        self.update_fps(fps);
    }

    /// Repaint only the fixed-width FPS field, at most once per second.
    ///
    /// The text is always six characters wide (`"  8FPS"`), so drawing it
    /// directly over the previous value also clears old digits. No rectangle
    /// erase is needed, which avoids a once-per-second blink.
    pub fn update_fps(&mut self, fps: u32) {
        let mut text = [0u8; 6];
        let text = format_fps(fps, &mut text);
        let width = text_width(text, 1) as u16;
        self.draw_text(LCD_WIDTH as i16 - width as i16 - 4, 3, text, CYAN, BLACK, 1);
    }

    /// Draw the pause badge once. The next camera frame overwrites it.
    pub fn show_pause(&mut self) {
        self.fill_rect(80, 100, 80, 24, BLACK);
        self.draw_rect(Rect::new(80, 100, 80, 24), YELLOW, 1);
        self.draw_text_centered(109, "PAUSE", YELLOW, BLACK, 1);
    }

    pub fn show_standby(&mut self) {
        self.fill_screen(BLACK);

        // Stylised eye logo.
        self.fill_rect(70, 30, 100, 16, BLUE);
        self.fill_rect(82, 24, 76, 4, BLUE);
        self.fill_rect(82, 48, 76, 4, BLUE);
        self.fill_rect(105, 30, 30, 16, WHITE);
        self.fill_rect(112, 34, 16, 8, BLACK);
        self.fill_rect(116, 36, 8, 4, CYAN);

        self.draw_text_centered(66, "ESP32-S3-EYE", WHITE, BLACK, 2);
        self.draw_text_centered(88, "RUST + EMBASSY", CYAN, BLACK, 1);
        self.draw_text_centered(100, "OV2640 / ST7789", GRAY, BLACK, 1);

        self.fill_rect(40, 122, 160, 1, DARK_GRAY);

        self.draw_text_centered(134, "UP / DN   MODE", WHITE, BLACK, 1);
        self.draw_text_centered(148, "PLAY      PAUSE", WHITE, BLACK, 1);
        self.draw_text_centered(162, "BOOT      NEXT", WHITE, BLACK, 1);
        self.draw_text_centered(188, "PRESS A KEY", YELLOW, BLACK, 1);
    }

    pub fn show_error(&mut self, message: &str) {
        self.fill_screen(BLACK);
        self.draw_text_centered(70, "CAMERA ERROR", 0xF800, BLACK, 2);
        self.draw_text_centered(110, message, WHITE, BLACK, 1);
        self.draw_text_centered(150, "CHECK OV2640 / I2C", GRAY, BLACK, 1);
    }
}

#[inline]
fn yuv_to_rgb565(y: u8, cb: u8, cr: u8) -> u16 {
    let c = y as i32 - 16;
    let d = cb as i32 - 128;
    let e = cr as i32 - 128;

    let r = clamp_u8((298 * c + 409 * e + 128) >> 8);
    let g = clamp_u8((298 * c - 100 * d - 208 * e + 128) >> 8);
    let b = clamp_u8((298 * c + 516 * d + 128) >> 8);

    ((r as u16 & 0xF8) << 8) | ((g as u16 & 0xFC) << 3) | (b as u16 >> 3)
}

#[inline]
fn clamp_u8(value: i32) -> u8 {
    if value < 0 {
        0
    } else if value > 255 {
        255
    } else {
        value as u8
    }
}

/// Format 0..=999 as a fixed six-character field, e.g. `"  8FPS"`.
fn format_fps(value: u32, buffer: &mut [u8; 6]) -> &str {
    let value = value.min(999);
    buffer[0] = if value >= 100 {
        b'0' + (value / 100) as u8
    } else {
        b' '
    };
    buffer[1] = if value >= 10 {
        b'0' + ((value / 10) % 10) as u8
    } else {
        b' '
    };
    buffer[2] = b'0' + (value % 10) as u8;
    buffer[3] = b'F';
    buffer[4] = b'P';
    buffer[5] = b'S';
    core::str::from_utf8(buffer).unwrap_or("?")
}
