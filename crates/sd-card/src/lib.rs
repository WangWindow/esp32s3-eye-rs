//! Generic SD card block access and a non-destructive read/write self-test.
//!
//! The crate is HAL-agnostic: it wraps `sdio`'s SD protocol/block device over
//! any `MmcBus` implementation and an async delay. Board pin mapping and host
//! configuration stay in the application.

#![no_std]

use aligned::Aligned;
use block_device_driver::BlockDevice as _;
use embedded_hal_async::delay::DelayNs;
use sdio::{MmcBus, MmcError};

type Card<B, D> = sdio::DefaultBlockDevice<sdio::sd::Card, B, D>;

#[derive(Debug)]
pub enum SdCardError {
    Card(MmcError),
}

impl From<MmcError> for SdCardError {
    fn from(error: MmcError) -> Self {
        Self::Card(error)
    }
}

/// Result of the non-destructive read/write self-test.
#[derive(Clone, Copy, Debug, Default)]
pub struct SelfTest {
    pub read_ok: bool,
    pub write_ok: bool,
}

pub struct SdCard<B: MmcBus, D: DelayNs> {
    card: Card<B, D>,
    capacity: u64,
}

impl<B: MmcBus, D: DelayNs> SdCard<B, D> {
    /// Initialise the card on an already-configured SD/MMC bus.
    pub async fn new(bus: B, delay: D, frequency_hz: u32) -> Result<Self, SdCardError> {
        let mut card = Card::new(bus, delay, frequency_hz).await?;
        let capacity = card.size().await?;

        Ok(Self { card, capacity })
    }

    pub fn capacity_bytes(&self) -> u64 {
        self.capacity
    }

    pub fn capacity_mib(&self) -> u32 {
        (self.capacity / (1024 * 1024)) as u32
    }

    /// Read block 0, then write/verify/restore the last block. The restore is
    /// attempted even if the verify fails, so a formatted card is not left
    /// with a test pattern in a potentially allocated block.
    pub async fn self_test(&mut self) -> Result<SelfTest, SdCardError> {
        let mut first = Aligned([0u8; 512]);
        self.card.read(0, core::slice::from_mut(&mut first)).await?;

        let blocks = (self.capacity / 512).max(2) as u32;
        let last = blocks - 1;

        let mut original = Aligned([0u8; 512]);
        self.card
            .read(last, core::slice::from_mut(&mut original))
            .await?;

        let mut pattern = Aligned([0u8; 512]);
        for (index, byte) in pattern.iter_mut().enumerate() {
            *byte = (index as u8) ^ 0xA5;
        }

        self.card
            .write(last, core::slice::from_ref(&pattern))
            .await?;

        let mut verify = Aligned([0u8; 512]);
        self.card
            .read(last, core::slice::from_mut(&mut verify))
            .await?;
        let write_ok = verify == pattern;

        // Best-effort restore; report the verify result, not the restore error.
        let _ = self
            .card
            .write(last, core::slice::from_ref(&original))
            .await;

        Ok(SelfTest {
            read_ok: true,
            write_ok,
        })
    }
}
