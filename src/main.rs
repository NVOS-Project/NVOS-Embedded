mod device;
mod bus;
mod gpio;
mod tests;

use std::{error::Error, collections::HashMap};
use bus::raw::RawBusController;
use gpio::{GpioBorrowChecker, PinState};
use device::DeviceServerBuilder;

// TODO: implement loading from persistent storage
fn load_pin_config() -> Result<HashMap<u8, PinState>, Box<dyn Error>> {
    let mut pins = HashMap::new();
    for i in 1..=40 {
        let pin = PinState::new(i, i+20);
        pins.insert(i, pin);
    }

    Ok(pins)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Building GPIO borrow checker");
    let pins = load_pin_config()?;
    let gpio_borrow = GpioBorrowChecker::new_rc(pins);
    println!("Building device server");
    let device_server = DeviceServerBuilder::configure()
        .add_bus(RawBusController::new(&gpio_borrow)?)
        .build()?;

    // TODO: add gRPC stuff
    println!("Server running!");
    Ok(())
}