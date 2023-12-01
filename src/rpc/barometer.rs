use self::barometer_server::Barometer;
use crate::capabilities::BarometerCapable;
use crate::device::DeviceServer;
use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use uuid::Uuid;

use super::errors;
use super::void::Void;

tonic::include_proto!("barometer");

pub struct BarometerService {
    server: Arc<RwLock<DeviceServer>>,
}

impl BarometerService {
    pub fn new(server: &Arc<RwLock<DeviceServer>>) -> Self {
        Self {
            server: server.clone(),
        }
    }

    fn get_device(
        &self,
        address: String,
    ) -> Result<MappedRwLockReadGuard<'_, dyn BarometerCapable>, Status> {
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

        if !device.has_capability::<dyn BarometerCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockReadGuard::map(guard, |x| {
            x.get_device(&address)
                .unwrap()
                .as_capability_ref::<dyn BarometerCapable>()
                .unwrap()
        }))
    }

    fn get_device_mut(
        &self,
        address: String,
    ) -> Result<MappedRwLockWriteGuard<'_, dyn BarometerCapable>, Status> {
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

        if !device.has_capability::<dyn BarometerCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockWriteGuard::map(guard, |x| {
            x.get_device_mut(&address)
                .unwrap()
                .as_capability_mut::<dyn BarometerCapable>()
                .unwrap()
        }))
    }
}

#[tonic::async_trait]
impl Barometer for BarometerService {
    async fn get_supported_gains(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetSupportedGainsResponse>, Status> {
        let device = self.get_device(request.get_ref().address.to_owned())?;
        let gains = device.get_supported_gains();

        let values = gains
            .into_iter()
            .map(|(id, multiplier)| GainValue {
                id: id as u32,
                multiplier: multiplier as u32,
            })
            .collect();

        Ok(Response::new(GetSupportedGainsResponse { values }))
    }

    async fn get_supported_intervals(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetSupportedIntervalsResponse>, Status> {
        let device = self.get_device(request.get_ref().address.to_owned())?;
        let intervals = device.get_supported_intervals();

        let values = intervals
            .into_iter()
            .map(|(id, time_ms)| SleepInterval {
                id: id as u32,
                time_ms: time_ms as u32,
            })
            .collect();

        Ok(Response::new(GetSupportedIntervalsResponse { values }))
    }

    async fn get_gain(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetGainResponse>, Status> {
        let device = self.get_device(request.get_ref().address.to_owned())?;
        let gain_multiplier = device.get_gain().map_err(errors::map_device_error)?;
        Ok(Response::new(GetGainResponse {
            gain_multiplier: gain_multiplier as u32,
        }))
    }

    async fn set_gain(&self, request: Request<SetGainRequest>) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(request.get_ref().address.to_owned())?;
        device
            .set_gain(request.get_ref().gain_id as u8)
            .map_err(errors::map_device_error)?;
        Ok(Response::new(Void::default()))
    }

    async fn get_interval(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetIntervalResponse>, Status> {
        let device = self.get_device(request.get_ref().address.to_owned())?;
        let sleep_interval_ms = device.get_interval().map_err(errors::map_device_error)?;
        Ok(Response::new(GetIntervalResponse {
            sleep_interval_ms: sleep_interval_ms as u32,
        }))
    }

    async fn set_interval(
        &self,
        request: Request<SetIntervalRequest>,
    ) -> Result<Response<Void>, Status> {
        let mut device = self.get_device_mut(request.get_ref().address.to_owned())?;
        device
            .set_interval(request.get_ref().interval_id as u8)
            .map_err(errors::map_device_error)?;
        Ok(Response::new(Void::default()))
    }

    async fn get_pressure(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetPressureResponse>, Status> {
        let mut device = self.get_device_mut(request.get_ref().address.to_owned())?;
        let pressure = device.get_pressure().map_err(errors::map_device_error)?;
        Ok(Response::new(GetPressureResponse { value: pressure }))
    }

    async fn get_altitude(
        &self,
        request: Request<BarometerRequest>,
    ) -> Result<Response<GetAltitudeResponse>, Status> {
        let mut device = self.get_device_mut(request.get_ref().address.to_owned())?;
        let altitude = device.get_altitude().map_err(errors::map_device_error)?;
        Ok(Response::new(GetAltitudeResponse { value: altitude }))
    }
}
