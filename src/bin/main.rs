#![no_std]
#![no_main]
#![deny(clippy::large_stack_frames)]

use embassy_time::{Duration, Instant, Timer};
use esp_hal::{
    Blocking,
    clock::CpuClock,
    dma::DmaChannel,
    gpio::{Level, Output, OutputConfig},
    i2c::master::{Config as I2cConfig, I2c},
    interrupt::software::SoftwareInterruptControl,
    spi::master::{Config as SpiConfig, Spi},
    time::Rate,
    timer::timg::TimerGroup,
};
use esp32s3_cam_async::AsyncCameraDriver;
use static_cell::StaticCell;

use esp32s3_eye::{
    board::{CAM_FRAME_BYTES, LCD_ROW_BYTES},
    buttons::{Button, Buttons},
    camera::{self, CameraPipeline, MotionDetector},
    display::{Display, Overlay},
    modes::Mode,
};

// Panic handler + USB Serial/JTAG `println!` implementation.
use esp_backtrace as _;
use esp_println::println;

// Required by the ESP-IDF bootloader image format.
esp_bootloader_esp_idf::esp_app_desc!();

static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

#[esp_hal::main]
fn main() -> ! {
    let config = esp_hal::Config::default().with_cpu_clock(CpuClock::max());
    let peripherals = esp_hal::init(config);

    println!();
    println!("ESP32-S3-EYE · Rust + esp-hal + Embassy");
    println!("OV2640 QVGA YUV422 → ST7789 240x240");

    // Put the 153.6 KiB QVGA frame buffer in the board's 8 MiB PSRAM. The
    // internal SRAM stays free for the stack, DMA descriptors and the executor.
    let psram =
        esp_hal::psram::Psram::new(peripherals.PSRAM, esp_hal::psram::PsramConfig::default());
    let (psram_ptr, psram_size) = psram.raw_parts();
    println!("PSRAM: {} bytes", psram_size);
    assert!(psram_size >= CAM_FRAME_BYTES, "PSRAM is too small");
    let frame: &'static mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(psram_ptr, CAM_FRAME_BYTES) };

    // esp-rtos owns the scheduler and the Embassy time driver. It must run
    // before the executor because OV2640 setup and capture timeouts use
    // `embassy-time`.
    let timg0 = TimerGroup::new(peripherals.TIMG0);
    let software_interrupts = SoftwareInterruptControl::new(peripherals.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, software_interrupts.software_interrupt0);

    // LCD: SPI2 @ 40 MHz, manual CS/DC, backlight on GPIO48.
    let spi = Spi::new(
        peripherals.SPI2,
        SpiConfig::default().with_frequency(Rate::from_mhz(40)),
    )
    .expect("SPI2")
    .with_sck(peripherals.GPIO21)
    .with_mosi(peripherals.GPIO47);

    let display = Display::new(
        spi,
        Output::new(peripherals.GPIO44, Level::High, OutputConfig::default()),
        Output::new(peripherals.GPIO43, Level::High, OutputConfig::default()),
        Output::new(peripherals.GPIO48, Level::Low, OutputConfig::default()),
    );

    let buttons = Buttons::new(peripherals.ADC1, peripherals.GPIO1, peripherals.GPIO0);
    let led = Output::new(peripherals.GPIO3, Level::Low, OutputConfig::default());

    // SCCB uses the camera-only I2C pins GPIO4/5.
    let i2c = I2c::new(
        peripherals.I2C0,
        I2cConfig::default().with_frequency(Rate::from_khz(400)),
    )
    .expect("I2C0")
    .with_sda(peripherals.GPIO4)
    .with_scl(peripherals.GPIO5);

    let camera = camera::create_driver(
        peripherals.LCD_CAM,
        peripherals.DMA_CH0.split().0,
        peripherals.GPIO15,
        peripherals.GPIO13,
        peripherals.GPIO6,
        peripherals.GPIO7,
        peripherals.GPIO11,
        peripherals.GPIO9,
        peripherals.GPIO8,
        peripherals.GPIO10,
        peripherals.GPIO12,
        peripherals.GPIO18,
        peripherals.GPIO17,
        peripherals.GPIO16,
    )
    .expect("LCD_CAM camera driver");

    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(|spawner| {
        spawner.spawn(app_task(camera, i2c, display, buttons, led, frame).expect("spawn app task"));
    });
}

#[embassy_executor::task]
async fn app_task(
    camera: AsyncCameraDriver<'static>,
    mut i2c: I2c<'static, Blocking>,
    mut display: Display<'static>,
    mut buttons: Buttons<'static>,
    mut led: Output<'static>,
    frame: &'static mut [u8],
) {
    display.init().ok();
    display.show_standby();

    println!("Initialising OV2640…");
    if let Err(error) = camera::init_sensor(&mut i2c).await {
        println!("OV2640 init failed: {:?}", error);
        display.show_error("NO OV2640 / SCCB");
        loop {
            Timer::after(Duration::from_secs(1)).await;
        }
    }
    println!("OV2640 ready (QVGA YUV422)");

    let mut pipeline = CameraPipeline::new(camera);
    let mut line = [0u8; LCD_ROW_BYTES];
    let mut detector = MotionDetector::default();

    let mut mode = Mode::Camera;
    let mut paused = false;
    let mut started = false;
    let mut last_scan = Instant::now();
    let mut last_frame_time = Instant::now();
    let mut last_fps_update = Instant::now();
    let mut fps = 0u32;
    let mut displayed_fps = 0u32;
    let mut frame_count = 0u32;

    // Let the splash screen stay visible for a moment before the first frame.
    Timer::after(Duration::from_millis(300)).await;
    // The status bar lives outside the camera window and is only repainted
    // here and on mode changes, so it no longer flashes at the frame rate.
    display.draw_status_bar(mode, 0);

    loop {
        // Poll the ADC ladder and BOOT key every 25 ms. The camera loop can
        // take tens of milliseconds, so this is deliberately not a separate
        // task: a single owner avoids sharing the display and sensor.
        if last_scan.elapsed() >= Duration::from_millis(25) {
            last_scan = Instant::now();

            if let Some(button) = buttons.poll() {
                match button {
                    Button::Up | Button::Boot => {
                        mode = mode.next();
                        paused = false;
                    }
                    Button::Down => {
                        mode = mode.previous();
                        paused = false;
                    }
                    Button::Play => {
                        paused = !paused;
                        if paused && mode != Mode::Standby {
                            display.show_pause();
                        }
                    }
                }

                println!(
                    "button={:?} mode={:?} paused={} adc={}",
                    button,
                    mode,
                    paused,
                    buttons.last_raw()
                );

                if mode == Mode::Standby || paused {
                    pipeline.stop();
                    started = false;
                }

                if mode == Mode::Standby {
                    display.show_standby();
                } else {
                    display.draw_status_bar(mode, fps);
                    displayed_fps = fps;
                }
            }
        }

        if mode == Mode::Standby {
            led.set_low();
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }

        if paused {
            led.set_low();
            Timer::after(Duration::from_millis(50)).await;
            continue;
        }

        if !started {
            pipeline.start();
            started = true;
        }

        // Temporary benchmark: capture continuously without display.
        if true {
            let t = Instant::now();
            if pipeline.capture_frame(frame).await.is_some() {
                frame_count = frame_count.wrapping_add(1);
                let elapsed = t.elapsed().as_millis();
                if frame_count.is_multiple_of(60) {
                    println!(
                        "BENCH frames={} capture={}ms",
                        frame_count, elapsed
                    );
                }
            }
            continue;
        }

        #[allow(unreachable_code)]
        let capture_start = Instant::now();
        if pipeline.capture_frame(frame).await.is_some() {
            let capture_ms = capture_start.elapsed().as_millis();
            // Stop the DMA while the CPU is busy converting and drawing.
            // The camera keeps streaming; the next capture waits for VSYNC.
            pipeline.stop();
            started = false;
            frame_count = frame_count.wrapping_add(1);

            let now = Instant::now();
            let elapsed_ms = (now - last_frame_time).as_millis().max(1);
            fps = (1000 / elapsed_ms).min(999) as u32;
            last_frame_time = now;

            if frame_count.is_multiple_of(60) {
                println!(
                    "frames={} fps={} capture={}ms mode={:?}",
                    frame_count, fps, capture_ms, mode
                );
            }

            // Repaint the small FPS field at most once per second, and only
            // when the value actually changed.
            if last_fps_update.elapsed() >= Duration::from_secs(1) {
                if fps != displayed_fps {
                    display.update_fps(fps);
                    displayed_fps = fps;
                }
                last_fps_update = Instant::now();
            }

            let analyse_start = Instant::now();
            let analysis = camera::analyse(frame, mode, &mut detector);
            let analyse_ms = analyse_start.elapsed().as_millis();
            let overlay = Overlay {
                motion: analysis.motion,
                face: analysis.face,
            };

            let draw_start = Instant::now();
            // One contiguous SPI write for the whole preview area. The status
            // bar is not touched and overlays are composited into each line.
            display.draw_yuv422_frame(frame, &mut line, overlay);
            let draw_ms = draw_start.elapsed().as_millis();

            if frame_count.is_multiple_of(60) {
                println!("  analyse={}ms draw={}ms", analyse_ms, draw_ms);
            }

            led.set_high();
        } else {
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}
