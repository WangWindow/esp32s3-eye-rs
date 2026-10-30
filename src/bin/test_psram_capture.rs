#![no_std]
#![no_main]

use embassy_executor::Spawner;
use embassy_time::{Duration, Timer};
use esp32s3_cam_async::AsyncCameraDriver;
use esp_backtrace as _;
use esp_hal::{
    Blocking,
    dma::{DmaChannel, DmaDescriptor, DmaRxBuf},
    i2c::master::{Config as I2cConfig, I2c},
    interrupt::software::SoftwareInterruptControl,
    timer::timg::TimerGroup,
};
use esp_println::println;
use esp32s3_eye::{
    board::CAM_FRAME_BYTES,
    camera,
};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();
static mut DESC: [DmaDescriptor; 40] = [DmaDescriptor::EMPTY; 40];

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    println!("CAPTURE TEST: init");
    let psram = esp_hal::psram::Psram::new(p.PSRAM, esp_hal::psram::PsramConfig::default());
    let (ptr, size) = psram.raw_parts();
    println!("CAPTURE TEST: psram {} bytes", size);
    let frame: &'static mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(ptr, CAM_FRAME_BYTES) };
    let descriptors: &'static mut [DmaDescriptor] =
        unsafe { &mut *core::ptr::addr_of_mut!(DESC) };
    let rx_buf = DmaRxBuf::new(descriptors, frame).expect("DmaRxBuf");

    let timg0 = TimerGroup::new(p.TIMG0);
    let sw = SoftwareInterruptControl::new(p.SW_INTERRUPT);
    esp_rtos::start(timg0.timer0, sw.software_interrupt0);

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
    .expect("camera driver");

    let i2c = I2c::new(
        p.I2C0,
        I2cConfig::default().with_frequency(esp_hal::time::Rate::from_khz(400)),
    )
    .expect("I2C")
    .with_sda(p.GPIO4)
    .with_scl(p.GPIO5);

    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());
    executor.run(|spawner: Spawner| {
        spawner.spawn(run(camera, i2c, rx_buf).unwrap());
    });
}

#[embassy_executor::task]
async fn run(
    camera: AsyncCameraDriver<'static>,
    mut i2c: I2c<'static, Blocking>,
    rx_buf: DmaRxBuf,
) {
    println!("CAPTURE TEST: task");
    camera::init_sensor(&mut i2c).await.expect("sensor");
    println!("CAPTURE TEST: sensor ready");

    let transfer = camera.receive(rx_buf).expect("receive");
    println!("CAPTURE TEST: dma started");
    let (result, _camera, rx_buf) = transfer.wait().await;
    println!(
        "CAPTURE TEST: done result={:?} bytes={} y={}",
        result,
        rx_buf.number_of_received_bytes(),
        rx_buf.as_slice()[100 * 640]
    );

    loop {
        println!("capture test tick");
        Timer::after(Duration::from_secs(1)).await;
    }
}
