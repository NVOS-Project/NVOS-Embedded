use std::net::ToSocketAddrs;
use std::{collections::HashMap, net::IpAddr};
use std::fmt::Display;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::io::{Read, Write};

#[derive(Debug, PartialEq)]
pub enum ConfigError {
    SerializeError(String),
    InvalidEntry(String),
    MissingEntry(String),
    DuplicateEntry(String),
    Other(String)
}

impl Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            ConfigError::SerializeError(msg) => format!("serialize/parse error: {}", msg),
            ConfigError::InvalidEntry(msg) => format!("invalid config entry: {}", msg),
            ConfigError::MissingEntry(msg) => format!("missing config entry: {}", msg),
            ConfigError::DuplicateEntry(msg) => format!("duplicate config entry: {}", msg),
            ConfigError::Other(msg) => format!("config error: {}", msg)
        })
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigSectionRPC {
    pub server_host: String,
    pub server_port: u16
}

impl ConfigSectionRPC {
    pub fn new(server_host: String, server_port: u16) -> Self {
        Self { server_host, server_port }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Err(err) = self.server_host.parse::<IpAddr>() {
            return Err(ConfigError::InvalidEntry(format!("failed to parse server host: {}", err)));
        }

        if self.server_port == 0 {
            return Err(ConfigError::InvalidEntry("invalid server port".to_string()));
        }

        Ok(())
    }
}

impl Default for ConfigSectionRPC {
    fn default() -> Self {
        Self::new("0.0.0.0".to_string(), 30000)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConfigSectionADB {
    pub server_host: String,
    pub server_port: u16,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64
}

impl ConfigSectionADB {
    pub fn new(server_host: String, server_port: u16, read_timeout_ms: u64, write_timeout_ms: u64) -> Self {
        Self { server_host, server_port, read_timeout_ms, write_timeout_ms }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if let Err(err) = format!("{}:{}", self.server_host, self.server_port).to_socket_addrs() {
            return Err(ConfigError::InvalidEntry(format!("failed to parse server host: {}", err)));
        }

        if self.server_port == 0 {
            return Err(ConfigError::InvalidEntry("invalid server port".to_string()));
        }

        Ok(())
    }
}

impl Default for ConfigSectionADB {
    fn default() -> Self {
        Self::new("localhost".to_string(), 5037, 1000, 1000)
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ConfigSectionGPIO {
    pub pin_config: HashMap<u8, u8>
}

impl ConfigSectionGPIO {
    pub fn new(pin_config: HashMap<u8, u8>) -> Self {
        Self { pin_config }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut known_pin_ids = Vec::new();
        let mut known_bcm_ids = Vec::new();

        for (id, bcm) in &self.pin_config {
            if known_pin_ids.contains(&id) {
                return Err(ConfigError::InvalidEntry(
                    format!("invalid pin configuration: ({} -> {}), pin ID {} is defined more than once", id, bcm, bcm)
                ));
            }

            if known_bcm_ids.contains(&bcm) {
                return Err(ConfigError::InvalidEntry(
                    format!("invalid pin configuration: ({} -> {}), pin BCM ID {} is defined more than once", id, bcm, bcm)
                ));
            }

            known_pin_ids.push(id);
            known_bcm_ids.push(bcm);
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeviceConfig {
    pub driver: String,
    pub friendly_name: Option<String>,
    pub driver_data: Value
}

impl DeviceConfig {
    pub fn new(driver: String, friendly_name: Option<String>, driver_data: Value) -> Self {
        Self { driver, friendly_name, driver_data }
    }

    pub fn new_without_data(driver: String, friendly_name: Option<String>) -> Self {
        Self { driver, friendly_name, driver_data: Value::Null }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.driver.trim().is_empty() {
            return Err(ConfigError::InvalidEntry("invalid device config: driver name cannot be empty".to_string()));
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ConfigSectionDevices {
    pub devices: Vec<DeviceConfig>
}

impl ConfigSectionDevices {
    pub fn new(devices: Vec<DeviceConfig>) -> Self {
        Self { devices }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        for device in &self.devices {
            device.validate()?;
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct BusControllerConfig {
    pub name: String,
    pub data: Value
}

impl BusControllerConfig {
    pub fn new(bus: String, data: Value) -> Self {
        Self { name: bus, data }
    }

    pub fn new_without_data(bus: String) -> Self {
        Self { name: bus, data: Value::Null }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.name.trim().is_empty() {
            return Err(ConfigError::InvalidEntry("invalid bus controller config: bus name cannot be empty".to_string()));
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct ConfigSectionControllers {
    pub controllers: Vec<BusControllerConfig>   
}

impl ConfigSectionControllers {
    pub fn new(controllers: Vec<BusControllerConfig>) -> Self {
        Self { controllers }
    }

    pub fn validate(&self) -> Result<(), ConfigError> {
        let mut seen_names = Vec::new();
        for name in self.controllers.iter().map(|x| &x.name) {
            if seen_names.contains(&name) {
                return Err(ConfigError::DuplicateEntry(format!("bus controller {} is defined more than once", name)));
            }

            seen_names.push(name);
        }

        for bus in &self.controllers {
            bus.validate()?;
        }

        Ok(())
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct Configuration {
    pub rpc_section: ConfigSectionRPC,
    pub adb_section: ConfigSectionADB,
    pub gpio_section: ConfigSectionGPIO,
    pub device_section: ConfigSectionDevices,
    pub controller_section: ConfigSectionControllers
}

impl Configuration {
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.rpc_section.validate()?;
        self.adb_section.validate()?;
        self.gpio_section.validate()?;
        self.device_section.validate()?;
        self.controller_section.validate()?;
        Ok(())
    }

    pub fn from_reader<R: Read>(reader: R) -> Result<Configuration, ConfigError> {
        let config: Configuration = match serde_json::from_reader(reader) {
            Ok(c) => c,
            Err(e) => {
                return Err(ConfigError::SerializeError(
                    format!("failed to deserialize config file: {}", e)
                ));
            }
        };
    
        config.validate()?;
        Ok(config)
    }

    pub fn from_str(json_str: String) -> Result<Configuration, ConfigError> {
        Self::from_reader(json_str.as_bytes())        
    }

    pub fn to_writer<W: Write>(&self, writer: W, pretty: bool) -> Result<(), ConfigError> {
        let result;
        if pretty {
            result = serde_json::to_writer_pretty(writer, self);
        } else {
            result = serde_json::to_writer(writer, self);
        }
        
        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(ConfigError::SerializeError(
                format!("failed to serialize config: {}", e)
            ))
        }
    }

    pub fn to_str(&self, pretty: bool) -> Result<String, ConfigError> {
        let result;
        if pretty {
            result = serde_json::to_string_pretty(self);
        } else {
            result = serde_json::to_string(self);
        }

        match result {
            Ok(s) => Ok(s),
            Err(e) => Err(ConfigError::SerializeError(
                format!("failed to serialize config: {}", e)
            )),
        }
    }
}