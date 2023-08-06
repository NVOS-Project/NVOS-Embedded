use crate::bus::BusController;
use crate::gpio::{GpioBorrowChecker, GpioError};
use std::fmt::Display;
use std::{any::Any, sync::Arc};
use std::collections::HashMap;
use parking_lot::{Mutex, RwLock};
use uuid::Uuid;
use rppal::i2c::{I2c, Error};

pub struct I2CPinDefinition {
    sda: u8,
    scl: u8
}

impl I2CPinDefinition {
    pub fn new(sda: u8, scl: u8) -> Self {
        I2CPinDefinition { sda, scl }
    }

    pub fn overlap(&self, other: &Self) -> bool {
        self.sda == other.sda ||
        self.scl == other.scl ||
        self.sda == other.scl ||
        self.scl == other.sda
    }

    pub fn to_vec(&self) -> Vec<u8> {
        vec![self.sda, self.scl]
    }

    pub fn to_arr(&self) -> [u8; 2] {
        [self.sda, self.scl]
    }
}

struct I2cInfo {
    bus_id: u8,
    lease_id: Uuid,
    bus: Arc<Mutex<I2c>>
}

#[derive(Debug, PartialEq)]
pub enum I2CError {
    InvalidConfig(String),
    BusNotFound(u8),
    LeaseNotFound,
    InvalidAddress(u16),
    NotSupported,
    ChannelBusy(u8),
    HardwareError(String),
    Other(String)
}

impl Display for I2CError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            I2CError::InvalidConfig(msg) => format!("invalid config: {}", msg),
            I2CError::BusNotFound(channel_id) => format!("I2C channel {} does not exist", channel_id),
            I2CError::LeaseNotFound => format!("specified I2C channel is not open"),
            I2CError::InvalidAddress(device_address) => format!("invalid slave address: {}", device_address),
            I2CError::NotSupported => format!("not supported"),
            I2CError::ChannelBusy(channel_id) => format!("I2C channel {} is busy", channel_id),
            I2CError::HardwareError(msg) => format!("hardware error: {}", msg),
            I2CError::Other(msg) => format!("{}", msg),
        })
    }
}

impl I2cInfo {
    fn new(bus_id: u8, lease_id: Uuid, bus: I2c) -> Self {
        Self::with_rc(bus_id, lease_id, Arc::new(Mutex::new(bus)))
    }

    fn with_rc(bus_id: u8, lease_id: Uuid, bus: Arc<Mutex<I2c>>) -> Self {
        I2cInfo { bus_id, lease_id, bus }
    }
}

fn rppal_map_err(err: Error, default_err_msg: &str) -> I2CError {
    match err {
        Error::Io(e) => I2CError::HardwareError(format!("I/O error: {}", e)),
        Error::InvalidSlaveAddress(addr) => I2CError::InvalidAddress(addr),
        Error::FeatureNotSupported => I2CError::NotSupported,
        _ => I2CError::Other(default_err_msg.to_string())
    }
}

pub struct I2CBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    pin_config: HashMap<u8, I2CPinDefinition>,
    owned_buses: HashMap<u8, I2cInfo>
}

impl BusController for I2CBusController {
    fn name(&self) -> String {
        "I2C".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl I2CBusController {
    pub fn new(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>, pin_config: HashMap<u8, I2CPinDefinition>) -> Result<Self, I2CError> {        
        let gpio_checker = gpio_borrow.read();

        for (bus_id, definition) in &pin_config {
            if definition.sda == definition.scl {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use the same pin twice: bus {} -> (SDA: {}. SCL: {})",
                    bus_id, definition.sda, definition.scl
                )));
            }

            if !gpio_checker.has_pin(definition.sda) {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use invalid pin: bus {} pin {} (SDA)",
                    bus_id, definition.sda
                )));
            }

            if !gpio_checker.has_pin(definition.scl) {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use invalid pin: bus {} pin {} (SCL)",
                    bus_id, definition.scl
                )));
            }

            for (other_bus_id, other_definition) in &pin_config {
                if bus_id != other_bus_id && definition.overlap(other_definition) {
                    return Err(I2CError::InvalidConfig(
                        format!("I2C bus pin definitions overlap: bus {} -> (SDA: {}, SCL: {}) with bus {} -> (SDA: {}, SCL: {})",
                        bus_id, definition.sda, definition.scl, other_bus_id, other_definition.sda, other_definition.scl
                    )));
                }
            }
        }

        Ok(I2CBusController { 
            gpio_borrow: gpio_borrow.clone(), 
            pin_config: pin_config, 
            owned_buses: HashMap::new()
        })
    }

    fn open(&mut self, bus_id: u8) -> Result<Arc<Mutex<I2c>>, I2CError> {
        if self.owned_buses.contains_key(&bus_id) {
            return Err(I2CError::ChannelBusy(bus_id));
        }

        let definition = match self.pin_config.get(&bus_id) {
            Some(v) => v,
            None => return Err(I2CError::BusNotFound(bus_id))
        };

        let mut borrow_checker = self.gpio_borrow.write();
        if !borrow_checker.can_borrow_many(&definition.to_arr()) {
            return Err(I2CError::HardwareError("I2C channel pins are already in use".to_string()));
        }

        let bus = I2c::with_bus(bus_id)
            .map_err(|err| rppal_map_err(err, &format!("Internal RPPAL error while opening I2C bus {}", bus_id)))?;

        let borrow_id = borrow_checker.borrow_many(definition.to_vec())
            .map_err(|err| I2CError::HardwareError(err.to_string()))?;

        let bus_info = I2cInfo::new(bus_id, borrow_id, bus);
        let result = bus_info.bus.clone();
        self.owned_buses.insert(bus_id, bus_info);
        Ok(result)
    }

    pub fn get(&mut self, bus_id: u8) -> Result<Arc<Mutex<I2c>>, I2CError> {
        let res = self.owned_buses.get(&bus_id);
        let bus = match res {
            Some(info) => info.bus.clone(),
            None => self.open(bus_id)?
        };

        Ok(bus)
    }

    pub fn close(&mut self, bus_id: u8) -> Result<(), I2CError> {
        let info = match self.owned_buses.get(&bus_id) {
            Some(info) => info,
            None => return Err(I2CError::LeaseNotFound)
        };

        let mut borrow_checker = self.gpio_borrow.write();
        borrow_checker.release(&info.lease_id)
            .map_err(|err| I2CError::HardwareError(err.to_string()))?;

        self.owned_buses.remove(&bus_id);
        Ok(())
    }
}