//! OV2640 bring-up and the DVP/LCD_CAM capture pipeline.
//!
//! The sensor is configured for QVGA (320×240) YUV422 (YUYV), which is the
//! native format of the OV2640 DSP output. The display driver converts it to
//! RGB565 one scanline at a time.

use esp_hal::{Blocking, dma_rx_stream_buffer, i2c::master::I2c};
use esp32s3_cam_async::{
    AsyncCameraDriver, CameraCapture, CaptureConfig, CaptureEvent, ConverterMode,
    ov2640::{ClockConfig, Ov2640, Ov2640Config, PixelFormat, Resolution},
};

use crate::{
    board::{
        CAM_FRAME_BYTES, CAM_HEIGHT, CAM_ROW_BYTES, CAM_VFLIP, CAM_VIEW_HEIGHT, CAM_WIDTH,
        CAM_Y_OFFSET, CROP_X, STATUS_BAR_HEIGHT,
    },
    display::Rect,
};

const DMA_CHUNK_SIZE: usize = 2048;
const DMA_BLOCK_SIZE: usize = 512;

/// 20 MHz XCLK is stable on the short ESP32-S3-EYE camera flex. Use
/// `ClockConfig::BALANCED` (10 MHz) if a particular camera module shows
/// dropped frames or colour artefacts.
const CAMERA_CLOCK: ClockConfig = ClockConfig::HIGH_FPS;

/// Grid used by the motion and skin-colour detectors.
const GRID_CELL: usize = 16;
const GRID_W: usize = CAM_WIDTH / GRID_CELL; // 20
const GRID_H: usize = CAM_HEIGHT / GRID_CELL; // 15
const GRID_CELLS: usize = GRID_W * GRID_H; // 300

/// Initialise the OV2640 over SCCB.
pub async fn init_sensor(i2c: &mut I2c<'_, Blocking>) -> Result<(), esp_hal::i2c::master::Error> {
    let sensor = Ov2640::new(Ov2640Config {
        i2c_addr: esp32s3_cam_async::ov2640::OV2640_ADDR,
        format: PixelFormat::Yuv422,
        resolution: Resolution::Qvga,
        clock: CAMERA_CLOCK,
    });

    sensor.check_id(i2c)?;
    sensor.init(i2c).await
}

/// Owns the LCD_CAM driver, DMA stream and capture state machine.
pub struct CameraPipeline {
    capture: CameraCapture<'static>,
    chunk: [u8; DMA_CHUNK_SIZE],
}

impl CameraPipeline {
    pub fn new(driver: AsyncCameraDriver<'static>) -> Self {
        // The buffer and descriptors are static within this expansion.
        let dma_buffer = dma_rx_stream_buffer!(DMA_CHUNK_SIZE, DMA_BLOCK_SIZE);
        let capture = CameraCapture::new(driver, dma_buffer, CaptureConfig::default());

        Self {
            capture,
            chunk: [0; DMA_CHUNK_SIZE],
        }
    }

    pub fn start(&mut self) {
        if let Err(error) = self.capture.start() {
            esp_println::println!("camera DMA start failed: {:?}", error);
        }
    }

    pub fn stop(&mut self) {
        self.capture.stop_capture();
    }

    /// Wait for a complete, VSYNC-aligned frame.
    ///
    /// The first partial frame after `start()` is discarded so that the
    /// returned buffer always starts at row 0.
    pub async fn capture_frame(&mut self, frame: &mut [u8]) -> Option<usize> {
        let frame_bytes = frame.len().min(CAM_FRAME_BYTES);
        let mut synced = false;
        let mut position = 0usize;

        loop {
            let result = self.capture.next_chunk(&mut self.chunk).await;

            if result.event == CaptureEvent::DmaError {
                synced = false;
                position = 0;
                continue;
            }

            if result.resyncing {
                if result.eof {
                    synced = true;
                    position = 0;
                }
                continue;
            }

            if !synced {
                if result.eof {
                    synced = true;
                    position = 0;
                }
                continue;
            }

            if result.len > 0 {
                let count = (frame_bytes - position).min(result.len);
                frame[position..position + count].copy_from_slice(&self.chunk[..count]);
                position += count;
            }

            if result.eof {
                if position >= frame_bytes {
                    return Some(position);
                }
                position = 0;
            }
        }
    }
}

/// Result of analysing one camera frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameAnalysis {
    pub motion: bool,
    pub face: Option<Rect>,
}

pub struct MotionDetector {
    previous: [u8; GRID_CELLS],
    primed: bool,
}

impl Default for MotionDetector {
    fn default() -> Self {
        Self {
            previous: [0; GRID_CELLS],
            primed: false,
        }
    }
}

impl MotionDetector {
    fn update(&mut self, luma: &[u8; GRID_CELLS]) -> bool {
        let mut changed = 0usize;

        if self.primed {
            for (current, previous) in luma.iter().zip(self.previous.iter()) {
                if current.abs_diff(*previous) > 18 {
                    changed += 1;
                }
            }
        }

        self.previous = *luma;
        self.primed = true;

        // A handful of changed cells can be sensor noise; require a visible
        // moving region (roughly 3 % of the frame).
        changed >= 9
    }
}

/// Analyse a frame according to the active mode.
pub fn analyse(
    frame: &[u8],
    mode: crate::modes::Mode,
    detector: &mut MotionDetector,
) -> FrameAnalysis {
    match mode {
        crate::modes::Mode::Motion => FrameAnalysis {
            motion: detector.update(&luma_grid(frame)),
            face: None,
        },
        crate::modes::Mode::Face => FrameAnalysis {
            motion: false,
            face: largest_skin_blob(&skin_grid(frame)),
        },
        _ => FrameAnalysis::default(),
    }
}

/// Average luma of every 16×16 cell.
fn luma_grid(frame: &[u8]) -> [u8; GRID_CELLS] {
    let mut sums = [0u32; GRID_CELLS];

    for y in 0..CAM_HEIGHT {
        let row = &frame[y * CAM_ROW_BYTES..(y + 1) * CAM_ROW_BYTES];
        let cell_row = (y / GRID_CELL) * GRID_W;
        for x in 0..CAM_WIDTH {
            // Y is the even byte of each YUYV pair.
            sums[cell_row + x / GRID_CELL] += row[x * 2] as u32;
        }
    }

    let mut result = [0u8; GRID_CELLS];
    for (result_cell, sum) in result.iter_mut().zip(sums.iter()) {
        *result_cell = (*sum / (GRID_CELL * GRID_CELL) as u32) as u8;
    }
    result
}

/// Skin-colour mask in YCbCr (Cb 77..=127, Cr 133..=173).
fn skin_grid(frame: &[u8]) -> [bool; GRID_CELLS] {
    let mut counts = [0u16; GRID_CELLS];

    for y in 0..CAM_HEIGHT {
        let row = &frame[y * CAM_ROW_BYTES..(y + 1) * CAM_ROW_BYTES];
        let cell_row = (y / GRID_CELL) * GRID_W;

        for pair in 0..(CAM_WIDTH / 2) {
            let i = pair * 4;
            let y0 = row[i];
            let cb = row[i + 1];
            let y1 = row[i + 2];
            let cr = row[i + 3];
            let skin = (77..=127).contains(&cb) && (133..=173).contains(&cr);

            if skin && y0 > 40 {
                counts[cell_row + (pair * 2) / GRID_CELL] += 1;
            }
            if skin && y1 > 40 {
                counts[cell_row + (pair * 2 + 1) / GRID_CELL] += 1;
            }
        }
    }

    let mut mask = [false; GRID_CELLS];
    for index in 0..GRID_CELLS {
        // At least 15 % of the 256 pixels in a cell must be skin coloured.
        mask[index] = counts[index] >= 38;
    }
    mask
}

/// Find the largest 4-connected skin region and return its screen-space box.
fn largest_skin_blob(mask: &[bool; GRID_CELLS]) -> Option<Rect> {
    const MIN_CELLS: usize = 4;
    const MAX_CELLS: usize = 120;

    let mut visited = [false; GRID_CELLS];
    let mut queue = [0u8; GRID_CELLS];
    let mut best: Option<(usize, usize, usize, usize, usize)> = None; // area, x0, y0, x1, y1

    for start in 0..GRID_CELLS {
        if !mask[start] || visited[start] {
            continue;
        }

        let mut head = 0usize;
        let mut tail = 0usize;
        queue[tail] = start as u8;
        tail += 1;
        visited[start] = true;

        let mut area = 0usize;
        let mut x0 = GRID_W;
        let mut y0 = GRID_H;
        let mut x1 = 0usize;
        let mut y1 = 0usize;

        while head < tail {
            let cell = queue[head] as usize;
            head += 1;
            area += 1;

            let cx = cell % GRID_W;
            let cy = cell / GRID_W;
            x0 = x0.min(cx);
            y0 = y0.min(cy);
            x1 = x1.max(cx);
            y1 = y1.max(cy);

            let push = |nx: usize,
                        ny: usize,
                        visited: &mut [bool; GRID_CELLS],
                        queue: &mut [u8; GRID_CELLS],
                        tail: &mut usize| {
                let index = ny * GRID_W + nx;
                if !visited[index] && mask[index] {
                    visited[index] = true;
                    queue[*tail] = index as u8;
                    *tail += 1;
                }
            };

            if cx > 0 {
                push(cx - 1, cy, &mut visited, &mut queue, &mut tail);
            }
            if cx + 1 < GRID_W {
                push(cx + 1, cy, &mut visited, &mut queue, &mut tail);
            }
            if cy > 0 {
                push(cx, cy - 1, &mut visited, &mut queue, &mut tail);
            }
            if cy + 1 < GRID_H {
                push(cx, cy + 1, &mut visited, &mut queue, &mut tail);
            }
        }

        if (MIN_CELLS..=MAX_CELLS).contains(&area) {
            let replace = best.map(|(best_area, ..)| area > best_area).unwrap_or(true);
            if replace {
                best = Some((area, x0, y0, x1, y1));
            }
        }
    }

    best.map(|(_, x0, y0, x1, y1)| {
        let left = (x0 * GRID_CELL).saturating_sub(CROP_X);
        let right = ((x1 + 1) * GRID_CELL).saturating_sub(CROP_X);

        // Source-frame Y -> screen Y. The preview centre-crops CAM_Y_OFFSET
        // rows, starts below the status bar, and may be vertically flipped.
        let source_top = y0 * GRID_CELL;
        let source_bottom = (y1 + 1) * GRID_CELL;
        let (top, bottom) = if CAM_VFLIP {
            // screen_y = STATUS_BAR_HEIGHT + CAM_Y_OFFSET + CAM_VIEW_HEIGHT - 1 - source_y
            let screen_sum = STATUS_BAR_HEIGHT + CAM_Y_OFFSET + CAM_VIEW_HEIGHT - 1;
            (screen_sum - source_bottom, screen_sum - source_top)
        } else {
            (
                source_top + STATUS_BAR_HEIGHT - CAM_Y_OFFSET,
                source_bottom + STATUS_BAR_HEIGHT - CAM_Y_OFFSET,
            )
        };

        let left = left.min(crate::board::LCD_WIDTH) as u16;
        let right = right.min(crate::board::LCD_WIDTH) as u16;
        let top = top.clamp(STATUS_BAR_HEIGHT, crate::board::LCD_HEIGHT) as u16;
        let bottom = bottom.clamp(STATUS_BAR_HEIGHT, crate::board::LCD_HEIGHT) as u16;

        let width = right.saturating_sub(left);
        let height = bottom.saturating_sub(top);

        // Add a little padding, but keep the box below the status bar.
        let pad = 4u16;
        let x = left.saturating_sub(pad);
        let y = top.saturating_sub(pad).max(STATUS_BAR_HEIGHT as u16);
        let w = (width + pad * 2).min(crate::board::LCD_WIDTH as u16 - x);
        let h = (height + pad * 2).min(crate::board::LCD_HEIGHT as u16 - y);

        Rect::new(x, y, w, h)
    })
}

/// Create the LCD_CAM driver for the board's fixed camera wiring.
#[allow(clippy::too_many_arguments)]
pub fn create_driver(
    lcd_cam: esp_hal::peripherals::LCD_CAM<'static>,
    dma: esp_hal::dma::AnyGdmaRxChannel<'static>,
    xclk: esp_hal::peripherals::GPIO15<'static>,
    pclk: esp_hal::peripherals::GPIO13<'static>,
    vsync: esp_hal::peripherals::GPIO6<'static>,
    href: esp_hal::peripherals::GPIO7<'static>,
    d0: esp_hal::peripherals::GPIO11<'static>,
    d1: esp_hal::peripherals::GPIO9<'static>,
    d2: esp_hal::peripherals::GPIO8<'static>,
    d3: esp_hal::peripherals::GPIO10<'static>,
    d4: esp_hal::peripherals::GPIO12<'static>,
    d5: esp_hal::peripherals::GPIO18<'static>,
    d6: esp_hal::peripherals::GPIO17<'static>,
    d7: esp_hal::peripherals::GPIO16<'static>,
) -> Result<AsyncCameraDriver<'static>, esp_hal::lcd_cam::cam::ConfigError> {
    use esp_hal::lcd_cam::{
        ByteOrder,
        cam::{Config, EofMode, VhdeMode, VsyncFilterThreshold},
    };

    let config = Config::default()
        .with_frequency(CAMERA_CLOCK.xclk())
        .with_eof_mode(EofMode::VsyncSignal)
        .with_vh_de_mode(VhdeMode::De)
        .with_vsync_filter_threshold(VsyncFilterThreshold::Four)
        .with_byte_order(ByteOrder::Native)
        .with_enable_2byte_mode(false);

    AsyncCameraDriver::new(
        lcd_cam,
        dma,
        config,
        ConverterMode::Yuv422Passthrough,
        xclk,
        pclk,
        vsync,
        href,
        d0,
        d1,
        d2,
        d3,
        d4,
        d5,
        d6,
        d7,
    )
}
