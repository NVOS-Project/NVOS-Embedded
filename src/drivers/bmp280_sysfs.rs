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
    thread,
    time::Duration,
};

use crate::{
    bus::i2c_sysfs::{self, SysfsI2CBusController},
    capabilities::{Capability, ThermometerCapable},
    config::ConfigError,
    device::{DeviceDriver, DeviceError},
};
type I2cBus = Arc<Mutex<I2c<File>>>;

const SPINWAIT_INTERVAL: u16 = 10;
const DEFAULT_I2C_ADDR: u8 = 0x76;
const CHIP_ID: u8 = 0x58;
const COMMAND_BIT: u8 = 0x80;

const REGISTER_CALIB0: u8 = 0x08;
const REGISTER_CALIB25: u8 = 0x20;
const CALIB_DATA_LEN: usize = REGISTER_CALIB25 as usize - REGISTER_CALIB0 as usize;
const REGISTER_ID: u8 = 0x50;
const REGISTER_RESET: u8 = 0x60;
const REGISTER_STATUS: u8 = 0x73;
const REGISTER_CONTROL: u8 = 0x74;
const REGISTER_CONFIG: u8 = 0x75;
const PRESSURE_MSB: u8 = 0x77;
const TEMPERATURE_MSB: u8 = 0x7A;

enum PowerMode {
    Sleep = 0x00,
    Once = 0x01,
    Normal = 0x03,
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum StandbyTime {
    _1MS = 0x00,
    _63MS = 0x01,
    _125MS = 0x02,
    _250MS = 0x03,
    _500MS = 0x04,
    _1000MS = 0x05,
    _2000MS = 0x06,
    _4000MS = 0x07,
}

impl StandbyTime {
    const fn into_millis(self) -> u16 {
        match self {
            StandbyTime::_1MS => 1,
            StandbyTime::_63MS => 63,
            StandbyTime::_125MS => 125,
            StandbyTime::_250MS => 250,
            StandbyTime::_500MS => 500,
            StandbyTime::_1000MS => 1000,
            StandbyTime::_2000MS => 2000,
            StandbyTime::_4000MS => 4000,
        }
    }

    const fn from_millis(value: u16) -> Option<Self> {
        Some(match value {
            1 => StandbyTime::_1MS,
            63 => StandbyTime::_63MS,
            125 => StandbyTime::_125MS,
            250 => StandbyTime::_250MS,
            500 => StandbyTime::_500MS,
            1000 => StandbyTime::_1000MS,
            2000 => StandbyTime::_2000MS,
            4000 => StandbyTime::_4000MS,
            _ => return None,
        })
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
enum GainValue {
    _1X = 0x01,
    _2X = 0x02,
    _4X = 0x03,
    _8X = 0x04,
    _16X = 0x05,
}

impl GainValue {
    const fn into_multiplier(self) -> u16 {
        match self {
            GainValue::_1X => 1,
            GainValue::_2X => 2,
            GainValue::_4X => 4,
            GainValue::_8X => 8,
            GainValue::_16X => 16,
        }
    }

    const fn from_multiplier(value: u16) -> Option<Self> {
        Some(match value {
            1 => GainValue::_1X,
            2 => GainValue::_2X,
            4 => GainValue::_4X,
            8 => GainValue::_8X,
            16 => GainValue::_16X,
            _ => return None,
        })
    }
}

#[allow(non_snake_case)]
struct CalibrationData {
    dig_T1: u16,
    dig_T2: i16,
    dig_T3: i16,
    dig_P1: u16,
    dig_P2: i16,
    dig_P3: i16,
    dig_P4: i16,
    dig_P5: i16,
    dig_P6: i16,
    dig_P7: i16,
    dig_P8: i16,
    dig_P9: i16,
}

const SUPPORTED_STANDBY_TIMES: [u16; 8] = [
    StandbyTime::_1MS.into_millis(),
    StandbyTime::_63MS.into_millis(),
    StandbyTime::_125MS.into_millis(),
    StandbyTime::_250MS.into_millis(),
    StandbyTime::_500MS.into_millis(),
    StandbyTime::_1000MS.into_millis(),
    StandbyTime::_2000MS.into_millis(),
    StandbyTime::_4000MS.into_millis(),
];

const SUPPORTED_GAIN_VALUES: [u16; 5] = [
    GainValue::_1X.into_multiplier(),
    GainValue::_2X.into_multiplier(),
    GainValue::_4X.into_multiplier(),
    GainValue::_8X.into_multiplier(),
    GainValue::_16X.into_multiplier(),
];

#[derive(Serialize, Deserialize, Debug)]
pub struct Bmp280SysfsConfig {
    pub default_thermometer_gain: u16,
    pub default_pressure_gain: u16,
    pub default_standby_time: u16,
    pub device_address: u8,
    pub device_ready_timeout: u16,
    pub bus_id: u8,
}

impl Default for Bmp280SysfsConfig {
    fn default() -> Self {
        Self {
            default_thermometer_gain: GainValue::_1X.into_multiplier(),
            default_pressure_gain: GainValue::_4X.into_multiplier(),
            default_standby_time: StandbyTime::_63MS.into_millis(),
            device_address: DEFAULT_I2C_ADDR,
            device_ready_timeout: 100,
            bus_id: 0,
        }
    }
}

// helper methods for managing the device
fn set_mode_and_gain<T: Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    thermometer_gain: GainValue,
    pressure_gain: GainValue,
    mode: PowerMode,
) -> Result<(), Error> {
    let data = ((thermometer_gain as u8) << 5) | ((pressure_gain as u8) << 3) | mode as u8;
    i2c_sysfs::write_register(bus, address, COMMAND_BIT | REGISTER_CONTROL, data)
}

fn get_chip_id<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<u8, Error> {
    let mut buf = [0u8; 1];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_ID, &mut buf)?;

    Ok(buf[0])
}

fn read_adc<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<(u32, u32), Error> {
    let mut temp_buf = [0u8; 3];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | TEMPERATURE_MSB, &mut temp_buf)?;

    let temp =
        ((temp_buf[0] as u32) << 12) | ((temp_buf[1] as u32) << 4) | (temp_buf[2] as u32 >> 4);

    let mut press_buf = [0u8; 3];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | TEMPERATURE_MSB, &mut press_buf)?;
    let press =
        ((press_buf[0] as u32) << 12) | ((press_buf[1] as u32) << 4) | (press_buf[2] as u32 >> 4);

    Ok((temp, press))
}

fn is_adc_valid<T: Write + Read + AsRawFd>(bus: &mut I2c<T>, address: u8) -> Result<bool, Error> {
    let mut status_buf = [0u8; 1];
    i2c_sysfs::read_register(bus, address, COMMAND_BIT | REGISTER_STATUS, &mut status_buf)?;

    return Ok(status_buf[0] & 0x09 == 0x00);
}

fn wait_adc_valid<T: Write + Read + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    step: u16,
    timeout: u16,
) -> Result<(), DeviceError> {
    let mut elapsed = 0;
    let wait_interval = Duration::from_millis(step as u64);
    loop {
        if elapsed >= timeout {
            return Err(DeviceError::HardwareError(format!(
                "timed out waiting for the chip to become ready"
            )));
        }

        match is_adc_valid(bus, address) {
            Ok(result) => {
                if result {
                    break;
                }
            }
            Err(e) => {
                return Err(DeviceError::HardwareError(format!(
                    "failed to read chip status: {}",
                    e
                )))
            }
        };

        elapsed += step;
        thread::sleep(wait_interval)
    }

    debug!("ADC ready after ~{} ms", elapsed);
    Ok(())
}

fn set_standby_time<T: Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
    time: StandbyTime,
) -> Result<(), Error> {
    let data = (time as u8) << 5;
    i2c_sysfs::write_register(bus, address, COMMAND_BIT | REGISTER_CONFIG, data)
}

fn read_calib_data<T: Read + Write + AsRawFd>(
    bus: &mut I2c<T>,
    address: u8,
) -> Result<CalibrationData, Error> {
    let mut calib_buf = [0u8; CALIB_DATA_LEN];
    i2c_sysfs::read_register(bus, address, REGISTER_CALIB0, &mut calib_buf)?;

    Ok(CalibrationData {
        dig_T1: (calib_buf[1] as u16) << 8 | calib_buf[0] as u16,
        dig_T2: (calib_buf[3] as i16) << 8 | calib_buf[2] as i16,
        dig_T3: (calib_buf[5] as i16) << 8 | calib_buf[4] as i16,
        dig_P1: (calib_buf[7] as u16) << 8 | calib_buf[6] as u16,
        dig_P2: (calib_buf[9] as i16) << 8 | calib_buf[8] as i16,
        dig_P3: (calib_buf[11] as i16) << 8 | calib_buf[10] as i16,
        dig_P4: (calib_buf[13] as i16) << 8 | calib_buf[12] as i16,
        dig_P5: (calib_buf[15] as i16) << 8 | calib_buf[14] as i16,
        dig_P6: (calib_buf[17] as i16) << 8 | calib_buf[16] as i16,
        dig_P7: (calib_buf[19] as i16) << 8 | calib_buf[18] as i16,
        dig_P8: (calib_buf[21] as i16) << 8 | calib_buf[20] as i16,
        dig_P9: (calib_buf[23] as i16) << 8 | calib_buf[22] as i16,
    })
}

fn compensate_values(temperature: i32, pressure: i32, calibration: &CalibrationData) -> (f32, f32) {
    let var1_t = (((temperature >> 3) - ((calibration.dig_T1 as i32) << 1))
        * (calibration.dig_T2 as i32))
        >> 11;
    let var2_t = (((((temperature >> 4) - (calibration.dig_T1 as i32))
        * ((temperature >> 4) - (calibration.dig_T1 as i32)))
        >> 12)
        * (calibration.dig_T3 as i32))
        >> 14;

    let t_fine = var1_t + var2_t;
    let temp = ((t_fine * 5 + 128) >> 8) as f32 / 100.0;

    let press;
    let mut var1_p: i64 = (t_fine as i64) - 128000;
    let mut var2_p: i64 = var1_p * var1_p * (calibration.dig_P6 as i64);
    var2_p = var2_p + ((var1_p * (calibration.dig_P5 as i64)) << 17);
    var2_p = var2_p + ((calibration.dig_P4 as i64) << 35);
    var1_p = ((var1_p * var1_p * (calibration.dig_P3 as i64)) >> 8)
        + ((var1_p * (calibration.dig_P2 as i64)) << 12);
    var1_p = (((1i64 << 47) + var1_p) * (calibration.dig_P1 as i64)) >> 33;

    if var1_p == 0 {
        press = -1.0;
    } else {
        let mut p: i64 = 1048576 - pressure as i64;
        p = (((p << 31) - var2_p) * 3125) / var1_p;
        var1_p = ((calibration.dig_P9 as i64) * (p >> 13) * (p >> 13)) >> 25;
        var2_p = ((calibration.dig_P8 as i64) * p) >> 19;

        p = ((p + var1_p + var2_p) >> 8) + ((calibration.dig_P7 as i64) << 4);

        press = (p as f32) / 256.0;
    }

    (temp, press)
}

pub struct Bmp280SysfsDriver {
    config: Bmp280SysfsConfig,
    bus: Option<I2cBus>,
    calibration_data: Option<CalibrationData>,
    thermometer_gain: GainValue,
    pressure_gain: GainValue,
    standby_time: StandbyTime,
    is_loaded: bool,
}

impl Bmp280SysfsDriver {
    fn from_config(config: Bmp280SysfsConfig) -> Result<Self, DeviceError> {
        let thermometer_gain = match GainValue::from_multiplier(config.default_thermometer_gain) {
            Some(g) => g,
            None => {
                return Err(DeviceError::InvalidConfig(
                    ConfigError::InvalidEntry(format!(
                        "invalid thermometer gain multiplier: {}, supported gain values are {}",
                        config.default_thermometer_gain,
                        SUPPORTED_GAIN_VALUES.map(|x| x.to_string()).join(", ")
                    ))
                    .to_string(),
                ))
            }
        };

        let pressure_gain = match GainValue::from_multiplier(config.default_pressure_gain) {
            Some(g) => g,
            None => {
                return Err(DeviceError::InvalidConfig(
                    ConfigError::InvalidEntry(format!(
                        "invalid thermometer gain multiplier: {}, supported gain values are {}",
                        config.default_pressure_gain,
                        SUPPORTED_GAIN_VALUES.map(|x| x.to_string()).join(", ")
                    ))
                    .to_string(),
                ))
            }
        };

        let standby_time = match StandbyTime::from_millis(config.default_standby_time) {
            Some(t) => t,
            None => {
                return Err(DeviceError::InvalidConfig(
                    ConfigError::InvalidEntry(format!(
                        "invalid standby time: {}, supported values are {}",
                        config.default_standby_time,
                        SUPPORTED_STANDBY_TIMES.map(|x| x.to_string()).join(", ")
                    ))
                    .to_string(),
                ))
            }
        };

        Ok(Self {
            config: config,
            bus: None,
            calibration_data: None,
            thermometer_gain: thermometer_gain,
            pressure_gain: pressure_gain,
            standby_time,
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
}

impl DeviceDriver for Bmp280SysfsDriver {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn name(&self) -> String {
        "bmp280-sysfs".to_string()
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
        let data: Bmp280SysfsConfig = match serde_json::from_value(config.driver_data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.driver_data == Value::Null {
                    match serde_json::to_value(Bmp280SysfsConfig::default()) {
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

    fn start(&mut self, parent: &mut crate::device::DeviceServer) -> Result<(), DeviceError> {
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

        wait_adc_valid(&mut transaction, address, SPINWAIT_INTERVAL, self.config.device_ready_timeout)?;

        let calibration = read_calib_data(&mut transaction, address)
            .map_err(|e| DeviceError::HardwareError(format!("failed to read calibration data from chip: {}", e)))?;

        if let Err(e) = set_mode_and_gain(
            &mut transaction,
            address,
            self.thermometer_gain,
            self.pressure_gain,
            PowerMode::Normal,
        ) {
            return Err(DeviceError::HardwareError(format!(
                "failed to enable and configure device: {}",
                e
            )));
        }

        if let Err(e) = set_standby_time(&mut transaction, address, self.standby_time) {
            warn!("Failed to set standby time: {}", e);
        }

        drop(transaction);
        self.bus = Some(bus);
        self.calibration_data = Some(calibration);
        self.is_loaded = true;
        Ok(())
    }

    fn stop(&mut self, _parent: &mut crate::device::DeviceServer) -> Result<(), DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device unload requested but this device isn't loaded".to_string(),
            ));
        }

        match self.bus {
            Some(ref bus) => {
                let address = self.config.device_address;
                let mut transaction = bus.lock();

                if let Err(e) = set_mode_and_gain(
                    &mut transaction,
                    address,
                    GainValue::_1X,
                    GainValue::_1X,
                    PowerMode::Sleep,
                ) {
                    warn!("Failed to disable device: {}", e);
                }
            }
            None => warn!("Failed to disable hardware: I2C bus was uninitialized"),
        };

        self.bus = None;
        self.calibration_data = None;
        self.is_loaded = false;
        Ok(())
    }
}

impl Capability for Bmp280SysfsDriver {}

#[cast_to]
impl ThermometerCapable for Bmp280SysfsDriver {
    fn get_supported_gains(&self) -> HashMap<u8, u16> {
        SUPPORTED_GAIN_VALUES
            .iter()
            .enumerate()
            .map(|(index, &value)| (index as u8, value))
            .collect()
    }

    fn get_supported_intervals(&self) -> HashMap<u8, u16> {
        SUPPORTED_STANDBY_TIMES
            .iter()
            .enumerate()
            .map(|(index, &value)| (index as u8, value))
            .collect()
    }

    fn get_gain(&self) -> Result<u16, DeviceError> {
        self.assert_state(false)?;
        Ok(self.thermometer_gain.into_multiplier())
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
            },
        };

        let address = self.config.device_address;
        let mut transaction = self.bus.as_ref().unwrap().lock();
        wait_adc_valid(&mut transaction, address, SPINWAIT_INTERVAL, self.standby_time.into_millis() + SPINWAIT_INTERVAL)?;
        set_mode_and_gain(&mut transaction, address, gain_value, self.pressure_gain, PowerMode::Normal)
            .map_err(|e| DeviceError::HardwareError(format!("failed to apply new gain value: {}", e)))?;

        self.thermometer_gain = gain_value;
        Ok(())
    }

    fn get_interval(&self) -> Result<u16, DeviceError> {
        self.assert_state(false)?;
        Ok(self.standby_time.into_millis())
    }

    fn set_interval(&mut self, interval_id: u8) -> Result<(), DeviceError> {
        self.assert_state(true)?;
        let standby_millis = match SUPPORTED_STANDBY_TIMES.get(interval_id as usize) {
            Some(time) => time,
            None => return Err(DeviceError::HardwareError(format!(
                "standby time ID is not supported: {}",
                interval_id
            ))),
        };

        let standby_time = match StandbyTime::from_millis(*standby_millis) {
            Some(time) => time,
            None => {
                error!("Failed to convert a time interval of {}ms to a StandbyTime because it is unsupported, but it is being offered in the list of supported integration times", standby_millis);
                return Err(DeviceError::Internal);
            },
        };

        let address = self.config.device_address;
        let mut transaction = self.bus.as_ref().unwrap().lock();
        wait_adc_valid(&mut transaction, address, SPINWAIT_INTERVAL, self.standby_time.into_millis() + SPINWAIT_INTERVAL)?;
        set_standby_time(&mut transaction, address, standby_time)
            .map_err(|e| DeviceError::HardwareError(format!("failed to apply new standby time: {}", e)))?;

        self.standby_time = standby_time;
        Ok(())
    }

    fn get_temperature_celsius(&mut self) -> Result<f32, DeviceError> {
        self.assert_state(true)?;

        let address = self.config.device_address;
        let calibration_data = match self.calibration_data.as_ref() {
            Some(data) => data,
            None => {
                error!("Calibration data was uninitialized");
                return Err(DeviceError::Internal);
            }
        };

        let mut transaction = self.bus.as_ref().unwrap().lock();
        // technically we should wait for the ADCs to become valid rn buuut it seems like we can read them just fine
        let (temp_raw, press_raw) = read_adc(&mut transaction, address)
            .map_err(|e| DeviceError::HardwareError(format!("failed to read sensor data: {}", e)))?;

        let (temp, _) = compensate_values(temp_raw as i32, press_raw as i32, calibration_data);
        Ok(temp)
    }

    fn get_temperature_fahrenheit(&mut self) -> Result<f32, DeviceError> {
        let temp = self.get_temperature_celsius()?;
        Ok(temp * (9.0/5.0) + 32.0)
    }
}
