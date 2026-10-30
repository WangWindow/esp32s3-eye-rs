#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::dma::{DmaDescriptor, DmaRxBuf};
use esp_println::println;
use esp32s3_eye::board::CAM_FRAME_BYTES;

esp_bootloader_esp_idf::esp_app_desc!();

static mut DESC: [DmaDescriptor; 40] = [DmaDescriptor::EMPTY; 40];

#[esp_hal::main]
fn main() -> ! {
    let p = esp_hal::init(esp_hal::Config::default());
    println!("PSRAM DMA TEST: init");
    let psram = esp_hal::psram::Psram::new(p.PSRAM, esp_hal::psram::PsramConfig::default());
    let (ptr, size) = psram.raw_parts();
    println!("PSRAM DMA TEST: {} bytes", size);
    let buffer: &'static mut [u8] =
        unsafe { core::slice::from_raw_parts_mut(ptr, CAM_FRAME_BYTES) };
    let descriptors: &'static mut [DmaDescriptor] =
        unsafe { &mut *core::ptr::addr_of_mut!(DESC) };
    match DmaRxBuf::new(descriptors, buffer) {
        Ok(buf) => println!("PSRAM DMA TEST: OK len={}", buf.len()),
        Err(error) => println!("PSRAM DMA TEST: ERR {:?}", error),
    }
    loop {
        println!("psram dma tick");
        esp_hal::delay::Delay::new().delay_millis(1000);
    }
}
