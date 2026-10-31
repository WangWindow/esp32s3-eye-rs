//! Buttons on the ESP32-S3-EYE.
//!
//! The three front-panel keys (UP / PLAY / DOWN) are wired as an ADC ladder on
//! GPIO1 (ADC1 channel 0). BOOT is a normal active-low GPIO on GPIO0.

use esp_hal::{
    Blocking,
    analog::adc::{Adc, AdcConfig, AdcPin, Attenuation},
    gpio::{Input, InputConfig, Pull},
    peripherals::{ADC1, GPIO0, GPIO1},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Up,
    Play,
    Down,
    Boot,
}

/// Two consecutive scans must agree before a press is reported.
const DEBOUNCE_SCANS: u8 = 2;

pub struct Buttons<'d> {
    adc: Adc<'d, ADC1<'d>, Blocking>,
    adc_pin: AdcPin<GPIO1<'d>, ADC1<'d>>,
    boot: Input<'d>,
    candidate: Option<Button>,
    reported: Option<Button>,
    stable_scans: u8,
    last_raw: u16,
}

impl<'d> Buttons<'d> {
    pub fn new(adc1: ADC1<'d>, adc_pin: GPIO1<'d>, boot_pin: GPIO0<'d>) -> Self {
        let mut config = AdcConfig::new();
        let adc_pin = config.enable_pin(adc_pin, Attenuation::_11dB);
        let adc = Adc::new(adc1, config);
        let boot = Input::new(boot_pin, InputConfig::default().with_pull(Pull::Up));

        Self {
            adc,
            adc_pin,
            boot,
            candidate: None,
            reported: None,
            stable_scans: 0,
            last_raw: 0,
        }
    }

    /// Sample the buttons once. Returns a press event only on the falling edge.
    pub fn poll(&mut self) -> Option<Button> {
        self.last_raw = self.adc.read_blocking(&mut self.adc_pin);

        let candidate = if self.boot.is_low() {
            Some(Button::Boot)
        } else {
            classify(self.last_raw)
        };

        if candidate != self.candidate {
            self.candidate = candidate;
            self.stable_scans = 0;
            return None;
        }

        self.stable_scans = self.stable_scans.saturating_add(1);
        if self.stable_scans < DEBOUNCE_SCANS {
            return None;
        }

        if candidate != self.reported {
            self.reported = candidate;
            if candidate.is_some() {
                return candidate;
            }
        }

        None
    }

    pub fn last_raw(&self) -> u16 {
        self.last_raw
    }
}

fn classify(raw: u16) -> Option<Button> {
    // The esp-bsp `button_adc` configuration gives the ladder voltages in mV:
    // UP = 2.41 V, PLAY = 1.98 V, DOWN = 0.82 V. Convert the 12-bit reading
    // back to millivolts and pick the nearest centre.
    const LADDER: [(u16, Button); 3] = [
        (2410, Button::Up),
        (1980, Button::Play),
        (820, Button::Down),
    ];
    const TOLERANCE_MV: u16 = 200;
    // ESP32-S3 12 dB attenuation has an approximately 3.1 V full scale.
    const FULL_SCALE_MV: u32 = 3100;

    let millivolts = (raw as u32 * FULL_SCALE_MV / 4095) as u16;
    let mut best = None;
    let mut best_distance = TOLERANCE_MV;

    for (centre, button) in LADDER {
        let distance = millivolts.abs_diff(centre);
        if distance < best_distance {
            best = Some(button);
            best_distance = distance;
        }
    }

    best
}
