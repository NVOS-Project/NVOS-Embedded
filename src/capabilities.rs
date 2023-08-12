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
    fn GetMode(&self) -> Result<LEDMode, DeviceError>;
    fn SetMode(&mut self, mode: LEDMode) -> Result<(), DeviceError>;
    fn GetBrightness(&self) -> Result<f32, DeviceError>;
    fn SetBrightness(&mut self, brightness: f32) -> Result<(), DeviceError>;
    fn GetPowerState(&self) -> Result<(), DeviceError>;
    fn SetPowerState(&mut self, powered_on: bool) -> Result<(), DeviceError>;
}

pub trait GpsCapable : Capability {
    fn GetLocation(&self) -> Result<(f64, f64), DeviceError>;
    fn GetAltitude(&self) -> Result<f32, DeviceError>;
    fn HasFix(&self) -> Result<bool, DeviceError>;
    fn GetSpeed(&self) -> Result<f32, DeviceError>;
    fn GetHeading(&self) -> Result<f32, DeviceError>;
    fn GetSatellites(&self) -> Result<Vec<Satellite>, DeviceError>;
    fn GetNmea(&self) -> Result<&Nmea, DeviceError>;
}