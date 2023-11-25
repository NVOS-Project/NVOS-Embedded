use self::light_sensor_server::LightSensor;
use crate::{capabilities::LightSensorCapable, device::DeviceServer};
use parking_lot::{RwLock, RwLockReadGuard, MappedRwLockReadGuard, RwLockWriteGuard, MappedRwLockWriteGuard};
use std::sync::Arc;
use tonic::{Status, Response, Request};
use uuid::Uuid;

use super::void::Void;
use crate::rpc::errors;

tonic::include_proto!("light_sensor");

pub struct LightSensorService {
    server: Arc<RwLock<DeviceServer>>,
}

impl LightSensorService {
    pub fn new(server: &Arc<RwLock<DeviceServer>>) -> Self {
        Self {
            server: server.clone(),
        }
    }

    fn get_device(
        &self,
        address: String,
    ) -> Result<MappedRwLockReadGuard<'_, dyn LightSensorCapable>, Status> {
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

        if !device.has_capability::<dyn LightSensorCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockReadGuard::map(guard, |x| {
            x.get_device(&address)
                .unwrap()
                .as_capability_ref::<dyn LightSensorCapable>()
                .unwrap()
        }))
    }

    fn get_device_mut(
        &self,
        address: String,
    ) -> Result<MappedRwLockWriteGuard<'_, dyn LightSensorCapable>, Status> {
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

        if !device.has_capability::<dyn LightSensorCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockWriteGuard::map(guard, |x| {
            x.get_device_mut(&address)
                .unwrap()
                .as_capability_mut::<dyn LightSensorCapable>()
                .unwrap()
        }))
    }
}

#[tonic::async_trait]
impl LightSensor for LightSensorService {
    async fn get_supported_channels(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetSupportedChannelsResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let supported_channels = device.get_supported_channels();
        
        let channels: Vec<Channel> = supported_channels
            .into_iter()
            .map(|(id, name)| Channel { id: (id as u32), name })
            .collect();

        let response = GetSupportedChannelsResponse { values: channels };
        Ok(Response::new(response))
    }

    async fn get_supported_gains(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetSupportedGainsResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let supported_gains = device.get_supported_gains();
        
        let gains: Vec<GainValue> = supported_gains
            .into_iter()
            .map(|(id, multiplier)| GainValue { id: (id as u32), multiplier: (multiplier as u32) })
            .collect();

        let response = GetSupportedGainsResponse { values: gains };
        Ok(Response::new(response))
    }

    async fn get_supported_intervals(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetSupportedIntervalsResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let supported_intervals = device.get_supported_intervals();
        
        let intervals: Vec<IntegrationTime> = supported_intervals
            .into_iter()
            .map(|(id, time_ms)| IntegrationTime { id: (id as u32), time_ms: (time_ms as u32) })
            .collect();

        let response = GetSupportedIntervalsResponse { values: intervals };
        Ok(Response::new(response))
    }

    async fn get_auto_gain_enabled(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetAutoGainEnabledResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let auto_gain_enabled = device.get_auto_gain_enabled().map_err(errors::map_device_error)?;
        let response = GetAutoGainEnabledResponse { enabled: auto_gain_enabled };
        Ok(Response::new(response))
    }

    async fn set_auto_gain_enabled(
        &self,
        req: Request<SetAutoGainEnabledRequest>,
    ) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        device.set_auto_gain_enabled(req.get_ref().enabled).map_err(errors::map_device_error)?;
        Ok(Response::new(Void::default()))
    }

    async fn get_gain(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetGainResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let gain = device.get_gain().map_err(errors::map_device_error)?;
        let response = GetGainResponse {
            gain_multiplier: gain as u32,
        };
        Ok(Response::new(response))
    }

    async fn set_gain(
        &self,
        req: Request<SetGainRequest>,
    ) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        let gain_id = req.get_ref().gain_id;
        if gain_id > u8::MAX as u32 {
            return Err(Status::out_of_range("gain ID was out of range"));
        }

        device.set_gain(gain_id as u8).map_err(errors::map_device_error)?;
        Ok(Response::new(Void::default()))
    }

    async fn get_interval(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetIntervalResponse>, Status> {
        let device = self.get_device(req.get_ref().address.to_owned())?;
        let interval = device.get_interval().map_err(errors::map_device_error)?;
        let response = GetIntervalResponse {
            integration_time_ms: interval as u32,
        };
        Ok(Response::new(response))
    }

    async fn set_interval(
        &self,
        req: Request<SetIntervalRequest>,
    ) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        let interval_id = req.get_ref().interval_id;
        if interval_id > u8::MAX as u32 {
            return Err(Status::out_of_range("interval ID was out of range"));
        }

        device.set_interval(interval_id as u8).map_err(errors::map_device_error)?;
        Ok(Response::new(Void::default()))
    }

    async fn get_luminosity(
        &self,
        req: Request<GetLuminosityRequest>,
    ) -> Result<Response<GetLuminosityResponse>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        let channel_id = req.get_ref().channel_id;
        if channel_id > u8::MAX as u32 {
            return Err(Status::out_of_range("channel ID was out of range"));
        }

        let luminosity = device.get_luminosity(channel_id as u8).map_err(errors::map_device_error)?;
        let response = GetLuminosityResponse { value: luminosity };
        Ok(Response::new(response))
    }

    async fn get_illuminance(
        &self,
        req: Request<LightSensorRequest>,
    ) -> Result<Response<GetIlluminanceResponse>, Status> {
        let mut device = self.get_device_mut(req.get_ref().address.to_owned())?;
        let illuminance = device.get_illuminance().map_err(errors::map_device_error)?;
        let response = GetIlluminanceResponse { value: illuminance };
        Ok(Response::new(response))
    }
}