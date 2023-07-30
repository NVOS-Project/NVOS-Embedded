mod tests;
mod device;
mod capabilities;
mod bus;
mod gpio;
//mod rpc;

use std::{error::Error, collections::HashMap, sync::{RwLock, Arc}};
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
    let gpio_borrow = GpioBorrowChecker::new_arc(pins);
    println!("Building device server");
    let device_server = DeviceServerBuilder::configure()
        .add_bus(RawBusController::new(&gpio_borrow).expect("failed to build RawBusController"))
        .build()
        .expect("failed to build device server");

    // TODO: add gRPC stuff
    println!("Server running!");
    Ok(())
}