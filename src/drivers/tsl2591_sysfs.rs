use i2c_linux::I2c;
use intertrait::cast_to;
use log::{debug, error, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs::File,
    io::{Error, Read, Write},
    os::fd::AsRawFd,
    sync::Arc,
};

use crate::{
    bus::i2c_sysfs,
    bus::i2c_sysfs::SysfsI2CBusController,
    capabilities::{Capability, LightSensorCapable},
    config::ConfigError,
    device::{DeviceDriver, DeviceError, DeviceServer},
};
type I2cBus = Arc<Mutex<I2c<File>>>;

const LUX_DF: f32 = 735.0;
const DEFAULT_I2C_ADDR: u8 = 0x29;
const CHIP_ID: u8 = 0x50;

const COMMAND_BIT: u8 = 0xA0;
const REGISTER_ENABLE: u8 = 0x00;
const REGISTER_CONTROL: u8 = 0x01;
const REGISTER_ID_ADDR: u8 = 0x12;
const REGISTER_STATUS: u8 = 0x13;
const REGISTER_CHAN0_LSB: u8 = 0x14;
const REGISTER_CHAN1_LSB: u8 = 0x16;

const ENABLE_POWEROFF: u8 = 0x00;
const ENABLE_POWERON: u8 = 0x01;
const ENABLE_AEN: u8 = 0x02;

const SUPPORTED_CHANNELS: [&str; 3] = ["Visible+Infrared", "Infrared", "Visible"];

#[derive(Copy, Clone, PartialEq, Debug)]
enum IntegrationTime {
    _100MS = 0x00,
    _200MS = 0x01,
    _300MS = 0x02,
    _400MS = 0x03,
    _500MS = 0x04,
    _600MS = 0x05,
}

impl IntegrationTime {
    const fn into_millis(self) -> u16 {
        match self {
            IntegrationTime::_100MS => 100,
            IntegrationTime::_200MS => 200,
            IntegrationTime::_300MS => 300,
            IntegrationTime::_400MS => 400,
            IntegrationTime::_500MS => 500,
            IntegrationTime::_600MS => 600,
        }
    }

    const fn from_millis(value: u16) -> Option<Self> {
        Some(match value {
            100 => IntegrationTime::_100MS,
            200 => IntegrationTime::_200MS,
            300 => IntegrationTime::_300MS,
            400 => IntegrationTime::_400MS,
            500 => IntegrationTime::_500MS,
            600 => IntegrationTime::_600MS,
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum GainValue {
    _1X = 0x00,
    _25X = 0x10,
    _428X = 0x20,
    _9876X = 0x30,
}

impl GainValue {
    const fn into_multiplier(self) -> u16 {
        match self {
            GainValue::_1X => 1,
            GainValue::_25X => 25,
            GainValue::_428X => 428,
            GainValue::_9876X => 9876,
        }
    }

    const fn from_multiplier(value: u16) -> Option<Self> {
        Some(match value {
            1 => GainValue::_1X,
            25 => GainValue::_25X,
            428 => GainValue::_428X,
            9876 => GainValue::_9876X,
            _ => return None,
        })
    }
}

#[derive(PartialEq, Copy, Clone)]
enum ChannelId {
    FullSpectrum = 0,
    Infrared = 1,
    Visible = 2,
}

impl ChannelId {
    fn from(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::FullSpectrum),
            1 => Some(Self::Infrared),
            2 => Some(Self::Visible),
            _ => None,
        }
    }
}

const SUPPORTED_INTEGRATION_TIMES: [u16; 6] = [
    IntegrationTime::_100MS.into_millis(),
    IntegrationTime::_200MS.into_millis(),
    IntegrationTime::_300MS.into_millis(),
    IntegrationTime::_400MS.into_millis(),
    IntegrationTime::_500MS.into_millis(),
    IntegrationTime::_600MS.into_millis(),
];

const SUPPORTED_GAIN_VALUES: [u16; 4] = [
    GainValue::_1X.into_multiplier(),
    GainValue::_25X.into_multiplier(),
    GainValue::_428X.into_multiplier(),
    GainValue::_9876X.into_multiplier(),
];

#[derive(Serialize, Deserialize, Debug)]
pub struct Tsl2591SysfsConfig {
    pub auto_gain_enabled: bool,
    pub default_gain: u16,
    pub default_integration_time: u16,
    pub device_address: u8,
    pub bus_id: u8,
}

impl Default for Tsl2591SysfsConfig {
    fn default() -> Self {
        Tsl2591SysfsConfig {
            auto_gain_enabled: true,
            default_gain: GainValue::_1X.into_multiplier(),
            default_integration_time: IntegrationTime::_100MS.into_millis(),
            device_address: DEFAULT_I2C_ADDR,
            bus_id: 0,
        }
    }
}

// helper methods for managing the device
fn set_timing_and_gain<T: Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    timing: IntegrationTime,
    gain: GainValue,
) -> Result<(), Error> {
    i2c_sysfs::write_register(
        bus,
        address,
        COMMAND_BIT | REGISTER_CONTROL,
        timing as u8 | gain as u8,
    )?;
    Ok(())
}

fn enable<T: Write + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<(), Error> {
    i2c_sysfs::write_register(
        bus,
        address,
        COMMAND_BIT | REGISTER_ENABLE,
        ENABLE_POWERON | ENABLE_AEN,
    )
}

fn disable<T: Write + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<(), Error> {
    i2c_sysfs::write_register(bus, address, COMMAND_BIT | REGISTER_ENABLE, ENABLE_POWEROFF)
}

fn is_adc_valid<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<bool, Error> {
    let mut status_buf = [0u8; 1];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_STATUS, &mut status_buf)?;

    return Ok((status_buf[0] & 0x01) != 0);
}

fn get_chip_id<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<u8, Error> {
    let mut buf = [0u8; 1];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_ID_ADDR, &mut buf)?;

    Ok(buf[0])
}

fn read_adc<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<(u16, u16), Error> {
    let mut c0_buf = [0u8; 2];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_CHAN0_LSB, &mut c0_buf)?;

    let c0 = (c0_buf[1] as u16) << 8 | c0_buf[0] as u16;

    let mut c1_buf = [0u8; 2];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_CHAN1_LSB, &mut c1_buf)?;

    let c1 = (c1_buf[1] as u16) << 8 | c1_buf[0] as u16;
    Ok((c0, c1))
}

pub struct Tsl2591SysfsDriver {
    auto_gain_enabled: bool,
    config: Tsl2591SysfsConfig,
    bus: Option<I2cBus>,
    gain: GainValue,
    integration_time: IntegrationTime,
    is_loaded: bool,
}

impl Tsl2591SysfsDriver {
    fn from_config(config: Tsl2591SysfsConfig) -> Result<Self, DeviceError> {
        let gain = match GainValue::from_multiplier(config.default_gain) {
            Some(g) => g,
            None => {
                return Err(DeviceError::InvalidConfig(
                    ConfigError::InvalidEntry(format!(
                        "invalid gain multiplier: {}, supported gain values are {}",
                        config.default_gain,
                        SUPPORTED_GAIN_VALUES.map(|x| x.to_string()).join(", ")
                    ))
                    .to_string(),
                ))
            }
        };

        let integration_time = match IntegrationTime::from_millis(config.default_integration_time) {
            Some(t) => t,
            None => {
                return Err(DeviceError::InvalidConfig(
                    ConfigError::InvalidEntry(format!(
                        "invalid integration time: {}, supported values are {}",
                        config.default_integration_time,
                        SUPPORTED_INTEGRATION_TIMES
                            .map(|x| x.to_string())
                            .join(", ")
                    ))
                    .to_string(),
                ))
            }
        };

        Ok(Self {
            auto_gain_enabled: config.auto_gain_enabled,
            config: config,
            bus: None,
            gain: gain,
            integration_time: integration_time,
            is_loaded: false,
        })
    }

    fn assert_state(&self, check_bus: bool) -> Result<(), DeviceError> {
        if self.is_loaded && (!check_bus || self.bus.is_some()) {
            Ok(())
        } else {
            Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ))
        }
    }

    fn get_sensor_data(&mut self) -> Result<(u16, u16), DeviceError> {
        self.assert_state(true)?;
        let mut transaction = self.bus.as_ref().unwrap().lock();

        let (c0, c1) = read_adc(&mut transaction, self.config.device_address).map_err(|e| {
            DeviceError::HardwareError(format!("failed to read sensor data: {}", e))
        })?;

        if self.auto_gain_enabled {
            drop(transaction);
            self.auto_gain_update(c0);
        }

        Ok((c0, c1))
    }

    // Works the same way as the esphome tsl2591 compensation algorithm does.
    // Tries to keep the sensor saturation within the (1/3; 2/3) range.
    fn auto_gain_update(&mut self, c0: u16) {
        let divider = if self.integration_time == IntegrationTime::_100MS {
            2
        } else {
            1
        };
        let current_gain = self.gain;
        let mut new_gain = current_gain;

        match current_gain {
            GainValue::_1X => {
                if c0 < (u16::MAX / 3) / GainValue::_428X.into_multiplier() {
                    // Very low, go up to high
                    new_gain = GainValue::_428X;
                } else if c0 < (u16::MAX / 3) / GainValue::_25X.into_multiplier() {
                    // Kinda low, go up to med
                    new_gain = GainValue::_25X;
                }
            }
            GainValue::_25X => {
                if c0
                    < (u16::MAX / 3)
                        / (GainValue::_9876X.into_multiplier() / GainValue::_25X.into_multiplier())
                {
                    // Very low, go up to max
                    new_gain = GainValue::_9876X;
                } else if c0
                    < (u16::MAX / 3)
                        / (GainValue::_428X.into_multiplier() / GainValue::_25X.into_multiplier())
                {
                    // Kinda low, go up to high
                    new_gain = GainValue::_428X;
                } else if c0 > ((u16::MAX as f32) * 0.945) as u16 / divider {
                    // Too high, go down to low
                    new_gain = GainValue::_1X;
                }
            }
            GainValue::_428X => {
                if c0
                    < (u16::MAX / 3)
                        / (GainValue::_9876X.into_multiplier() / GainValue::_428X.into_multiplier())
                {
                    // Kinda low, go up to max
                    new_gain = GainValue::_9876X;
                } else if c0 > ((u16::MAX as f32) * 0.945) as u16 / divider {
                    // Too high, go down to mid
                    new_gain = GainValue::_25X;
                }
            }
            GainValue::_9876X => {
                if c0 > ((u16::MAX as f32) * 0.945) as u16 / divider {
                    // Too high, go down to high
                    new_gain = GainValue::_428X;
                }
            }
        };

        if current_gain != new_gain {
            debug!(
                "Auto gain updating from {:?} to {:?}",
                current_gain, new_gain
            );
            let mut transaction = self.bus.as_ref().unwrap().lock();
            match set_timing_and_gain(
                &mut transaction,
                self.config.device_address,
                self.integration_time,
                new_gain,
            ) {
                Ok(_) => {
                    self.gain = new_gain;
                }
                Err(e) => {
                    warn!("Failed to auto update gain: {}", e);
                }
            }
        }
    }
}

impl DeviceDriver for Tsl2591SysfsDriver {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn name(&self) -> String {
        "tsl2591_sysfs".to_string()
    }

    fn is_running(&self) -> bool {
        self.is_loaded
    }

    fn new(
        config: Option<&mut crate::config::DeviceConfig>,
    ) -> Result<Self, crate::device::DeviceError>
    where
        Self: Sized,
    {
        if config.is_none() {
            return Err(DeviceError::InvalidConfig(
                "this driver requires a configuration object but none was provided".to_owned(),
            ));
        }

        let config = config.unwrap();
        let data: Tsl2591SysfsConfig = match serde_json::from_value(config.driver_data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.driver_data == Value::Null {
                    match serde_json::to_value(Tsl2591SysfsConfig::default()) {
                        Ok(c) => {
                            config.driver_data = c;
                            return Err(DeviceError::InvalidConfig(
                                ConfigError::MissingEntry(
                                    "device was missing config data, default config was written"
                                        .to_string(),
                                )
                                .to_string(),
                            ));
                        }
                        Err(e) => {
                            warn!("Failed to write default configuration: {}", e);
                            return Err(DeviceError::InvalidConfig(
                                ConfigError::MissingEntry(
                                    format!("device was missing config data, default config failed to be written: {}", e)
                                ).to_string()
                            ));
                        }
                    }
                }

                return Err(DeviceError::InvalidConfig(
                    ConfigError::SerializeError(format!(
                        "failed to deserialize device config data: {}",
                        e
                    ))
                    .to_string(),
                ));
            }
        };

        Self::from_config(data)
    }

    fn start(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError> {
        if self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device load requested but this device is already loaded".to_string(),
            ));
        }

        let address = self.config.device_address;
        let bus_id = self.config.bus_id;

        let mut i2c = match parent.get_bus_mut::<SysfsI2CBusController>() {
            Some(controller) => controller,
            None => return Err(DeviceError::MissingController("i2c_sysfs".to_string())),
        };

        let bus = match i2c.get(bus_id) {
            Ok(bus) => bus,
            Err(e) => return Err(DeviceError::HardwareError(e.to_string())),
        };

        let mut transaction = bus.lock();
        let chip_id = match get_chip_id(&mut transaction, address) {
            Ok(id) => id,
            Err(e) => {
                return Err(DeviceError::HardwareError(format!(
                    "failed to identify chip: {}",
                    e
                )))
            }
        };

        if chip_id != CHIP_ID {
            return Err(DeviceError::HardwareError(format!(
                "bus {} address {} contains an invalid device - reported chipID {} but expected {}",
                bus_id, address, chip_id, CHIP_ID
            )));
        }

        if let Err(e) = enable(&mut transaction, address) {
            return Err(DeviceError::HardwareError(format!(
                "failed to enable device: {}",
                e
            )));
        }

        if let Err(e) = set_timing_and_gain(
            &mut transaction,
            self.config.device_address,
            self.integration_time,
            self.gain,
        ) {
            warn!("Failed to set initial timing and gain: {}", e);
        }

        drop(transaction);
        self.bus = Some(bus);
        self.is_loaded = true;
        Ok(())
    }

    fn stop(&mut self, _parent: &mut DeviceServer) -> Result<(), DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device unload requested but this device isn't loaded".to_string(),
            ));
        }

        match self.bus {
            Some(ref bus) => {
                let address = self.config.device_address;
                let mut transaction = bus.lock();

                if let Err(e) = disable(&mut transaction, address) {
                    warn!("Failed to disable device: {}", e);
                }
            }
            None => warn!("Failed to disable hardware: I2C bus was uninitialized"),
        };

        self.bus = None;
        self.is_loaded = false;
        Ok(())
    }
}

impl Capability for Tsl2591SysfsDriver {}

#[cast_to]
impl LightSensorCapable for Tsl2591SysfsDriver {
    fn get_supported_gains(&self) -> HashMap<u8, u16> {
        SUPPORTED_GAIN_VALUES
            .iter()
            .enumerate()
            .map(|(index, &value)| (index as u8, value))
            .collect()
    }

    fn get_supported_intervals(&self) -> HashMap<u8, u16> {
        SUPPORTED_INTEGRATION_TIMES
            .iter()
            .enumerate()
            .map(|(index, &value)| (index as u8, value))
            .collect()
    }

    fn get_supported_channels(&self) -> HashMap<u8, String> {
        SUPPORTED_CHANNELS
            .iter()
            .enumerate()
            .map(|(index, &value)| (index as u8, value.to_owned()))
            .collect()
    }

    fn get_auto_gain_enabled(&self) -> Result<bool, DeviceError> {
        self.assert_state(false)?;
        Ok(self.auto_gain_enabled)
    }

    fn set_auto_gain_enabled(&mut self, _enabled: bool) -> Result<(), DeviceError> {
        self.assert_state(false)?;
        self.auto_gain_enabled = _enabled;
        Ok(())
    }

    fn get_gain(&self) -> Result<u16, DeviceError> {
        self.assert_state(false)?;
        Ok(self.gain.into_multiplier())
    }

    fn set_gain(&mut self, gain_id: u8) -> Result<(), DeviceError> {
        self.assert_state(true)?;
        let gain_multiplier = match SUPPORTED_GAIN_VALUES.get(gain_id as usize) {
            Some(gain) => gain,
            None => {
                return Err(DeviceError::InvalidOperation(format!(
                    "gain value ID is not supported: {}",
                    gain_id
                )))
            }
        };

        let gain_value = match GainValue::from_multiplier(*gain_multiplier) {
            Some(gain) => gain,
            None => {
                error!("Failed to convert a gain multiplier of {}x to a GainValue because it is unsupported, but it is being offered in the list of supported gain values", gain_multiplier);
                return Err(DeviceError::Internal);
            }
        };

        let mut transaction = self.bus.as_ref().unwrap().lock();
        set_timing_and_gain(
            &mut transaction,
            self.config.device_address,
            self.integration_time,
            gain_value,
        )
        .map_err(|e| {
            DeviceError::HardwareError(format!("failed to apply new gain value: {}", e))
        })?;

        self.gain = gain_value;
        Ok(())
    }

    fn get_interval(&self) -> Result<u16, DeviceError> {
        self.assert_state(false)?;
        Ok(self.integration_time.into_millis())
    }

    fn set_interval(&mut self, interval_id: u8) -> Result<(), DeviceError> {
        self.assert_state(true)?;
        let interval_millis = match SUPPORTED_INTEGRATION_TIMES.get(interval_id as usize) {
            Some(time) => time,
            None => {
                return Err(DeviceError::InvalidOperation(format!(
                    "integration time ID is not supported: {}",
                    interval_id
                )))
            }
        };

        let integration_time = match IntegrationTime::from_millis(*interval_millis) {
            Some(time) => time,
            None => {
                error!("Failed to convert a time interval of {}ms to an IntegrationTime because it is unsupported, but it is being offered in the list of supported integration times", interval_millis);
                return Err(DeviceError::Internal);
            }
        };

        let mut transaction = self.bus.as_ref().unwrap().lock();
        set_timing_and_gain(
            &mut transaction,
            self.config.device_address,
            integration_time,
            self.gain,
        )
        .map_err(|e| {
            DeviceError::HardwareError(format!("failed to apply new integration time: {}", e))
        })?;

        self.integration_time = integration_time;
        Ok(())
    }

    fn get_luminosity(&mut self, channel_id: u8) -> Result<u32, DeviceError> {
        self.assert_state(false)?;

        let channel = match ChannelId::from(channel_id) {
            Some(c) => c,
            None => {
                return Err(DeviceError::InvalidOperation(format!(
                    "channel ID is not supported: {}",
                    channel_id
                )))
            }
        };

        let (c0, c1) = self.get_sensor_data()?;

        match channel {
            ChannelId::FullSpectrum => Ok(c0.into()),
            ChannelId::Infrared => Ok(c1.into()),
            ChannelId::Visible => {
                if c1 > c0 {
                    Err(DeviceError::Other("infrared overflow".to_string()))
                } else {
                    Ok((c0 - c1).into())
                }
            }
        }
    }

    fn get_illuminance(&mut self) -> Result<f32, DeviceError> {
        self.assert_state(false)?;
        let integration_time = self.integration_time.into_millis() as f32;
        let gain_value = self.gain.into_multiplier() as f32;

        let (mut c0, c1) = self.get_sensor_data()?;
        let overflow_value = if self.integration_time == IntegrationTime::_100MS {
            36863
        } else {
            65535
        };

        if c0 == overflow_value || c1 == overflow_value {
            return Err(DeviceError::Other("sensor reading overflow".to_string()));
        }

        // bug fix for thing
        if c0 == 0x0000 {
            c0 = 1;
        }

        let cpl = (integration_time * gain_value) / LUX_DF;
        let lux = ((c0 as f32 - c1 as f32) * (1.0 - (c1 as f32 / c0 as f32))) / cpl;

        Ok(lux)
    }
}
