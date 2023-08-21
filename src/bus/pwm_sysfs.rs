use super::{pwm::PWMError, BusController};
use crate::{
    config::{BusControllerConfig, ConfigError},
    gpio::{GpioBorrowChecker, GpioError},
};
use log::warn;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, path::Path, fs::OpenOptions, io::Write};
use sysfs_pwm::{Error, Pwm};
use uuid::Uuid;

const SYSFS_PWM_PATH: &str = "/sys/class/pwm";

fn sysfs_map_err(err: Error, default_err_msg: &str) -> PWMError {
    match err {
        Error::Io(msg) => PWMError::OsError(msg.to_string()),
        Error::Unexpected(msg) => PWMError::OsError(msg),
        _ => PWMError::Other(format!("{}: {}", default_err_msg.to_string(), err)),
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct PWMChannel {
    pub chip_num: u8,
    pub chip_channel: u8,
    pub gpio_num: u8,
}

impl PWMChannel {
    pub fn new(chip_num: u8, chip_channel: u8, gpio_num: u8) -> Self {
        Self {
            chip_num,
            chip_channel,
            gpio_num,
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct SysfsPWMConfigData {
    channels: HashMap<u8, PWMChannel>,
}

impl SysfsPWMConfigData {
    fn new(channels: HashMap<u8, PWMChannel>) -> Self {
        Self { channels }
    }
}

pub struct SysfsPWMBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    pin_config: HashMap<u8, PWMChannel>,
    owned_channels: HashMap<u8, Uuid>,
}

impl BusController for SysfsPWMBusController {
    fn name(&self) -> String {
        "pwm_sysfs".to_string()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl SysfsPWMBusController {
    pub fn new(
        gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>,
        pin_config: HashMap<u8, PWMChannel>,
    ) -> Result<Self, PWMError> {
        let path = Path::new(SYSFS_PWM_PATH);
        if !path.exists() || !path.is_dir() {
            return Err(PWMError::OsError("PWM is not supported on this system".to_string()));
        }

        let gpio_checker = gpio_borrow.read();

        for (channel_id, channel_data) in &pin_config {
            if !gpio_checker.has_pin(channel_data.gpio_num) {
                return Err(PWMError::InvalidConfig(format!(
                    "PWM channel is attempting to use invalid pin: channel {} pin {}",
                    channel_id, channel_data.gpio_num
                )));
            }

            for (other_channel_id, other_channel_data) in &pin_config {
                if channel_id != other_channel_id
                    && channel_data.gpio_num == other_channel_data.gpio_num
                {
                    return Err(PWMError::InvalidConfig(format!(
                        "PWM channel definitions overlap: channel {} -> {} with channel {} -> {}",
                        channel_id,
                        channel_data.gpio_num,
                        other_channel_id,
                        other_channel_data.gpio_num
                    )));
                }
            }
        }

        Ok(SysfsPWMBusController {
            gpio_borrow: gpio_borrow.clone(),
            pin_config: pin_config,
            owned_channels: HashMap::new(),
        })
    }

    pub fn from_config(
        gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>,
        config: &mut BusControllerConfig,
    ) -> Result<Self, PWMError> {
        let data: SysfsPWMConfigData = match serde_json::from_value(config.data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.data == Value::Null {
                    config.data = match serde_json::to_value(SysfsPWMConfigData::default()) {
                        Ok(c) => c,
                        Err(e) => {
                            warn!("Failed to write default configuration: {}", e);
                            Value::Null
                        }
                    }
                }

                return Err(PWMError::InvalidConfig(
                    ConfigError::SerializeError(format!("invalid PWM data struct json: {}", e))
                        .to_string(),
                ));
            }
        };

        Self::new(gpio_borrow, data.channels)
    }

    pub fn open(&mut self, channel: u8) -> Result<Pwm, PWMError> {
        if self.owned_channels.contains_key(&channel) {
            return Err(PWMError::ChannelBusy(channel));
        }

        let pwm_data = match self.pin_config.get(&channel) {
            Some(p) => p,
            None => return Err(PWMError::ChannelNotFound(channel)),
        };

        let mut borrow_checker = self.gpio_borrow.write();
        if !borrow_checker.can_borrow_one(pwm_data.gpio_num) {
            return Err(PWMError::HardwareError(
                GpioError::Busy(pwm_data.gpio_num).to_string(),
            ));
        }

        let bus = Pwm::new(pwm_data.chip_num as u32, pwm_data.chip_channel as u32)
            .and_then(|pwm| pwm.export().map(|_| pwm))
            .map_err(|err| {
                sysfs_map_err(
                    err,
                    &format!(
                        "Internal sysfs error while opening PWM channel {} (channel {} on chip {})",
                        channel, pwm_data.chip_channel, pwm_data.chip_num
                    ),
                )
            })?;

        // Try to reset PWM polarity if supported
        // error out if polarity can't be set
        let polarity_path = Path::new(SYSFS_PWM_PATH).join(format!("pwmchip{}/pwm{}/polarity", pwm_data.chip_num, pwm_data.chip_channel));
        if polarity_path.exists() {
            OpenOptions::new().write(true).open(polarity_path)
                .and_then(|mut fd| fd.write_all(b"normal"))
                .map_err(|err| PWMError::HardwareError(format!("failed to reset PWM polarity: {}", err)))?;
        }

        let borrow_id = borrow_checker.borrow_one(pwm_data.gpio_num)
            .map_err(|err| PWMError::HardwareError(err.to_string()))?;
        
        self.owned_channels.insert(channel, borrow_id);

        Ok(bus)
    }

    pub fn close(&mut self, channel: u8) -> Result<(), PWMError> {
        let id = match self.owned_channels.get(&channel) {
            Some(i) => i,
            None => return Err(PWMError::LeaseNotFound),
        };

        let pwm_data = match self.pin_config.get(&channel) {
            Some(p) => p,
            None => return Err(PWMError::ChannelNotFound(channel)),
        };

        Pwm::new(pwm_data.chip_num as u32, pwm_data.chip_channel as u32)
            .and_then(|pwm| pwm.unexport())
            .map_err(|err| {
                sysfs_map_err(
                    err,
                    &format!(
                        "Internal sysfs error while closing PWM channel {} (channel {} on chip {})",
                        channel, pwm_data.chip_channel, pwm_data.chip_num
                    ),
                )
            })?;

        self.gpio_borrow.write().release(id)
            .map_err(|err| PWMError::HardwareError(err.to_string()))?;
        self.owned_channels.remove(&channel);
        Ok(())
    }
}
