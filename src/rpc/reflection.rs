use std::sync::Arc;
use parking_lot::RwLock;
use tonic::{Result, Request, Response, Status};
use crate::device::DeviceServer;
use self::device_reflection_server::DeviceReflection;
use super::void::Void;

tonic::include_proto!("reflection");

pub struct DeviceReflectionService {
    server: Arc<RwLock<DeviceServer>>
}

impl DeviceReflectionService {
    pub fn new(server: &Arc<RwLock<DeviceServer>>) -> Self {
        DeviceReflectionService { server: server.clone() }
    }
}

fn map_capability_to_rpc(cap: crate::capabilities::CapabilityId) -> self::CapabilityId {
    match cap {
        crate::capabilities::CapabilityId::LEDController => CapabilityId::LedController,
        crate::capabilities::CapabilityId::GPS => CapabilityId::Gps,
    }
}

fn map_capabilities_to_rpc(caps: Vec<crate::capabilities::CapabilityId>) -> Vec<self::CapabilityId> {
    caps.iter().map(|x| map_capability_to_rpc(x.to_owned())).collect()
}

#[tonic::async_trait]
impl DeviceReflection for DeviceReflectionService {
    async fn list_devices(&self, _req: Request<Void>) -> Result<Response<ListDevicesResponse>, Status> {
        let mut devices = Vec::<Device>::new();
        for (address, device) in self.server.read().get_devices() {
            devices.push(Device { 
                address: address.to_string(),
                capabilities: map_capabilities_to_rpc(device.get_capabilities())
                    .into_iter().map(|x| x as i32).collect()
            });
        }

        Ok(Response::new(ListDevicesResponse { count: devices.len() as u32, devices: devices }))
    }

    async fn list_controllers(&self, _req: Request<Void>) -> Result<Response<ListControllersResponse>, Status> {
        let mut controllers = Vec::<BusController>::new();
        for controller in self.server.read().get_buses() {
            controllers.push(BusController { name: controller.name() });
        }

        Ok(Response::new(ListControllersResponse { count: controllers.len() as u32, controllers: controllers }))
    }
}