//! ESP32-S3-EYE pin map and panel constants.
//!
//! Sources: Espressif `esp-bsp/bsp/esp32_s3_eye/include/bsp/esp32_s3_eye.h`
//! and the ESP32-S3-EYE v2.2 schematic.

/// OV2640 master clock (XCLK).
pub const CAM_XCLK: u8 = 15;
/// OV2640 pixel clock.
pub const CAM_PCLK: u8 = 13;
/// OV2640 frame sync.
pub const CAM_VSYNC: u8 = 6;
/// OV2640 line/data enable (HREF).
pub const CAM_HREF: u8 = 7;

/// OV2640 DVP data bits D0..D7.
pub const CAM_DATA: [u8; 8] = [11, 9, 8, 10, 12, 18, 17, 16];

/// SCCB / shared I2C bus.
pub const I2C_SDA: u8 = 4;
pub const I2C_SCL: u8 = 5;

/// LCD SPI clock and MOSI.
pub const LCD_SCLK: u8 = 21;
pub const LCD_MOSI: u8 = 47;
/// LCD command/data select.
pub const LCD_DC: u8 = 43;
/// LCD chip select.
pub const LCD_CS: u8 = 44;
/// LCD backlight (active high).
pub const LCD_BL: u8 = 48;

/// Reset / BOOT key.
pub const BUTTON_BOOT: u8 = 0;
/// ADC-ladder button input (ADC1 channel 0).
pub const BUTTON_ADC: u8 = 1;
/// Green module LED (active high).
pub const LED: u8 = 3;

/// LCD resolution.
pub const LCD_WIDTH: usize = 240;
pub const LCD_HEIGHT: usize = 240;
/// Static status bar at the top of the panel.
pub const STATUS_BAR_HEIGHT: usize = 18;
/// Camera preview area below the status bar.
pub const CAM_VIEW_HEIGHT: usize = LCD_HEIGHT - STATUS_BAR_HEIGHT;
/// Pixels are sent big-endian, matching `BSP_LCD_BIGENDIAN` in esp-bsp.
pub const LCD_BYTES_PER_PIXEL: usize = 2;
/// Bytes in one LCD row.
pub const LCD_ROW_BYTES: usize = LCD_WIDTH * LCD_BYTES_PER_PIXEL;

/// Camera output resolution (QVGA). The display centre-crops this to 240×222.
pub const CAM_WIDTH: usize = 320;
pub const CAM_HEIGHT: usize = 240;
/// Vertical centre crop of the QVGA frame (240 - 222) / 2.
pub const CAM_Y_OFFSET: usize = (CAM_HEIGHT - CAM_VIEW_HEIGHT) / 2;
/// Set to `true` if the module is mounted upside-down relative to the LCD.
/// Flipping is done while scanning out the frame, so the captured buffer and
/// the face-box coordinates are adjusted consistently.
pub const CAM_VFLIP: bool = true;
/// Bytes in one QVGA YUV422 (YUYV) frame.
pub const CAM_FRAME_BYTES: usize = CAM_WIDTH * CAM_HEIGHT * 2;
/// Bytes in one camera row.
pub const CAM_ROW_BYTES: usize = CAM_WIDTH * 2;

/// Horizontal crop offset: (320 - 240) / 2.
pub const CROP_X: usize = (CAM_WIDTH - LCD_WIDTH) / 2;
/// Byte offset of the first visible pixel in a camera row.
pub const CROP_OFFSET_BYTES: usize = CROP_X * 2;
