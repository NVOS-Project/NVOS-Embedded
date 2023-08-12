use self::led_controller_server::LedController;
use crate::{capabilities::{LEDControllerCapable, LEDMode}, device::{DeviceServer, DeviceError}};
use parking_lot::{RwLock, RwLockReadGuard, MappedRwLockReadGuard, RwLockWriteGuard, MappedRwLockWriteGuard};
use std::{str::FromStr, sync::Arc};
use tonic::{Code, Status, Response, Request};
use uuid::Uuid;

use super::void::Void;

tonic::include_proto!("led");

fn map_led_mode(mode: LEDMode) -> LedMode {
    match mode {
        LEDMode::Visible => LedMode::Vis,
        LEDMode::Infrared => LedMode::Ir
    }
}

fn reverse_map_led_mode(mode: LedMode) -> LEDMode {
    match mode {
        LedMode::Vis => LEDMode::Visible,
        LedMode::Ir => LEDMode::Infrared
    }
}

pub struct LEDControllerService {
    server: Arc<RwLock<DeviceServer>>,
}

impl LEDControllerService {
    pub fn new(server: &Arc<RwLock<DeviceServer>>) -> Self {
        Self {
            server: server.clone(),
        }
    }

    fn get_device(
        &self,
        address: String,
    ) -> Result<MappedRwLockReadGuard<'_, dyn LEDControllerCapable>, Status> {
        let guard = self.server.read();
        let address = match Uuid::parse_str(&address) {
            Ok(addr) => addr,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Failed to parse device address: {}",
                    e
                )))
            }
        };

        let device = match guard.get_device(&address) {
            Some(device) => device,
            None => return Err(Status::not_found("Device does not exist")),
        };

        if !device.has_capability::<dyn LEDControllerCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockReadGuard::map(guard, |x| {
            x.get_device(&address)
                .unwrap()
                .as_capability_ref::<dyn LEDControllerCapable>()
                .unwrap()
        }))
    }

    fn get_device_mut(
        &self,
        address: String,
    ) -> Result<MappedRwLockWriteGuard<'_, dyn LEDControllerCapable>, Status> {
        let guard = self.server.write();
        let address = match Uuid::parse_str(&address) {
            Ok(addr) => addr,
            Err(e) => {
                return Err(Status::invalid_argument(format!(
                    "Failed to parse device address: {}",
                    e
                )))
            }
        };

        let device = match guard.get_device(&address) {
            Some(device) => device,
            None => return Err(Status::not_found("Device does not exist")),
        };

        if !device.has_capability::<dyn LEDControllerCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockWriteGuard::map(guard, |x| {
            x.get_device_mut(&address)
                .unwrap()
                .as_capability_mut::<dyn LEDControllerCapable>()
                .unwrap()
        }))
    }

}

#[tonic::async_trait]
impl LedController for LEDControllerService {
    async fn get_state(&self, req: Request<GetStateRequest>) -> Result<Response<GetStateResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let power_state = device.get_power_state();
        let brightness = device.get_brightness();
        let mode = device.get_mode();
        let mut response = GetStateResponse::default();

        response.powered_on = power_state.unwrap_or(false);
        response.brightness = brightness.unwrap_or(0.0);
        response.mode = map_led_mode(mode.unwrap_or(LEDMode::Infrared)) as i32;
        Ok(Response::new(response))
    }

    async fn set_brightness(&self, req: Request<SetBrightnessRequest>) -> Result<Response<Void>, Status> {
        let brightness = req.get_ref().brightness;
        if brightness < 0.0 || brightness > 1.0 {
            return Err(Status::out_of_range("Brightness value was out of range"));
        }

        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        match device.set_brightness(brightness) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to set brightness: {}", e)))
        }
    }

    async fn set_mode(&self, req: Request<SetModeRequest>) -> Result<Response<Void>, Status> {
        let mode = match LedMode::from_i32(req.get_ref().mode) {
            Some(mode) => mode,
            None => return Err(Status::invalid_argument("Unsupported LED mode"))
        };

        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        match device.set_mode(reverse_map_led_mode(mode)) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to set mode: {}", e)))
        }
    }

    async fn set_power_state(&self, req: Request<SetPowerStateRequest>) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        match device.set_power_state(req.get_ref().powered_on) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to set power state: {}", e)))
        }
    }
}