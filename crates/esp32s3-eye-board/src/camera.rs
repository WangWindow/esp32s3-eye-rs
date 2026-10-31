//! Board camera composition: OV2640 setup and LCD_CAM capture.
//!
//! Generic OV2640 and LCD_CAM drivers live in the `ov2640` and
//! `esp32s3-lcd-cam` crates. This module only wires them to the board clocks
//! and buffers.

use embassy_time::{Duration, Timer};
use esp_hal::{
    Blocking,
    dma::AhbGdmaRxChannel,
    dma_rx_stream_buffer,
    i2c::master::I2c,
    lcd_cam::{
        ByteOrder,
        cam::{Config, ConfigError, EofMode, VhdeMode, VsyncFilterThreshold},
    },
    time::Rate,
};
use esp32s3_lcd_cam::{
    AsyncCameraDriver, CameraCapture, CaptureConfig, CaptureEvent, ConverterMode,
};
use ov2640::{ClockConfig, OV2640_ADDR, Ov2640, Ov2640Config, PixelFormat, Resolution};

/// 20 MHz XCLK is the value the OV2640 init tables were calibrated for.
/// QVGA RGB565 runs at roughly 12 FPS with the async capture state machine,
/// comfortably inside the 20 Hz display ceiling.
pub(crate) const CAMERA_CLOCK: ClockConfig = ClockConfig::new(crate::CAMERA_XCLK_HZ, 0, None);

#[derive(Debug)]
pub enum InitError<E> {
    I2c(E),
    WrongId,
}

/// Initialise the OV2640 over SCCB.
pub async fn init_sensor(
    i2c: &mut I2c<'_, Blocking>,
) -> Result<(), InitError<esp_hal::i2c::master::Error>> {
    let sensor = Ov2640::new(Ov2640Config {
        i2c_addr: OV2640_ADDR,
        format: PixelFormat::Rgb565,
        resolution: Resolution::Qvga,
        clock: CAMERA_CLOCK,
    });

    sensor.reset(i2c).map_err(InitError::I2c)?;
    Timer::after(Duration::from_millis(50)).await;
    if !sensor.check_id(i2c).map_err(InitError::I2c)? {
        return Err(InitError::WrongId);
    }
    sensor.init(i2c).await.map_err(InitError::I2c)
}

/// Internal DMA ring. One QVGA RGB565 frame is 153.6 KiB, so 192 KiB keeps
/// the DMA comfortably ahead of the display/analysis loop.
const STREAM_BUFFER_SIZE: usize = 192 * 1024;
const STREAM_CHUNK_SIZE: usize = 4092;
const CHUNK_BUFFER_SIZE: usize = 4092;

/// Continuous DMA camera front-end.
pub struct Camera {
    capture: CameraCapture<'static>,
    chunk: [u8; CHUNK_BUFFER_SIZE],
    synced: bool,
    restarts: u32,
}

impl Camera {
    fn new(driver: AsyncCameraDriver<'static>) -> Self {
        let dma_buffer = dma_rx_stream_buffer!(STREAM_BUFFER_SIZE, STREAM_CHUNK_SIZE);
        let capture = CameraCapture::new(driver, dma_buffer, CaptureConfig::default());

        Self {
            capture,
            chunk: [0; CHUNK_BUFFER_SIZE],
            synced: false,
            restarts: 0,
        }
    }

    /// Number of ring-full / error restarts since start-up.
    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    pub fn start(&mut self) {
        self.synced = false;
        if let Err(error) = self.capture.start() {
            esp_println::println!("camera DMA start failed: {:?}", error);
        }
    }

    pub fn stop(&mut self) {
        self.capture.stop_capture();
        self.synced = false;
    }

    /// Assemble one VSYNC-aligned frame from the continuous DMA ring.
    pub async fn capture_frame(&mut self, frame: &mut [u8]) -> Option<usize> {
        let frame_bytes = frame.len();
        let mut position = 0usize;

        loop {
            let result = self.capture.next_chunk(&mut self.chunk).await;

            if result.event == CaptureEvent::DmaError {
                self.restarts = self.restarts.wrapping_add(1);
                self.synced = false;
                position = 0;
                continue;
            }

            if result.resyncing {
                if result.eof {
                    self.restarts = self.restarts.wrapping_add(1);
                    self.synced = true;
                    position = 0;
                }
                continue;
            }

            if !self.synced {
                if result.eof {
                    self.synced = true;
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

/// Create the LCD_CAM driver for the board's fixed camera wiring.
#[allow(clippy::too_many_arguments)]
pub fn create_driver(
    lcd_cam: esp_hal::peripherals::LCD_CAM<'static>,
    dma: AhbGdmaRxChannel<'static>,
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
) -> Result<Camera, ConfigError> {
    let config = Config::default()
        .with_frequency(Rate::from_hz(CAMERA_CLOCK.xclk_hz()))
        .with_eof_mode(EofMode::VsyncSignal)
        .with_vh_de_mode(VhdeMode::De)
        .with_vsync_filter_threshold(VsyncFilterThreshold::Four)
        .with_byte_order(ByteOrder::Native)
        .with_enable_2byte_mode(false);

    AsyncCameraDriver::new(
        lcd_cam,
        dma,
        config,
        ConverterMode::Bypass,
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
    .map(Camera::new)
}
