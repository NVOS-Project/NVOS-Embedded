mod bus;
mod capabilities;
mod config;
mod device;
mod gpio;
mod rpc;
mod tests;

use config::{ConfigError, Configuration};
use device::DeviceServerBuilder;
use gpio::{GpioBorrowChecker, PinState};
use log::{error, info, warn, LevelFilter};
use parking_lot::RwLock;
use rpc::reflection::{device_reflection_server::DeviceReflectionServer, DeviceReflectionService};
use simple_logger::SimpleLogger;
use std::{
    collections::HashMap,
    error::Error,
    fs::File,
    io::{BufReader, BufWriter},
    path::Path,
    sync::Arc,
};
use tonic::transport::Server;

const SERVE_ADDR: &str = "0.0.0.0:30000";
const CONFIG_PATH: &str = "nvos_config.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_colors(true)
        .with_level(LevelFilter::Info)
        .init()?;

    info!("Loading configuration file at {}", CONFIG_PATH);
    let config;

    if !Path::new(CONFIG_PATH).exists() {
        warn!("Config file does not exist or is inaccessible");
        warn!("Creating default config file");
        config = Configuration::default();

        match File::create(CONFIG_PATH) {
            Ok(f) => {
                let writer = BufWriter::new(f);
                match config.to_writer(writer, true) {
                    Ok(_) => info!("Config file written to {}", CONFIG_PATH),
                    Err(e) => error!("Failed to write config file: {}", e),
                };
            }
            Err(e) => error!("Failed to open config file for write: {}", e),
        }
    } else {
        config = match File::open(CONFIG_PATH)
            .map_err(|err| ConfigError::Other(format!("failed to read config file: {}", err)))
            .and_then(|f| Configuration::from_reader(BufReader::new(f)))
        {
            Ok(c) => c,
            Err(e) => {
                error!(
                    "Failed to read config file at location {}: {}",
                    CONFIG_PATH, e
                );
                warn!("Using default config file instead.");
                Configuration::default()
            }
        };
    }

    info!("Building GPIO borrow checker");
    if config.gpio_section.pin_config.len() == 0 {
        warn!("Config does not have any GPIO entires. This will not work.");
    }
    
    let gpio_borrow = Arc::new(RwLock::new(GpioBorrowChecker::new(
        config
            .gpio_section
            .pin_config
            .iter()
            .map(|(pin_id, bcm_id)| {
                (
                    pin_id.clone(),
                    PinState::new(pin_id.clone(), bcm_id.clone()),
                )
            })
            .collect(),
    )));
    // TODO: build device server from config
    println!("Building device server");
    let device_server = Arc::new(RwLock::new(
        DeviceServerBuilder::configure()
            .build()
            .expect("failed to build device server"),
    ));

    // Serve gRPC
    let rpc_server = Server::builder()
        .tcp_nodelay(true)
        .add_service(DeviceReflectionServer::new(DeviceReflectionService::new(
            &device_server,
        )))
        .serve(String::from(SERVE_ADDR).parse().unwrap());

    println!("Server running on {}!", SERVE_ADDR);
    rpc_server.await?;
    Ok(())
}
