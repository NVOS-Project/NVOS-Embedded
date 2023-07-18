use std::any::Any;

pub trait BusController: Any {
    fn name(&self) -> String;
    fn as_any(&self) -> &dyn Any;
}

// Bus implementations
pub mod raw; // RawBusController
pub mod i2c; // I2CBusController
pub mod pwm; // PWMBusController;