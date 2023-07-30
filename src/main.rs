mod tests;
mod device;
mod capabilities;
mod bus;
mod gpio;
mod rpc;

use std::{error::Error, collections::HashMap, sync::Arc};
use gpio::{GpioBorrowChecker, PinState};
use device::DeviceServerBuilder;
use parking_lot::RwLock;
use tonic::transport::Server;
use rpc::reflection::{device_reflection_server::DeviceReflectionServer, DeviceReflectionService};

const SERVE_ADDR: &str = "0.0.0.0:30000";

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
    let gpio_borrow = Arc::new(RwLock::new(GpioBorrowChecker::new(pins)));

    println!("Building device server");
    let device_server = Arc::new(RwLock::new(DeviceServerBuilder::configure()
        .build()
        .expect("failed to build device server")));

    // Serve gRPC
    let rpc_server = Server::builder()
        .tcp_nodelay(true)
        .add_service(DeviceReflectionServer::new(DeviceReflectionService::new(&device_server)))
        .serve(String::from(SERVE_ADDR).parse().unwrap());

    println!("Server running on {}!", SERVE_ADDR);
    rpc_server.await?;
    Ok(())
}