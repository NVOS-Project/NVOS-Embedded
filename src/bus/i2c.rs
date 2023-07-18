use crate::bus::BusController;
use crate::gpio::GpioBorrowChecker;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use uuid::Uuid;
use rppal::i2c::{I2c, Error};

pub struct I2CPinDefinition(u8, u8);
impl I2CPinDefinition {
    pub fn overlap(&self, other: &Self) -> bool {
        self.0 == other.0 ||
        self.1 == other.1 ||
        self.0 == other.1 ||
        self.1 == other.0
    }

    pub fn to_vec(&self) -> Vec<u8> {
        vec![self.0, self.1]
    }

    pub fn to_arr(&self) -> [u8; 2] {
        [self.0, self.1]
    }
}

struct I2cInfo {
    bus_id: u8,
    lease_id: Uuid,
    bus: Rc<RefCell<I2c>>
}

#[derive(Debug, PartialEq)]
pub enum I2CError {
    InvalidConfig(String),
    BusNotFound(u8),
    LeaseNotFound,
    InvalidAddress(u16),
    NotSupported,
    Busy,
    HardwareError(String),
    Other(String)
}

impl I2cInfo {
    pub fn new(bus_id: u8, lease_id: Uuid, bus: I2c) -> Self {
        Self::with_rc(bus_id, lease_id, Rc::new(RefCell::new(bus)))
    }

    pub fn with_rc(bus_id: u8, lease_id: Uuid, bus: Rc<RefCell<I2c>>) -> Self {
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
    gpio_borrow: Rc<RefCell<GpioBorrowChecker>>,
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
}

impl I2CBusController {
    pub fn new(gpio_borrow: &Rc<RefCell<GpioBorrowChecker>>, pin_config: HashMap<u8, I2CPinDefinition>) -> Result<Self, I2CError> {        
        let gpio_checker = gpio_borrow.borrow();

        for (bus_id, definition) in &pin_config {
            if definition.0 == definition.1 {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use the same pin twice: bus {} -> (SDA: {}. SCL: {})",
                    bus_id, definition.0, definition.1
                )));
            }

            if !gpio_checker.has_pin(definition.0) {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use invalid pin: bus {} pin {} (SDA)",
                    bus_id, definition.0
                )));
            }

            if !gpio_checker.has_pin(definition.1) {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use invalid pin: bus {} pin {} (SCL)",
                    bus_id, definition.1
                )));
            }

            for (other_bus_id, other_definition) in &pin_config {
                if bus_id != other_bus_id && definition.overlap(other_definition) {
                    return Err(I2CError::InvalidConfig(
                        format!("I2C bus pin definitions overlap: bus {} -> (SDA: {}, SCL: {}) with bus {} -> (SDA: {}, SCL: {})",
                        bus_id, definition.0, definition.1, other_bus_id, other_definition.0, other_definition.1
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

    fn open(&mut self, bus_id: u8) -> Result<Rc<RefCell<I2c>>, I2CError> {
        if self.owned_buses.contains_key(&bus_id) {
            return Err(I2CError::Busy);
        }

        let definition = match self.pin_config.get(&bus_id) {
            Some(v) => v,
            None => return Err(I2CError::BusNotFound(bus_id))
        };

        let mut borrow_checker = self.gpio_borrow.borrow_mut();
        if !borrow_checker.can_borrow_many(&definition.to_arr()) {
            return Err(I2CError::Busy);
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

    pub fn get(&mut self, bus_id: u8) -> Result<Rc<RefCell<I2c>>, I2CError> {
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

        let mut borrow_checker = self.gpio_borrow.borrow_mut();
        borrow_checker.release(&info.lease_id)
            .map_err(|err| I2CError::HardwareError(err.to_string()))?;

        self.owned_buses.remove(&bus_id);
        Ok(())
    }
}