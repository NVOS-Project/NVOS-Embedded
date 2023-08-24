use crate::{
    bus::{pwm_sysfs::SysfsPWMBusController, raw_sysfs::SysfsRawBusController},
    capabilities::{Capability, LEDControllerCapable, LEDMode},
    config::{ConfigError, DeviceConfig},
    device::{Device, DeviceError, DeviceServer},
};
use intertrait::cast_to;
use log::{warn, debug};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::any::Any;
use sysfs_gpio::Pin;
use sysfs_pwm::Pwm;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Debug)]
pub struct SysfsLedControllerConfig {
    pub brightness_pwm_channel: u8,
    pub mode_switch_pin: u8,
    pub default_mode: LEDMode,
    pub default_brightness: f32,
    pub default_power_state_on: bool,
    pub power_on_gpio_state: u8,
    pub power_off_gpio_state: u8,
    pub ir_mode_gpio_state: u8,
    pub vis_mode_gpio_state: u8,
    pub pwm_period: u32,
    pub pwm_0_brightness_duty_cycle: u32,
    pub pwm_100_brightness_duty_cycle: u32,
}

impl Default for SysfsLedControllerConfig {
    fn default() -> Self {
        Self {
            brightness_pwm_channel: Default::default(),
            mode_switch_pin: Default::default(),
            // try not to burn out people's eyes until explicitly told to
            default_mode: LEDMode::Visible,
            default_brightness: 0.5,
            // power on the LEDs immediately to make sure we can get tracking
            default_power_state_on: true,
            power_on_gpio_state: 1,
            power_off_gpio_state: 0,
            ir_mode_gpio_state: 0,
            vis_mode_gpio_state: 1,
            pwm_period: 100,
            pwm_0_brightness_duty_cycle: 0,
            pwm_100_brightness_duty_cycle: 100,
        }
    }
}

pub struct SysfsLedController {
    config: SysfsLedControllerConfig,
    address: Option<Uuid>,
    mode_switch_pin: Option<Pin>,
    brightness_pin: Option<Pwm>,
    mode: LEDMode,
    brightness: f32,
    power_state_on: bool,
    is_loaded: bool,
}

impl SysfsLedController {
    pub fn new(config: SysfsLedControllerConfig) -> Result<Self, DeviceError> {
        let mode = config.default_mode;
        let brightness = config.default_brightness;
        let power_state = config.default_power_state_on;

        if config.power_off_gpio_state == config.power_on_gpio_state {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("GPIO values for power states overlap".to_string()).to_string(),
            ));
        }

        if config.ir_mode_gpio_state == config.vis_mode_gpio_state {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("GPIO values for modes overlap".to_string()).to_string(),
            ));
        }

        if config.pwm_period == 0 {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("PWM period must be greater than zero".to_string())
                    .to_string(),
            ));
        }

        if config.pwm_0_brightness_duty_cycle == config.pwm_100_brightness_duty_cycle {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("PWM duty cycles overlap".to_string()).to_string(),
            ));
        }

        Ok(Self {
            config: config,
            address: None,
            mode_switch_pin: None,
            brightness_pin: None,
            mode: mode,
            brightness: brightness,
            power_state_on: power_state,
            is_loaded: false,
        })
    }

    pub fn from_config(config: &mut DeviceConfig) -> Result<Self, DeviceError> {
        let data: SysfsLedControllerConfig = match serde_json::from_value(config.data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.data == Value::Null {
                    match serde_json::to_value(SysfsLedControllerConfig::default()) {
                        Ok(c) => {
                            config.data = c;
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
                        "failed to deseiralize device config data: {}",
                        e
                    ))
                    .to_string(),
                ));
            }
        };

        Self::new(data)
    }
}

impl Device for SysfsLedController {
    fn name(&self) -> String {
        "sysfs_generic_led".to_string()
    }

    fn load(&mut self, parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError> {
        self.address = Some(address);
        let mut gpio = match parent.get_bus_mut::<SysfsRawBusController>() {
            Some(bus) => bus,
            None => return Err(DeviceError::MissingController("sysfs_raw".to_string())),
        };
        let mut pwm = match parent.get_bus_mut::<SysfsPWMBusController>() {
            Some(bus) => bus,
            None => return Err(DeviceError::MissingController("sysfs_pwm".to_string())),
        };

        let mode_switch_pin = match gpio.open_out(self.config.mode_switch_pin) {
            Ok(pin) => pin,
            Err(e) => {
                return Err(DeviceError::HardwareError(format!(
                    "could not get mode switch pin: {}",
                    e
                )))
            }
        };

        let brightness_pin = match pwm.open(self.config.brightness_pwm_channel) {
            Ok(channel) => channel,
            Err(e) => {
                if let Err(e) = gpio.close(mode_switch_pin) {
                    warn!(
                        "Failed to close mode switch pin while recovering from an error: {}",
                        e
                    );
                }

                return Err(DeviceError::HardwareError(format!(
                    "could not get brightness control pwm channel: {}",
                    e
                )));
            }
        };

        if let Err(e) = brightness_pin.enable(true) {
            warn!("Failed to enable brightness PWM channel: {}", e);
        }

        self.mode_switch_pin = Some(mode_switch_pin);
        self.brightness_pin = Some(brightness_pin);

        // Try to set the default state on everything
        self.is_loaded = true;
        if let Err(e) = self.set_mode(self.config.default_mode) {
            warn!("Failed to set initial mode: {}", e);
        }
        if let Err(e) = self.set_brightness(self.config.default_brightness) {
            warn!("Failed to set initial brightness: {}", e);
        }
        if let Err(e) = self.set_power_state(self.config.default_power_state_on) {
            warn!("Failed to set initial power state: {}", e);
        }

        Ok(())
    }

    fn unload(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::Other(
                "device unload requested but this device isn't loaded".to_string(),
            ));
        }

        // Try to reset the state
        if let Err(e) = self.set_mode(self.config.default_mode) {
            warn!("Failed to reset mode: {}", e);
        }
        if let Err(e) = self.set_brightness(0.0) {
            warn!("Failed to reset brightness: {}", e);
        }
        if let Err(e) = self.set_power_state(false) {
            warn!("Failed to reset power state: {}", e);
        }

        if self.mode_switch_pin.is_some() {
            let mut gpio = match parent.get_bus_mut::<SysfsRawBusController>() {
                Some(bus) => bus,
                None => return Err(DeviceError::MissingController("sysfs_raw".to_string())),
            };

            if let Err(e) = gpio.close(self.mode_switch_pin.unwrap()) {
                warn!("Failed to close mode switch pin while shutting down: {}", e);
            }

            self.mode_switch_pin = None;
        }

        if self.brightness_pin.is_some() {
            let mut pwm = match parent.get_bus_mut::<SysfsPWMBusController>() {
                Some(bus) => bus,
                None => return Err(DeviceError::MissingController("sysfs_pwm".to_string())),
            };

            if let Err(e) = self.brightness_pin.as_ref().unwrap().enable(false) {
                warn!("Failed to disable brightness PWM channel: {}", e);
            }

            if let Err(e) = pwm.close(self.config.brightness_pwm_channel) {
                warn!(
                    "Failed to close brightness control pin while shutting down: {}",
                    e
                );
            }

            self.brightness_pin = None;
        }

        self.is_loaded = false;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Capability for SysfsLedController {}

#[cast_to]
impl LEDControllerCapable for SysfsLedController {
    fn get_mode(&self) -> Result<LEDMode, DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        Ok(self.mode.clone())
    }

    fn set_mode(&mut self, mode: LEDMode) -> Result<(), DeviceError> {
        if !self.is_loaded || !self.mode_switch_pin.is_some() {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        let gpio_value = match mode {
            LEDMode::Visible => self.config.vis_mode_gpio_state,
            LEDMode::Infrared => self.config.ir_mode_gpio_state,
            _ => {
                return Err(DeviceError::InvalidOperation(format!(
                    "LED mode is not supported: {:?}",
                    mode
                )))
            }
        };

        let pin = self.mode_switch_pin.as_ref().unwrap();
        match pin.set_value(gpio_value) {
            Ok(_) => {
                debug!("new mode: {:?}", mode);
                self.mode = mode;
                Ok(())
            }
            Err(e) => Err(DeviceError::HardwareError(format!(
                "failed to set mode: {}",
                e
            ))),
        }
    }

    fn get_brightness(&self) -> Result<f32, DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        Ok(self.brightness.clone())
    }

    fn set_brightness(&mut self, mut brightness: f32) -> Result<(), DeviceError> {
        if !self.is_loaded || !self.brightness_pin.is_some() {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        brightness = brightness.clamp(0.0, 1.0);
        let pwm = self.brightness_pin.as_ref().unwrap();
        if let Err(e) = pwm.set_period_ns(self.config.pwm_period) {
            return Err(DeviceError::HardwareError(format!(
                "failed to set brightness: could not set pwm period: {}",
                e
            )));
        }

        let duty_cycle = match self.power_state_on {
            true => {
                (((self.config.pwm_100_brightness_duty_cycle
                    - self.config.pwm_0_brightness_duty_cycle) as f32)
                    * brightness) as u32
            }
            false => self.config.pwm_0_brightness_duty_cycle,
        };

        if let Err(e) = pwm.set_duty_cycle_ns(duty_cycle) {
            return Err(DeviceError::HardwareError(format!(
                "failed to set brightness: could not set pwm duty cycle: {}",
                e
            )));
        }

        debug!("new brightness: {}", brightness);
        self.brightness = brightness;
        Ok(())
    }

    fn get_power_state(&self) -> Result<bool, DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        Ok(self.power_state_on.clone())
    }

    fn set_power_state(&mut self, powered_on: bool) -> Result<(), DeviceError> {
        if !self.is_loaded || !self.brightness_pin.is_some() {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        let pwm = self.brightness_pin.as_ref().unwrap();
        if let Err(e) = pwm.set_period_ns(self.config.pwm_period) {
            return Err(DeviceError::HardwareError(format!(
                "failed to set power state: could not set pwm period: {}",
                e
            )));
        }

        let duty_cycle = match powered_on {
            true => {
                (((self.config.pwm_100_brightness_duty_cycle
                    - self.config.pwm_0_brightness_duty_cycle) as f32)
                    * self.brightness) as u32
            }
            false => self.config.pwm_0_brightness_duty_cycle,
        };

        if let Err(e) = pwm.set_duty_cycle_ns(duty_cycle) {
            return Err(DeviceError::HardwareError(format!(
                "failed to set power state: could not set pwm duty cycle: {}",
                e
            )));
        }

        debug!("new power state: {}", powered_on);
        self.power_state_on = powered_on;
        Ok(())
    }
}
