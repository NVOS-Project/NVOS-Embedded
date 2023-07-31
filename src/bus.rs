use std::any::Any;
pub trait BusController: Any + Send + Sync {
    fn name(&self) -> String;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

// Bus implementations
pub mod raw; // RawBusController
pub mod i2c; // I2CBusController
pub mod pwm; // PWMBusController
pub mod uart; // UARTBusController