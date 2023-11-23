use std::collections::HashMap;

use intertrait::cast::CastRef;
use nmea::{Satellite, Nmea};
use serde::{Serialize, Deserialize};
use strum::{EnumIter, IntoEnumIterator};

use crate::device::{DeviceError, DeviceDriver};

pub fn get_device_capabilities<T: DeviceDriver + ?Sized>(device: &T) -> Vec<CapabilityId> {
    let mut capabilities = Vec::<CapabilityId>::new();
    for capability in CapabilityId::iter() {
        let has_capability = match capability {
            CapabilityId::LEDController => device.cast::<dyn LEDControllerCapable>().is_some(),
            CapabilityId::GPS => device.cast::<dyn GpsCapable>().is_some(),
            CapabilityId::LightSensor => device.cast::<dyn LightSensorCapable>().is_some()
        };

        if has_capability {
            capabilities.push(capability);
        }
    }

    capabilities
}

pub trait Capability {}

#[derive(Debug, EnumIter, Clone, Copy, PartialEq)]
pub enum CapabilityId {
    LEDController,
    GPS,
    LightSensor
}

// Any capability APIs will go here
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
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
    fn get_nmea(&self) -> Result<Nmea, DeviceError>;
    fn get_vertical_accuracy(&self) -> Result<f32, DeviceError>;
    fn get_horizontal_accuracy(&self) -> Result<f32, DeviceError>;
}

pub trait LightSensorCapable : Capability {
    fn get_supported_gains(&self) -> HashMap<u8, u16>;
    fn get_supported_intervals(&self) -> HashMap<u8, u16>;
    fn get_supported_channels(&self) -> HashMap<u8, String>;
    fn get_auto_gain_enabled(&self) -> Result<bool, DeviceError>;
    fn set_auto_gain_enabled(&mut self, enabled: bool) -> Result<(), DeviceError>;
    fn get_gain(&self) -> Result<u16, DeviceError>;
    fn set_gain(&mut self, gain_id: u8) -> Result<(), DeviceError>;
    fn get_interval(&self) -> Result<u16, DeviceError>;
    fn set_interval(&mut self, interval_id: u8) -> Result<(), DeviceError>;
    fn get_luminosity(&mut self, channel_id: u8) -> Result<u32, DeviceError>;
    fn calculate_lux(&mut self) -> Result<f32, DeviceError>;
}