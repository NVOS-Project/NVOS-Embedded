use std::any::Any;

pub trait BusController: Any {
    fn name(&self) -> String;
    fn as_any_ref(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Bus implementations
pub mod raw; // RawBusController
pub mod i2c; // I2CBusController
pub mod pwm; // PWMBusController
pub mod uart; // UARTBusController