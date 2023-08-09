use sysfs_gpio::{Pin, Direction, Error};
use std::{sync::Arc, collections::HashMap, any::Any, path::Path};
use parking_lot::RwLock;
use uuid::Uuid;
use crate::{gpio::{GpioBorrowChecker, GpioError}, config::BusControllerConfig};
use super::BusController;

const SYSFS_GPIO_PATH: &str = "/sys/class/gpio";

fn sysfs_map_err(err: Error, default_err_msg: &str) -> GpioError {
    match err {
        Error::Io(msg) => GpioError::OsError(msg.to_string()),
        Error::Unexpected(msg) => GpioError::OsError(msg),
        Error::InvalidPath(msg) => GpioError::Unsupported(msg),
        Error::Unsupported(msg) => GpioError::Unsupported(msg),
        _ => GpioError::Other(format!("{}: {}", default_err_msg.to_string(), err))
    }
}

pub struct SysfsRawBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    owned_pins: HashMap<u8, Uuid>
}

impl BusController for SysfsRawBusController {
    fn name(&self) -> String {
        "raw_sysfs".to_string()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl SysfsRawBusController {
    pub fn new(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>) -> Result<Self, GpioError> {
        let path = Path::new(SYSFS_GPIO_PATH);
        if !path.exists() || !path.is_dir() {
            return Err(GpioError::OsError("GPIO is not supported on this system".to_string()));
        }

        Ok(SysfsRawBusController { 
            gpio_borrow: gpio_borrow.clone(), 
            owned_pins: HashMap::new()
        })
    }

    pub fn from_config(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>, config: &BusControllerConfig) -> Result<Self, GpioError> {
        Self::new(gpio_borrow)
    }

    pub fn open_in(&mut self, pin: u8) -> Result<Pin, GpioError>{
        if self.owned_pins.contains_key(&pin) {
            return Err(GpioError::Busy(pin));
        }

        let pin = self.borrow_pin(pin, Direction::In)?;
        Ok(pin)
    }

    pub fn open_out(&mut self, pin: u8) -> Result<Pin, GpioError> {
        if self.owned_pins.contains_key(&pin) {
            return Err(GpioError::Busy(pin));
        }

        let pin = self.borrow_pin(pin, Direction::Out)?;
        Ok(pin)
    }

    pub fn close(&mut self, pin: Pin) -> Result<(), GpioError> {
        let mut borrow_checker = self.gpio_borrow.write();
        let bcm_id = pin.get_pin() as u8;
        let pin_id = match borrow_checker.get_borrowed()
            .iter().filter_map(|state| match state.bcm_id() == bcm_id {
                true => Some(state.pin_id()),
                false => None
            }).nth(0)
        {
            Some(id) => id,
            None => return Err(GpioError::LeaseNotFound) // this bus controller doesn't own this pin,
        };

        let id = match self.owned_pins.get(&pin_id) {
            Some(i) => i,
            None => return Err(GpioError::LeaseNotFound)
        };

        if pin.is_exported() {
            // reset pin state
            pin.set_value(0)
            .and(pin.set_direction(Direction::In))
            .and(pin.unexport()).map_err(|err| sysfs_map_err(err, &format!("Internal sysfs error while closing pin (ID {})", pin_id)))?;
        }

        borrow_checker.release(id)?;
        Ok(())
    }

    fn borrow_pin(&mut self, pin_id: u8, direction: Direction) -> Result<Pin, GpioError> {
        let mut borrow_checker = self.gpio_borrow.write();
        let bcm_id = borrow_checker.get(&pin_id)?.bcm_id();

        if !borrow_checker.can_borrow_one(pin_id) {
            return Err(GpioError::Busy(pin_id));
        }

        let pin = Pin::new(bcm_id.into());
        pin.export().and(pin.set_direction(direction)).map_err(|err| sysfs_map_err(err, &format!("Internal sysfs error while opening pin (ID {})", pin_id)))?;

        match borrow_checker.borrow_one(pin_id) {
            Ok(borrow_id) => {
                self.owned_pins.insert(pin_id, borrow_id);
                Ok(pin)
            },
            Err(err) => {
                // Unexport the GPIO
                let _ = pin.set_direction(Direction::In).and(pin.unexport());
                Err(err)
            },
        }
    }
}