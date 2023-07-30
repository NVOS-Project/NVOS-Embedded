use std::sync::{Arc, RwLock};

use tonic::{Result, Request, Response, Status};

use crate::device::DeviceServer;

use self::device_reflection_server::DeviceReflection;
use super::void::Void;

tonic::include_proto!("reflection");

struct DeviceReflectionService {
    server: Arc<RwLock<DeviceServer>>
}

impl DeviceReflectionService {
    pub fn new(server: Arc<RwLock<DeviceServer>>) -> Self {
        DeviceReflectionService { server: server.clone() }
    }
}

#[tonic::async_trait]
impl DeviceReflection for DeviceReflectionService {
    async fn list_devices(&self, _req: Request<Void>) -> Result<Response<ListDevicesResponse>, Status> {
        Err(Status::new(tonic::Code::Unimplemented, "Not implemented"))
    }

    async fn list_controllers(&self, _req: Request<Void>) -> Result<Response<ListControllersResponse>, Status> {
        Err(Status::new(tonic::Code::Unimplemented, "Not implemented"))
    }
}