use intertrait::CastFromSync;
use intertrait::cast::CastRef;
use nmea::{Satellite, Nmea};
use strum::{EnumIter, IntoEnumIterator};

use crate::device::DeviceError;

pub trait Capability : CastFromSync {
    fn get_capabilities(&self) -> Vec<CapabilityId> {
        let mut capabilities = Vec::<CapabilityId>::new();
        for capability in CapabilityId::iter() {
            let has_capability = match capability {
                CapabilityId::LEDController => self.cast::<dyn LEDControllerCapable>().is_some(),
                CapabilityId::GPS => self.cast::<dyn GpsCapable>().is_some()
            };

            if has_capability {
                capabilities.push(capability);
            }
        }

        capabilities
    }
}

#[derive(Debug, EnumIter, Clone)]
pub enum CapabilityId {
    LEDController,
    GPS
}

// Any capability APIs will go here
pub enum LEDMode {
    Visible,
    Infrared
}

pub trait LEDControllerCapable : Capability {
    fn get_mode(&self) -> Result<LEDMode, DeviceError>;
    fn set_mode(&mut self, mode: LEDMode) -> Result<(), DeviceError>;
    fn get_brightness(&self) -> Result<f32, DeviceError>;
    fn set_brightness(&mut self, brightness: f32) -> Result<(), DeviceError>;
    fn get_power_state(&self) -> Result<bool, DeviceError>;
    fn set_power_state(&mut self, powered_on: bool) -> Result<(), DeviceError>;
}

pub trait GpsCapable : Capability {
    fn get_location(&self) -> Result<(f64, f64), DeviceError>;
    fn get_altitude(&self) -> Result<f32, DeviceError>;
    fn has_fix(&self) -> Result<bool, DeviceError>;
    fn get_speed(&self) -> Result<f32, DeviceError>;
    fn get_heading(&self) -> Result<f32, DeviceError>;
    fn get_satellites(&self) -> Result<Vec<Satellite>, DeviceError>;
    fn get_nmea(&self) -> Result<&Nmea, DeviceError>;
}