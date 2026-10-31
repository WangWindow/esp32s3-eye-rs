//! Application UI: camera overlays, status bar, menus and static screens.
//!
//! Hardware access goes through `esp32s3_eye_board::display::Panel`.

use esp32s3_eye_board::display::Panel;
use esp32s3_eye_board::{
    CAM_ROW_BYTES, CAM_VFLIP, CAM_VIEW_HEIGHT, CAM_Y_OFFSET, CROP_OFFSET_BYTES, LCD_HEIGHT,
    LCD_ROW_BYTES, LCD_WIDTH, STATUS_BAR_HEIGHT,
};

use crate::geometry::{Point, Rect};
use crate::graphics::{GLYPH_ADVANCE, GLYPH_HEIGHT, GLYPH_WIDTH, glyph, text_width};
use crate::modes::Mode;

pub const BLACK: u16 = 0x0000;
pub const WHITE: u16 = 0xFFFF;
pub const BLUE: u16 = 0x001F;
pub const CYAN: u16 = 0x07FF;
pub const GREEN: u16 = 0x07E0;
pub const RED: u16 = 0xF800;
pub const YELLOW: u16 = 0xFFE0;
pub const GRAY: u16 = 0x8410;
pub const DARK_GRAY: u16 = 0x4208;

#[derive(Clone, Copy, Debug, Default)]
pub struct Overlay {
    pub motion: bool,
    pub face: Option<Rect>,
    pub keypoints: Option<[Point; 5]>,
}

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

    if let Some(points) = overlay.keypoints {
        for point in points {
            if (screen_y as i32 - point.y as i32).abs() <= 1 {
                let x = point.x as i32;
                for dx in -1..=1 {
                    if x + dx >= 0 {
                        put((x + dx) as usize, YELLOW);
                    }
                }
            }
        }
    }
}

pub struct Ui {
    line: [u8; LCD_ROW_BYTES],
}

impl Default for Ui {
    fn default() -> Self {
        Self::new()
    }
}

impl Ui {
    pub fn new() -> Self {
        Self {
            line: [0; LCD_ROW_BYTES],
        }
    }

    pub fn draw_rgb565_frame(&mut self, panel: &mut Panel, frame: &[u8], overlay: Overlay) {
        panel
            .set_window(
                0,
                STATUS_BAR_HEIGHT as u16,
                LCD_WIDTH as u16,
                CAM_VIEW_HEIGHT as u16,
            )
            .ok();
        panel.begin_pixels().ok();

        for screen_y in STATUS_BAR_HEIGHT..LCD_HEIGHT {
            let view_row = screen_y - STATUS_BAR_HEIGHT;
            let source_y = if CAM_VFLIP {
                CAM_Y_OFFSET + (CAM_VIEW_HEIGHT - 1 - view_row)
            } else {
                CAM_Y_OFFSET + view_row
            };
            let row_start = source_y * CAM_ROW_BYTES + CROP_OFFSET_BYTES;
            self.line
                .copy_from_slice(&frame[row_start..row_start + LCD_ROW_BYTES]);

            apply_overlay(&mut self.line, screen_y, overlay);
            panel.write_pixels(&self.line[..]).ok();
        }

        panel.end_pixels();
    }

    pub fn draw_status_bar(&mut self, panel: &mut Panel, mode: Mode, fps: u32) {
        panel.fill_rect(0, 0, LCD_WIDTH as u16, STATUS_BAR_HEIGHT as u16, BLACK);
        panel.fill_rect(3, 4, 4, 10, mode.accent());
        self.draw_text(panel, 11, 3, mode.label(), WHITE, BLACK, 1);
        self.update_fps(panel, fps);
    }

    pub fn update_fps(&mut self, panel: &mut Panel, fps: u32) {
        let mut text = [0u8; 6];
        let text = format_fps(fps, &mut text);
        let width = text_width(text, 1) as u16;
        self.draw_text(
            panel,
            LCD_WIDTH as i16 - width as i16 - 4,
            3,
            text,
            CYAN,
            BLACK,
            1,
        );
    }

    pub fn show_pause(&mut self, panel: &mut Panel) {
        panel.fill_rect(80, 100, 80, 24, BLACK);
        self.draw_rect(panel, Rect::new(80, 100, 80, 24), YELLOW, 1);
        self.draw_text_centered(panel, 109, "PAUSE", YELLOW, BLACK, 1);
    }

    pub fn show_standby(&mut self, panel: &mut Panel) {
        panel.fill_screen(BLACK);

        panel.fill_rect(70, 30, 100, 16, BLUE);
        panel.fill_rect(82, 24, 76, 4, BLUE);
        panel.fill_rect(82, 48, 76, 4, BLUE);
        panel.fill_rect(105, 30, 30, 16, WHITE);
        panel.fill_rect(112, 34, 16, 8, BLACK);
        panel.fill_rect(116, 36, 8, 4, CYAN);

        self.draw_text_centered(panel, 66, "ESP32-S3-EYE", WHITE, BLACK, 2);
        self.draw_text_centered(panel, 88, "RUST + EMBASSY", CYAN, BLACK, 1);
        self.draw_text_centered(panel, 100, "OV2640 / ST7789", GRAY, BLACK, 1);

        panel.fill_rect(40, 122, 160, 1, DARK_GRAY);

        self.draw_text_centered(panel, 134, "UP / DN   MODE", WHITE, BLACK, 1);
        self.draw_text_centered(panel, 148, "PLAY      PAUSE", WHITE, BLACK, 1);
        self.draw_text_centered(panel, 162, "BOOT      NEXT", WHITE, BLACK, 1);
        self.draw_text_centered(panel, 188, "PRESS A KEY", YELLOW, BLACK, 1);
    }

    pub fn show_error(&mut self, panel: &mut Panel, message: &str) {
        panel.fill_screen(BLACK);
        self.draw_text_centered(panel, 70, "CAMERA ERROR", RED, BLACK, 2);
        self.draw_text_centered(panel, 110, message, WHITE, BLACK, 1);
        self.draw_text_centered(panel, 150, "CHECK OV2640 / I2C", GRAY, BLACK, 1);
    }

    pub fn show_storage(
        &mut self,
        panel: &mut Panel,
        capacity_mib: u32,
        read_ok: bool,
        write_ok: bool,
        error: Option<&str>,
    ) {
        panel.fill_screen(BLACK);
        self.draw_text_centered(panel, 32, "SD CARD", WHITE, BLACK, 2);

        if let Some(error) = error {
            self.draw_text_centered(panel, 86, "INIT FAILED", RED, BLACK, 1);
            self.draw_text_centered(panel, 108, error, GRAY, BLACK, 1);
            self.draw_text_centered(panel, 140, "CHECK MICROSD SLOT", GRAY, BLACK, 1);
        } else {
            self.draw_text_centered(panel, 82, "CAPACITY", GRAY, BLACK, 1);
            let mut buffer = [0u8; 14];
            self.draw_text_centered(
                panel,
                100,
                format_mib(capacity_mib, &mut buffer),
                WHITE,
                BLACK,
                1,
            );
            self.draw_text_centered(
                panel,
                132,
                if read_ok { "READ  OK" } else { "READ  FAIL" },
                if read_ok { GREEN } else { RED },
                BLACK,
                1,
            );
            self.draw_text_centered(
                panel,
                150,
                if write_ok { "WRITE OK" } else { "WRITE FAIL" },
                if write_ok { GREEN } else { RED },
                BLACK,
                1,
            );
        }

        self.draw_text_centered(panel, 200, "UP / DOWN TO EXIT", GRAY, BLACK, 1);
    }

    fn draw_rect(&mut self, panel: &mut Panel, rect: Rect, color: u16, thickness: u16) {
        panel.draw_rect(rect.x, rect.y, rect.w, rect.h, color, thickness);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_char(
        &mut self,
        panel: &mut Panel,
        x: i16,
        y: i16,
        character: u8,
        fg: u16,
        bg: u16,
        scale: usize,
    ) {
        let scale = scale.max(1);
        let full_width = GLYPH_WIDTH * scale;
        let full_height = GLYPH_HEIGHT * scale;
        let mut buffer = [0u8; 1120];

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

        panel.set_window(x, y, clip_width, clip_height).ok();
        panel.begin_pixels().ok();
        for py in 0..clip_height as usize {
            let source = py * full_width * 2;
            self.line[..clip_width as usize * 2]
                .copy_from_slice(&buffer[source..source + clip_width as usize * 2]);
            panel
                .write_pixels(&self.line[..clip_width as usize * 2])
                .ok();
        }
        panel.end_pixels();
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text(
        &mut self,
        panel: &mut Panel,
        x: i16,
        y: i16,
        text: &str,
        fg: u16,
        bg: u16,
        scale: usize,
    ) {
        let scale = scale.max(1);
        let advance = (GLYPH_ADVANCE * scale) as i16;
        for (index, character) in text.bytes().enumerate() {
            let cx = x + index as i16 * advance;
            if cx >= LCD_WIDTH as i16 {
                break;
            }
            self.draw_char(panel, cx, y, character, fg, bg, scale);
        }
    }

    fn draw_text_centered(
        &mut self,
        panel: &mut Panel,
        y: i16,
        text: &str,
        fg: u16,
        bg: u16,
        scale: usize,
    ) {
        let width = text_width(text, scale) as i16;
        self.draw_text(
            panel,
            (LCD_WIDTH as i16 - width) / 2,
            y,
            text,
            fg,
            bg,
            scale,
        );
    }
}

fn format_mib(value: u32, buffer: &mut [u8; 14]) -> &str {
    let mut digits = [0u8; 10];
    let mut remaining = value;
    let mut index = digits.len();
    loop {
        index -= 1;
        digits[index] = b'0' + (remaining % 10) as u8;
        remaining /= 10;
        if remaining == 0 {
            break;
        }
    }

    let len = digits.len() - index;
    buffer[..len].copy_from_slice(&digits[index..]);
    buffer[len..len + 4].copy_from_slice(b" MiB");
    core::str::from_utf8(&buffer[..len + 4]).unwrap()
}

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
    core::str::from_utf8(buffer).unwrap()
}
