use crate::{capabilities::{LEDControllerCapable, LEDMode, GpsCapable}, device::{DeviceServer, DeviceError}};
use parking_lot::{RwLock, RwLockReadGuard, MappedRwLockReadGuard, RwLockWriteGuard, MappedRwLockWriteGuard};
use std::{str::FromStr, sync::Arc};
use tonic::{Code, Status, Response, Request};
use uuid::Uuid;

use self::gps_server::Gps;

use super::void::Void;

tonic::include_proto!("gps");


pub struct GpsService {
    server: Arc<RwLock<DeviceServer>>
}

impl GpsService {
    pub fn new(server: &Arc<RwLock<DeviceServer>>) -> Self {
        Self {
            server: server.clone(),
        }
    }

    fn get_device(
        &self,
        address: String,
    ) -> Result<MappedRwLockReadGuard<'_, dyn GpsCapable>, Status> {
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

        if !device.has_capability::<dyn GpsCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockReadGuard::map(guard, |x| {
            x.get_device(&address)
                .unwrap()
                .as_capability_ref::<dyn GpsCapable>()
                .unwrap()
        }))
    }

    fn get_device_mut(
        &self,
        address: String,
    ) -> Result<MappedRwLockWriteGuard<'_, dyn GpsCapable>, Status> {
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

        if !device.has_capability::<dyn GpsCapable>() {
            return Err(Status::invalid_argument(
                "This device does not support this capability",
            ));
        }

        Ok(RwLockWriteGuard::map(guard, |x| {
            x.get_device_mut(&address)
                .unwrap()
                .as_capability_mut::<dyn GpsCapable>()
                .unwrap()
        }))
    }

}

#[tonic::async_trait]
impl Gps for GpsService {
    async fn get_location(&self, req: Request<GpsRequest>) -> Result<Response<GetLocationResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_location() {
            Ok((lat, lon)) => Ok(Response::new(GetLocationResponse { latitude: lat, longitude: lon })),
            Err(e) => Err(Status::internal(format!("Failed to get location: {}", e)))
        }
    }

    async fn get_altitude(&self, req: Request<GpsRequest>) -> Result<Response<GetAltitudeResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_altitude() {
            Ok(alt) => Ok(Response::new(GetAltitudeResponse { altitude: alt })),
            Err(e) => Err(Status::internal(format!("Failed to get altitude: {}", e)))
        }
    }

    async fn has_fix(&self, req: Request<GpsRequest>) -> Result<Response<HasFixResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.has_fix() {
            Ok(fix) => Ok(Response::new(HasFixResponse { has_fix: fix })),
            Err(e) => Err(Status::internal(format!("Failed to get fix status: {}", e)))
        }
    }

    async fn get_speed(&self, req: Request<GpsRequest>) -> Result<Response<GetSpeedResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_speed() {
            Ok(speed) => Ok(Response::new(GetSpeedResponse { speed_over_ground: speed })),
            Err(e) => Err(Status::internal(format!("Failed to get ground speed: {}", e)))
        }
    }

    async fn get_heading(&self, req: Request<GpsRequest>) -> Result<Response<GetHeadingResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_speed() {
            Ok(heading) => Ok(Response::new(GetHeadingResponse { heading: heading })),
            Err(e) => Err(Status::internal(format!("Failed to get heading: {}", e)))
        }
    }

    async fn get_num_satellites(&self, req: Request<GpsRequest>) -> Result<Response<GetNumSatellitesResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_satellites() {
            Ok(satellite) => Ok(Response::new(GetNumSatellitesResponse { count: satellite.len() as u32 })),
            Err(e) => Err(Status::internal(format!("Failed to get number of satellites: {}", e)))
        }
    }

    async fn get_vertical_accuracy(&self, req: Request<GpsRequest>) -> Result<Response<GetAccuracyResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_vertical_accuracy() {
            Ok(acc) => Ok(Response::new(GetAccuracyResponse { accuracy: acc })),
            Err(e) => Err(Status::internal(format!("Failed to get accuracy: {}", e)))
        }
    }

    async fn get_horizontal_accuracy(&self, req: Request<GpsRequest>) -> Result<Response<GetAccuracyResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;

        match device.get_horizontal_accuracy() {
            Ok(acc) => Ok(Response::new(GetAccuracyResponse { accuracy: acc })),
            Err(e) => Err(Status::internal(format!("Failed to get accuracy: {}", e)))
        }
    }

    async fn get_full_report(&self, req: Request<GpsRequest>) -> Result<Response<GetFullReportResponse>, Status> {
        let address = req.get_ref().address.to_owned();
        let device = self.get_device(address)?;
        let mut response = GetFullReportResponse::default();

        let location = device.get_location();

        if location.is_ok() {
            let (lat, lon) = location.unwrap();
            response.latitude = lat;
            response.longitude = lon;
        }

        response.altitude = device.get_altitude().unwrap_or(0.0);
        response.speed_over_ground = device.get_speed().unwrap_or(0.0);
        response.heading = device.get_heading().unwrap_or(0.0);
        response.satellite_count = device.get_satellites().map(|x| x.len() as u32).unwrap_or(0);
        response.vertical_accuracy = device.get_vertical_accuracy().unwrap_or(0.0);
        response.horizontal_accuracy = device.get_horizontal_accuracy().unwrap_or(0.0);
        Ok(Response::new(response))
    }
}