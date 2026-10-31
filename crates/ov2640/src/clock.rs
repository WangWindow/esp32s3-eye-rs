//! Sensor clocking configuration — the knobs that set the capture / frame rate.

/// Clocking parameters that together determine how fast the OV2640 streams
/// pixels (and therefore the frame rate).
///
/// 1. **XCLK** ([`xclk_hz`](Self::xclk_hz)) — the master clock driven onto the
///    sensor's MCLK pin. Raising XCLK is the largest frame-rate lever; the
///    OV2640 runs cleanly up to ~24 MHz.
/// 2. **CLKRC** ([`clkrc`](Self::clkrc)) — the sensor's internal clock
///    pre-scalar (reg `0x11`, bits `[5:0]`). `0` = no pre-scale (fastest).
/// 3. **R_DVP_SP** ([`dvp_pclk`](Self::dvp_pclk)) — the DVP output clock divider
///    (reg `0xD3`). `None` keeps the validated per-format default.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockConfig {
    /// XCLK / MCLK frequency in Hz.
    pub xclk_hz: u32,
    /// CLKRC (reg `0x11`) internal clock pre-scalar, bits `[5:0]`.
    pub clkrc: u8,
    /// R_DVP_SP (reg `0xD3`) DVP clock divider override.
    pub dvp_pclk: Option<u8>,
}

impl ClockConfig {
    pub const fn new(xclk_hz: u32, clkrc: u8, dvp_pclk: Option<u8>) -> Self {
        Self {
            xclk_hz,
            clkrc,
            dvp_pclk,
        }
    }

    /// Conservative clocking: **10 MHz** XCLK, no PLL pre-scale.
    pub const BALANCED: Self = Self::new(10_000_000, 0, None);

    /// **20 MHz** XCLK, the value the OV2640 tables were calibrated for.
    pub const HIGH_FPS: Self = Self::new(20_000_000, 0, None);

    /// Alias for the highest validated preset (20 MHz).
    pub const MAX_FPS: Self = Self::HIGH_FPS;

    pub const fn xclk_hz(self) -> u32 {
        self.xclk_hz
    }
}

impl Default for ClockConfig {
    fn default() -> Self {
        Self::BALANCED
    }
}
