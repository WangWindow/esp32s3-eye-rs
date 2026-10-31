//! Board status LED.

use esp_hal::gpio::Output;

pub struct Led {
    pin: Output<'static>,
}

impl Led {
    pub fn new(pin: Output<'static>) -> Self {
        Self { pin }
    }

    pub fn on(&mut self) {
        self.pin.set_high();
    }

    pub fn off(&mut self) {
        self.pin.set_low();
    }
}
