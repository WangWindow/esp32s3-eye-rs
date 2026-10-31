//! Board SD wiring and card initialisation.
//!
//! The generic `sd-card` crate only sees a configured SDMMC slot; this module
//! owns the ESP32-S3 host/slot setup and the embassy-time delay adapter.

use embassy_time::{Duration, Timer};
use esp_hal::{
    Async,
    peripherals::{GPIO38, GPIO39, GPIO40, SDHOST},
    sdmmc::{Config as HostConfig, SdHostController, Slot, SlotConfig},
};
use esp_println::println;
use sd_card::SdCard;
use static_cell::StaticCell;

static SD_HOST: StaticCell<SdHostController<'static>> = StaticCell::new();

pub use sd_card::SelfTest;

/// `sdio` needs an async delay; embassy-time already owns the timer.
pub struct EmbassyDelay;

impl embedded_hal_async::delay::DelayNs for EmbassyDelay {
    async fn delay_ns(&mut self, ns: u32) {
        Timer::after(Duration::from_nanos(ns as u64)).await;
    }
}

pub type Storage = SdCard<Slot<'static, 0, Async>, EmbassyDelay>;

/// Board-level SD wiring.
pub struct SdPeripherals {
    pub host: SDHOST<'static>,
    pub clk: GPIO39<'static>,
    pub cmd: GPIO38<'static>,
    pub d0: GPIO40<'static>,
}

pub async fn init(sd: SdPeripherals) -> Option<Storage> {
    println!("Initialising SD card…");

    let host = match SdHostController::new(sd.host, HostConfig::default()) {
        Ok(host) => host,
        Err(error) => {
            println!("SD host init failed: {:?}", error);
            return None;
        }
    };
    let host: &'static SdHostController<'static> = SD_HOST.init(host);

    let slot = match host.slot::<0>(SlotConfig::default()) {
        Ok(slot) => slot
            .with_clk(sd.clk)
            .with_cmd(sd.cmd)
            .with_data0(sd.d0)
            .into_async(),
        Err(error) => {
            println!("SD slot config failed: {:?}", error);
            return None;
        }
    };

    match SdCard::new(slot, EmbassyDelay, crate::SD_FREQUENCY_HZ).await {
        Ok(storage) => {
            println!("SD: {} MiB", storage.capacity_mib());
            Some(storage)
        }
        Err(error) => {
            println!("SD init failed: {:?}", error);
            None
        }
    }
}
