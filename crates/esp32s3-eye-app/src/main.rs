#![no_std]
#![no_main]
#![deny(clippy::large_stack_frames)]

mod app;
mod display;
mod geometry;
mod graphics;
mod modes;
mod vision;

use esp32s3_eye_board::Board;
use static_cell::StaticCell;

use esp_backtrace as _;

esp_bootloader_esp_idf::esp_app_desc!();

static EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

#[esp_hal::main]
fn main() -> ! {
    let board = Board::init();
    let executor = EXECUTOR.init(esp_rtos::embassy::Executor::new());

    executor.run(|spawner| {
        spawner.spawn(app::task(board).expect("spawn app task"));
    });
}
