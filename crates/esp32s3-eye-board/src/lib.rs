#![no_std]
//! ESP32-S3-EYE board support package.
//!
//! All board hardware is exposed through [`Board`]: pins, camera, display,
//! buttons, LED and SD card. The application only talks to this interface.

use esp_hal::{
    Blocking, Config as HalConfig,
    clock::CpuClock,
    dma::DmaChannel,
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    psram::{Psram, PsramConfig},
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
    timer::timg::TimerGroup,
};
use esp_println::println;

pub mod buttons;
pub mod camera;
pub mod display;
pub mod led;
pub mod storage;

use buttons::Button;
use camera::Camera;
use display::Panel;
use led::Led;
use storage::{SdPeripherals, SelfTest, Storage};

/// OV2640 master clock (XCLK).
pub const CAM_XCLK: u8 = 15;
/// XCLK frequency used by the board camera configuration.
pub const CAMERA_XCLK_HZ: u32 = 20_000_000;
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
/// LCD SPI clock frequency.
pub const LCD_SPI_HZ: u32 = 80_000_000;
pub const LCD_MOSI: u8 = 47;
/// LCD command/data select.
pub const LCD_DC: u8 = 43;
/// LCD chip select.
pub const LCD_CS: u8 = 44;
/// LCD backlight (active high).
pub const LCD_BL: u8 = 48;

/// microSD slot (1-bit SDMMC).
pub const SD_CLK: u8 = 39;
pub const SD_CMD: u8 = 38;
pub const SD_D0: u8 = 40;
/// SD card clock frequency.
pub const SD_FREQUENCY_HZ: u32 = 20_000_000;

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
/// Bytes in one QVGA RGB565 frame.
pub const CAM_FRAME_BYTES: usize = CAM_WIDTH * CAM_HEIGHT * 2;
/// Bytes in one camera row.
pub const CAM_ROW_BYTES: usize = CAM_WIDTH * 2;

/// Horizontal crop offset: (320 - 240) / 2.
pub const CROP_X: usize = (CAM_WIDTH - LCD_WIDTH) / 2;
/// Byte offset of the first visible pixel in a camera row.
pub const CROP_OFFSET_BYTES: usize = CROP_X * 2;

pub struct Board {
    pub camera: Camera,
    pub display: Panel<'static>,
    pub buttons: buttons::Buttons<'static>,
    pub led: Led,
    pub frame: &'static mut [u8],
    pub storage: Option<Storage>,
    i2c: I2c<'static, Blocking>,
    sd: Option<SdPeripherals>,
    _psram: Psram,
}

impl Board {
    /// Initialise the HAL, PSRAM, LCD, camera, buttons, LED and SD wiring.
    pub fn init() -> Self {
        let p = esp_hal::init(HalConfig::default().with_cpu_clock(CpuClock::max()));

        println!();
        println!("ESP32-S3-EYE · Rust + esp-hal + Embassy");
        println!("board: OV2640 RGB565 / ST7789 / SDMMC");

        let timg0 = TimerGroup::new(p.TIMG0);
        esp_rtos::start(timg0.timer0, p.FROM_CPU_INTR0);

        let psram = Psram::new(p.PSRAM, PsramConfig::default());
        let (psram_ptr, psram_size) = psram.raw_parts();
        println!("PSRAM: {} bytes", psram_size);
        assert!(psram_size >= CAM_FRAME_BYTES, "PSRAM is too small");
        let frame: &'static mut [u8] =
            unsafe { core::slice::from_raw_parts_mut(psram_ptr, CAM_FRAME_BYTES) };

        let spi = Spi::new(
            p.SPI2,
            SpiConfig::default().with_frequency(Rate::from_hz(LCD_SPI_HZ)),
        )
        .expect("SPI2")
        .with_sck(p.GPIO21)
        .with_mosi(p.GPIO47);

        let display = Panel::new(
            spi,
            Output::new(p.GPIO44, Level::High, OutputConfig::default()),
            Output::new(p.GPIO43, Level::High, OutputConfig::default()),
            Output::new(p.GPIO48, Level::Low, OutputConfig::default()),
        );

        let i2c = I2c::new(
            p.I2C0,
            I2cConfig::default().with_frequency(Rate::from_khz(400)),
        )
        .expect("I2C0")
        .with_sda(p.GPIO4)
        .with_scl(p.GPIO5);

        let camera = camera::create_driver(
            p.LCD_CAM,
            p.DMA_CH0.split().0,
            p.GPIO15,
            p.GPIO13,
            p.GPIO6,
            p.GPIO7,
            p.GPIO11,
            p.GPIO9,
            p.GPIO8,
            p.GPIO10,
            p.GPIO12,
            p.GPIO18,
            p.GPIO17,
            p.GPIO16,
        )
        .expect("LCD_CAM camera driver");

        let buttons = buttons::Buttons::new(p.ADC1, p.GPIO1, p.GPIO0);
        let led = Led::new(Output::new(p.GPIO3, Level::Low, OutputConfig::default()));
        let sd = Some(SdPeripherals {
            host: p.SDHOST,
            clk: p.GPIO39,
            cmd: p.GPIO38,
            d0: p.GPIO40,
        });

        let mut board = Self {
            camera,
            display,
            buttons,
            led,
            frame,
            storage: None,
            i2c,
            sd,
            _psram: psram,
        };
        board.display.init().ok();
        board
    }

    pub async fn init_camera(
        &mut self,
    ) -> Result<(), camera::InitError<esp_hal::i2c::master::Error>> {
        camera::init_sensor(&mut self.i2c).await
    }

    pub async fn init_storage(&mut self) {
        if let Some(sd) = self.sd.take() {
            self.storage = storage::init(sd).await;
        }
    }

    pub async fn capture_frame(&mut self) -> Option<usize> {
        self.camera.capture_frame(self.frame).await
    }

    pub fn camera_start(&mut self) {
        self.camera.start();
    }

    pub fn camera_stop(&mut self) {
        self.camera.stop();
    }

    pub fn camera_restarts(&self) -> u32 {
        self.camera.restarts()
    }

    pub fn poll_button(&mut self) -> Option<Button> {
        self.buttons.poll()
    }

    pub fn led_on(&mut self) {
        self.led.on();
    }

    pub fn led_off(&mut self) {
        self.led.off();
    }

    pub fn sd_capacity_mib(&self) -> Option<u32> {
        self.storage.as_ref().map(Storage::capacity_mib)
    }

    pub async fn sd_self_test(&mut self) -> Option<SelfTest> {
        match self.storage.as_mut() {
            Some(storage) => storage.self_test().await.ok(),
            None => None,
        }
    }
}
