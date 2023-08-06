use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use parking_lot::RwLock;
use rppal::pwm::{Channel, Pwm, Error};
use uuid::Uuid;
use std::any::Any;
use crate::gpio::GpioBorrowChecker;
use crate::bus::BusController;

#[derive(Debug, PartialEq)]
pub enum PWMError {
    InvalidConfig(String),
    ChannelUnavailable(u8),
    LeaseNotFound,
    NotSupported,
    ChannelBusy(u8),
    HardwareError(String),
    Other(String)
}

impl Display for PWMError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            PWMError::InvalidConfig(msg) => format!("invalid config: {}", msg),
            PWMError::ChannelUnavailable(channel_id) => format!("pwm channel {} is not available", channel_id),
            PWMError::LeaseNotFound => format!("pwm channel is not open"),
            PWMError::NotSupported => format!("not supported"),
            PWMError::ChannelBusy(channel_id) => format!("pwm channel {} is busy", channel_id),
            PWMError::HardwareError(msg) => format!("hardware error: {}", msg),
            PWMError::Other(msg) => format!("{}", msg),
        })
    }
}

pub struct PWMBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    pin_config: HashMap<u8, u8>,
    owned_channels: HashMap<u8, Uuid>
}

impl BusController for PWMBusController {
    fn name(&self) -> String {
        "PWM".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn channel_to_u8(channel: Channel) -> Option<u8> {
    match channel {
        Channel::Pwm0 => Some(0),
        Channel::Pwm1 => Some(1),
        _ => None
    }
}

fn u8_to_channel(channel: u8) -> Option<Channel> {
    match channel {
        0 => Some(Channel::Pwm0),
        1 => Some(Channel::Pwm1),
        _ => None
    }
}

fn rppal_map_err(err: Error, default_err_msg: &str) -> PWMError {
    match err {
        Error::Io(e) => PWMError::HardwareError(format!("I/O error: {}", e)),
        _ => PWMError::Other(default_err_msg.to_string())
    }
}

impl PWMBusController {
    pub fn new(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>, pin_config: HashMap<u8, u8>) -> Result<Self, PWMError> {
        let gpio_checker = gpio_borrow.read();

        for (channel, pin) in &pin_config {
            if u8_to_channel(*channel).is_none() {
                return Err(PWMError::InvalidConfig(
                    format!("Unsupported PWM channel: channel {} pin {}",
                    channel, pin
                )))
            }
            if !gpio_checker.has_pin(*pin) {
                return Err(PWMError::InvalidConfig(
                    format!("PWM channel is attempting to use invalid pin: channel {} pin {}",
                    channel, pin
                )));
            }

            for (other_channel, other_pin) in &pin_config {
                if channel != other_channel && pin == other_pin {
                    return Err(PWMError::InvalidConfig(
                        format!("PWM channel definitions overlap: channel {} -> {} with channel {} -> {}",
                        channel, pin, other_channel, other_pin
                    )))
                }
            }
        }

        Ok(PWMBusController { 
            gpio_borrow: gpio_borrow.clone(), 
            pin_config: pin_config, 
            owned_channels: HashMap::new()
        })
    }

    pub fn open(&mut self, channel: u8) -> Result<Pwm, PWMError> {
        if self.owned_channels.contains_key(&channel) {
            return Err(PWMError::ChannelBusy(channel));
        }

        let pin = match self.pin_config.get(&channel) {
            Some(p) => p,
            None => return Err(PWMError::ChannelUnavailable(channel))
        };

        let mut borrow_checker = self.gpio_borrow.write();

        let borrow_id = borrow_checker.borrow_one(*pin)
        .map_err(|err| PWMError::HardwareError(err.to_string()))?;

        let bus = Pwm::new(u8_to_channel(channel).unwrap())
            .map_err(|err| rppal_map_err(err, &format!("Internal RPPAL error while opening PWM channel {}", channel)))?;
        
        self.owned_channels.insert(channel, borrow_id);
        Ok(bus)
    }

    pub fn close(&mut self, channel: u8) -> Result<(), PWMError> {
        let id = match self.owned_channels.get(&channel) {
            Some(i) => i,
            None => return Err(PWMError::LeaseNotFound)
        };

        self.gpio_borrow.write().release(id)
            .map_err(|err| PWMError::HardwareError(err.to_string()))?;
        self.owned_channels.remove(&channel);
        Ok(())
    }
}