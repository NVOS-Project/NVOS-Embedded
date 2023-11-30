use super::{
    i2c::{I2CError, I2CPinDefinition, I2cConfigData},
    BusController,
};
use crate::{
    config::{BusControllerConfig, ConfigError},
    gpio::GpioBorrowChecker,
};
use i2c_linux::I2c;
use log::warn;
use parking_lot::{Mutex, RwLock};
use serde_json::Value;
use std::{any::Any, collections::HashMap, fs::File, path::Path, sync::Arc, io::{Write, Error, Read}, os::fd::AsRawFd};
use uuid::Uuid;

const I2C_CLASS_PATH: &str = "/sys/class/i2c-dev";
const I2C_DEVICE_PATH: &str = "/dev";

// helper methods for interfacing with devices over I2C
pub fn write_command<T: Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    command: u8,
) -> Result<(), Error> {
    bus.smbus_set_slave_address(address as u16, false)?;
    bus.write(&[command])?;
    Ok(())
}

pub fn write_register<T: Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    register: u8,
    data: u8,
) -> Result<(), Error> {
    bus.smbus_set_slave_address(address as u16, false)?;
    bus.write(&[register, data])?;
    Ok(())
}

pub fn read_register<T: Read + Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    register: u8,
    buf: &mut [u8],
) -> Result<(), Error> {
    bus.smbus_set_slave_address(address as u16, false)?;
    bus.write(&[register])?;
    bus.read_exact(buf)?;
    Ok(())
}

fn sysfs_map_err(err: std::io::Error, default_err_msg: &str) -> I2CError {
    I2CError::HardwareError(format!("{}: {}", default_err_msg.to_string(), err))
}
struct I2cInfo {
    bus_id: u8,
    lease_id: Uuid,
    bus: Arc<Mutex<I2c<File>>>,
}

impl I2cInfo {
    fn new(bus_id: u8, lease_id: Uuid, bus: I2c<File>) -> Self {
        Self::with_rc(bus_id, lease_id, Arc::new(Mutex::new(bus)))
    }

    fn with_rc(bus_id: u8, lease_id: Uuid, bus: Arc<Mutex<I2c<File>>>) -> Self {
        I2cInfo {
            bus_id,
            lease_id,
            bus,
        }
    }
}

pub struct SysfsI2CBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    pin_config: HashMap<u8, I2CPinDefinition>,
    owned_buses: HashMap<u8, I2cInfo>,
}

impl BusController for SysfsI2CBusController {
    fn name(&self) -> String {
        "i2c_sysfs".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl SysfsI2CBusController {
    pub fn new(
        gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>,
        pin_config: HashMap<u8, I2CPinDefinition>,
    ) -> Result<Self, I2CError> {
        let path = Path::new(I2C_CLASS_PATH);
        if !path.exists() || !path.is_dir() {
            return Err(I2CError::OsError(
                "I2C is not supported on this system".to_string(),
            ));
        }

        let gpio_checker = gpio_borrow.read();

        for (bus_id, definition) in &pin_config {
            if definition.sda == definition.scl {
                return Err(I2CError::InvalidConfig(format!(
                    "I2C bus is attempting to use the same pin twice: bus {} -> (SDA: {}. SCL: {})",
                    bus_id, definition.sda, definition.scl
                )));
            }

            if !gpio_checker.has_pin(definition.sda) {
                return Err(I2CError::InvalidConfig(format!(
                    "I2C bus is attempting to use invalid pin: bus {} pin {} (SDA)",
                    bus_id, definition.sda
                )));
            }

            if !gpio_checker.has_pin(definition.scl) {
                return Err(I2CError::InvalidConfig(format!(
                    "I2C bus is attempting to use invalid pin: bus {} pin {} (SCL)",
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

        Ok(SysfsI2CBusController {
            gpio_borrow: gpio_borrow.clone(),
            pin_config: pin_config,
            owned_buses: HashMap::new(),
        })
    }

    pub fn from_config(
        gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>,
        config: &mut BusControllerConfig,
    ) -> Result<Self, I2CError> {
        let data: I2cConfigData = match serde_json::from_value(config.data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.data == Value::Null {
                    config.data = match serde_json::to_value(I2cConfigData::default()) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to write default configuration: {}", e);
                            Value::Null
                        }
                    };
                }

                return Err(I2CError::InvalidConfig(
                    ConfigError::SerializeError(format!("invalid I2C data struct json: {}", e))
                        .to_string(),
                ));
            }
        };

        Self::new(gpio_borrow, data.channels)
    }

    pub fn open(&mut self, bus_id: u8) -> Result<Arc<Mutex<I2c<File>>>, I2CError> {
        if self.owned_buses.contains_key(&bus_id) {
            return Err(I2CError::ChannelBusy(bus_id));
        }

        let definition = match self.pin_config.get(&bus_id) {
            Some(v) => v,
            None => return Err(I2CError::BusNotFound(bus_id)),
        };

        let mut borrow_checker = self.gpio_borrow.write();
        if !borrow_checker.can_borrow_many(&definition.to_arr()) {
            return Err(I2CError::HardwareError(
                "I2C channel pins are already in use".to_string(),
            ));
        }

        let bus = I2c::from_path(Path::new(I2C_DEVICE_PATH).join(format!("i2c-{}", bus_id)))
            .map_err(|err| sysfs_map_err(err, &format!("Internal sysfs error while opening I2C bus {}", bus_id)))?;

        let borrow_id = borrow_checker.borrow_many(definition.to_vec())
            .map_err(|err| I2CError::HardwareError(err.to_string()))?;

        let bus_info = I2cInfo::new(bus_id, borrow_id, bus);
        let result = bus_info.bus.clone();
        self.owned_buses.insert(bus_id, bus_info);
        Ok(result)
    }

    pub fn get(&mut self, bus_id: u8) -> Result<Arc<Mutex<I2c<File>>>, I2CError> {
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

        let rc = Arc::strong_count(&info.bus);
        if rc > 1 {
            warn!("Attempted to close I2C bus {} while still holding {} reference(s) to it", bus_id, rc - 1);
            return Err(I2CError::ChannelBusy(bus_id));
        }

        let mut borrow_checker = self.gpio_borrow.write();
        borrow_checker.release(&info.lease_id)
            .map_err(|err| I2CError::HardwareError(err.to_string()))?;

        self.owned_buses.remove(&bus_id);
        Ok(())
    }
}
