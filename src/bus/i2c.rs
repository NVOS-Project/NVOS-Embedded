use crate::bus::BusController;
use crate::gpio::{GpioBorrowChecker, GpioError};
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
}

struct I2cInfo {
    bus_id: u8,
    lease_id: Uuid,
    bus: I2c
}

#[derive(Debug, PartialEq)]
pub enum I2CError {
    InvalidConfig(String),
    BusNotFound(u8),
    InvalidAddress(u16),
    Busy,
    HardwareError,
    Other(String)
}

impl I2cInfo {
    pub fn new(bus_id: u8, lease_id: Uuid, bus: I2c) -> Self {
        I2cInfo { bus_id, lease_id, bus }
    }
}

pub struct I2CBusController {
    gpio_borrow: Rc<RefCell<GpioBorrowChecker>>,
    pin_config: HashMap<u8, I2CPinDefinition>,
    buses: HashMap<u8, I2cInfo>
}

impl BusController for I2CBusController {
    fn name(&self) -> String {
        "I2C".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn check_pin_config(pin_config: &HashMap<u8, I2CPinDefinition>) -> bool {
    for (bus_id, i2c_pin_def) in pin_config {
        for (other_bus_id, other_i2c_pin_def) in pin_config {
            if bus_id != other_bus_id && i2c_pin_def.0 == other_i2c_pin_def.0
                || i2c_pin_def.1 == other_i2c_pin_def.1
            {
                return false;
            }
        }
    }

    true
}

impl I2CBusController {
    pub fn new(gpio_borrow: &Rc<RefCell<GpioBorrowChecker>>, pin_config: HashMap<u8, I2CPinDefinition>) -> Result<Self, I2CError> {        
        for (bus_id, definition) in &pin_config {
            if definition.0 == definition.1 {
                return Err(I2CError::InvalidConfig(
                    format!("I2C bus is attempting to use the same pin twice: bus {} -> (SDA: {}. SCL: {})",
                    bus_id, definition.0, definition.1
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
            buses: HashMap::new()
        })
    }
}