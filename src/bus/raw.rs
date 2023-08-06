use crate::bus::BusController;
use crate::config::BusControllerConfig;
use crate::gpio::{GpioBorrowChecker, GpioError};
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use uuid::Uuid;
use rppal::gpio::{Gpio, Pin, InputPin, OutputPin, IoPin, Error, Mode};

fn rppal_map_err(err: Error, default_err_msg: &str) -> GpioError {
    match err {
        Error::PinNotAvailable(p) => GpioError::PinNotFound(p),
        Error::PinUsed(p) => GpioError::Busy(p),
        Error::PermissionDenied(s) => GpioError::PermissionDenied(s),
        _ => GpioError::Other(String::from(default_err_msg))
    }
}

pub enum InputMode {
    Normal,
    PullUp,
    PullDown
}

pub enum OutputMode {
    Normal,
    LogicHigh,
    LogicLow
}

pub struct RawBusController {
    gpio_controller: Gpio,
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    owned_pins: HashMap<u8, Uuid>
}

impl BusController for RawBusController {
    fn name(&self) -> String {
        "RAW".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl RawBusController {
    pub fn new(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>) -> Result<Self, GpioError> {
        let gpio = Gpio::new()
        .map_err(|err| rppal_map_err(err, "Internal RPPAL error while initializing Gpio interface"))?;
        
        Ok(RawBusController {
            gpio_controller: gpio,
            gpio_borrow: gpio_borrow.clone(), 
            owned_pins: HashMap::new()
        })
    }

    pub fn from_config(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>, config: &BusControllerConfig) -> Result<Self, GpioError> {
        Self::new(gpio_borrow)
    }

    pub fn open_in(&mut self, pin: u8, mode: InputMode) -> Result<InputPin, GpioError> {
        if self.owned_pins.contains_key(&pin) {
            return Err(GpioError::Busy(pin));
        }

        let pin = self.borrow_pin(pin)?;
        Ok(match mode {
            InputMode::Normal => pin.into_input(),
            InputMode::PullUp => pin.into_input_pullup(),
            InputMode::PullDown => pin.into_input_pulldown(),
        })
    }

    pub fn open_out(&mut self, pin: u8, mode: OutputMode) -> Result<OutputPin, GpioError> {
        if self.owned_pins.contains_key(&pin) {
            return Err(GpioError::Busy(pin));
        }

        let pin = self.borrow_pin(pin)?;
        Ok(match mode {
            OutputMode::Normal => pin.into_output(),
            OutputMode::LogicHigh => pin.into_output_high(),
            OutputMode::LogicLow => pin.into_output_low(),
        })
    }

    pub fn open_io(&mut self, pin: u8, mode: Mode) -> Result<IoPin, GpioError> {
        if self.owned_pins.contains_key(&pin) {
            return Err(GpioError::Busy(pin));
        }

        let pin = self.borrow_pin(pin)?;
        Ok(pin.into_io(mode))
    }

    pub fn close(&mut self, pin: u8) -> Result<(), GpioError> {
        let id = match self.owned_pins.get(&pin) {
            Some(i) => i,
            None => return Err(GpioError::LeaseNotFound)
        };

        self.gpio_borrow.write().release(id)?;
        self.owned_pins.remove(&pin);
        Ok(())
    }
    
    fn borrow_pin(&mut self, pin_id: u8) -> Result<Pin, GpioError> {
        let mut borrow_checker = self.gpio_borrow.write();
        let bcm_id = borrow_checker.get(&pin_id)?.bcm_id();

        if !borrow_checker.can_borrow_one(pin_id) {
            return Err(GpioError::Busy(pin_id));
        }

        let pin = self.gpio_controller.get(bcm_id)
            .map_err(|err| rppal_map_err(err, &format!("Internal RPPAL error while opening pin (BCM {})", bcm_id)))?;

        let borrow_id = borrow_checker.borrow_one(pin_id)?;
        self.owned_pins.insert(pin_id, borrow_id);
        Ok(pin)
    }
}