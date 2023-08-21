mod adb;
mod bus;
mod capabilities;
mod config;
mod device;
mod drivers;
mod gpio;
mod rpc;
mod tests;

use config::{ConfigError, Configuration};
use device::DeviceServer;
use gpio::{GpioBorrowChecker, PinState};
use log::{error, info, warn, LevelFilter, debug};
use parking_lot::RwLock;
use rpc::reflection::{device_reflection_server::DeviceReflectionServer, DeviceReflectionService};
use simple_logger::SimpleLogger;
use std::{
    error::Error,
    fs::{self, File},
    io::{BufReader, BufWriter},
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc;
use tonic::transport::Server;
use uuid::Uuid;

use crate::{
    adb::{AdbServer, PortType},
    device::Device,
    drivers::sysfs_led::SysfsLedController,
    rpc::{
        gps::{gps_server::GpsServer, GpsService},
        heartbeat::{heartbeat_server::HeartbeatServer, HeartbeatService},
        led::{led_controller_server::LedControllerServer, LEDControllerService},
        network::{network_manager_server::NetworkManagerServer, NetworkManagerService},
    },
};
use bus::i2c::I2CBusController;
use bus::i2c_sysfs::SysfsI2CBusController;
use bus::pwm::PWMBusController;
use bus::pwm_sysfs::SysfsPWMBusController;
use bus::raw::RawBusController;
use bus::raw_sysfs::SysfsRawBusController;
use bus::uart::UARTBusController;
use bus::BusController;

const CONFIG_PATH: &str = "nvos_config.json";

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    SimpleLogger::new()
        .with_colors(true)
        .with_level(LevelFilter::Debug)
        .init()?;

    info!("Loading configuration file at {}", CONFIG_PATH);
    let mut config;

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
        warn!("Config does not have any GPIO entries. This will not work.");
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

    info!("Building server");
    let mut device_server = DeviceServer::new();

    info!("Registering bus controllers");
    if config.controller_section.controllers.len() == 0 {
        warn!("Config does not have any bus controller entries.");
    }

    for bus_config in &mut config.controller_section.controllers {
        info!("Initializing bus controller \"{}\"", bus_config.name);
        let controller_instance: Result<Arc<RwLock<dyn BusController>>, String> =
            match bus_config.name.to_lowercase().as_str() {
                "raw" => RawBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "raw_sysfs" => SysfsRawBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "pwm" => PWMBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "pwm_sysfs" => SysfsPWMBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "uart" => UARTBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "i2c" => I2CBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                "i2c_sysfs" => SysfsI2CBusController::from_config(&gpio_borrow, bus_config)
                    .map(|bus| Arc::new(RwLock::new(bus)) as Arc<RwLock<dyn BusController>>)
                    .map_err(|err| err.to_string()),
                unknown_bus => Err(format!(
                    "Bus controller {} is not implemented by this server",
                    unknown_bus
                )),
            };

        match controller_instance {
            Ok(b) => match device_server.register_bus(b) {
                Ok(_) => info!("Bus controller \"{}\" is OK", bus_config.name),
                Err(e) => error!(
                    "Failed to register bus controller \"{}\": {}",
                    bus_config.name, e
                ),
            },
            Err(e) => error!(
                "Failed to build bus controller \"{}\": {}",
                bus_config.name, e
            ),
        }
    }

    info!("Registering devices");
    if config.device_section.devices.len() == 0 {
        warn!("Config does not have any device entries.");
    }

    for device_config in &mut config.device_section.devices {
        info!("Initializing device: (driver: {})", device_config.driver);
        let device_instance: Result<Box<dyn Device>, String> =
            match device_config.driver.to_lowercase().as_str() {
                "sysfs_generic_led" => SysfsLedController::from_config(device_config)
                    .map(|device| Box::new(device) as Box<dyn Device>)
                    .map_err(|err| err.to_string()),
                unknown_driver => Err(format!(
                    "Device driver {} is not implemented by this server",
                    unknown_driver
                )),
            };

        match device_instance {
            Ok(d) => match device_server.register_device(d) {
                Ok(id) => {
                    info!("Device (driver: {}) is OK", device_config.driver);
                    debug!("Device assigned address is {}", id);
                    match device_server.get_device(&id) {
                        Some(device) => {
                            debug!("Device capabilities:");
                            for cap in device.get_capabilities() {
                                debug!("  - {:?}", cap);
                            }
                        }
                        None => warn!("Failed to list device capabilities: device not found")
                    }
                },
                Err(e) => error!(
                    "Failed to register device (driver: {}): {}",
                    device_config.driver, e
                ),
            },
            Err(e) => error!(
                "Failed to build device (driver: {}): {}",
                device_config.driver, e
            ),
        }
    }

    info!("Syncing config to disk");
    if Path::new(CONFIG_PATH).exists() {
        // Backup config
        let backup_path = CONFIG_PATH.to_string() + ".bak";
        match fs::copy(CONFIG_PATH, &backup_path) {
            Ok(_) => info!("Backed up config file to {}", backup_path),
            Err(err) => warn!("Failed to backup config file: {}", err),
        }
    }

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

    info!("Starting device server");
    // Prepare the device server for multi threading
    let device_server = Arc::new(RwLock::new(device_server));

    info!("Starting ADB server connection");
    let adb_server = AdbServer::with_timeout(
        &config.adb_section.server_host,
        config.adb_section.server_port,
        Duration::from_millis(config.adb_section.read_timeout_ms),
        Duration::from_millis(config.adb_section.write_timeout_ms),
    );
    info!("Forwarding gRPC server port");
    match adb_server.add_port(
        PortType::Reverse,
        config.rpc_section.server_port,
        config.rpc_section.server_port,
        false,
    ) {
        Ok(_) => info!("Port forwarded: {}", config.rpc_section.server_port),
        Err(err) => error!("Failed to forward port: {}", err),
    }

    // Prepare the ADB server for multi threading
    let adb_server = Arc::new(RwLock::new(adb_server));

    // Prepare shutdown hook
    let (shutdown_tx, mut shutdown_rx) = mpsc::channel::<()>(1);
    let device_server_ref = device_server.clone();
    let adb_server_ref = adb_server.clone();
    let mut tried_graceful_shutdown = false;
    let ctrlc_result = ctrlc::set_handler(move || {
        info!("Received shutdown signal");
        if tried_graceful_shutdown {
            info!("Already tried graceful shutdown, forcibly shutting down.");
            panic!("Terminated");
        }

        tried_graceful_shutdown = true;

        info!("Shutting down device server");
        let mut ds = device_server_ref.write();
        for id in ds
            .get_devices()
            .iter()
            .map(|(k, _)| **k)
            .collect::<Vec<Uuid>>()
            .clone()
        {
            info!("Unloading device {}", id);
            if let Err(err) = ds.remove_device(&id) {
                error!("Failed to gracefully shutdown device {}: {}", id, err);
            }
        }

        info!("Shutting down ADB server");
        adb_server_ref.write().shutdown();

        info!("Gracefully shutting down RPC server");
        let _ = shutdown_tx.send(());
    });

    match ctrlc_result {
        Ok(_) => info!("Shutdown handler set"),
        Err(e) => warn!("Failed to set shutdown handler: {}", e),
    }

    // Serve gRPC
    let serve_addr =
        config.rpc_section.server_host + ":" + &config.rpc_section.server_port.to_string();
    let rpc_server = Server::builder()
        .tcp_nodelay(true)
        .accept_http1(true)
        .add_service(tonic_web::enable(DeviceReflectionServer::new(
            DeviceReflectionService::new(&device_server),
        )))
        .add_service(tonic_web::enable(LedControllerServer::new(
            LEDControllerService::new(&device_server),
        )))
        .add_service(tonic_web::enable(GpsServer::new(GpsService::new(
            &device_server,
        ))))
        .add_service(tonic_web::enable(NetworkManagerServer::new(
            NetworkManagerService::new(&adb_server),
        )))
        .add_service(tonic_web::enable(HeartbeatServer::new(
            HeartbeatService::new(),
        )))
        .serve_with_shutdown(serve_addr.parse().unwrap(), async {
            let _ = shutdown_rx.recv().await;
        });

    info!("Server running on {}!", serve_addr);
    rpc_server.await?;
    Ok(())
}
