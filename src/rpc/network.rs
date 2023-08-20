use std::sync::Arc;
use parking_lot::RwLock;
use tonic::{Request, Status, Response};
use crate::adb::{AdbServer, self};
use self::network_manager_server::NetworkManager;
use super::void::Void;

tonic::include_proto!("network");

pub struct NetworkManagerService {
    server: Arc<RwLock<AdbServer>>
}

impl NetworkManagerService {
    pub fn new(server: &Arc<RwLock<AdbServer>>) -> Self {
        Self {
            server: server.clone(),
        }
    }
}

#[tonic::async_trait]
impl NetworkManager for NetworkManagerService {
    async fn get_running_ports(&self, _req: Request<Void>) -> Result<Response<GetRunningPortsResponse>, Status> {
        let server = self.server.read();
        let mut ports = Vec::new();

        for port in server.get_running_ports().iter() {
            let port_type = match port.port_type {
                adb::PortType::Forward => PortType::Forward,
                adb::PortType::Reverse => PortType::Reverse
            };

            ports.push(Port { r#type: port_type as i32, local_port: port.local_port_num as u32, remote_port: port.remote_port_num as u32 });
        }

        Ok(Response::new(GetRunningPortsResponse { ports }))
    }

    async fn add_forward_port(&self, req: Request<AddPortRequest>) -> Result<Response<Void>, Status> {
        let data = req.get_ref();
        let device_port: u16 = match data.device_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Device port was out of range: {}", e)))
        };
        let server_port: u16 = match data.server_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Server port was out of range: {}", e)))
        };

        let server = self.server.read();
        match server.add_port(adb::PortType::Forward, server_port, device_port, true) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to add port: {}", e)))
        }
    }

    async fn add_reverse_port(&self, req: Request<AddPortRequest>) -> Result<Response<Void>, Status> {
        let data = req.get_ref();
        let device_port: u16 = match data.device_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Device port was out of range: {}", e)))
        };
        let server_port: u16 = match data.server_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Server port was out of range: {}", e)))
        };

        let server = self.server.read();
        match server.add_port(adb::PortType::Forward, server_port, device_port, false) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to add port: {}", e)))
        }
    }

    async fn remove_forward_port(&self, req: Request<RemoveForwardPortRequest>) -> Result<Response<Void>, Status> {
        let data = req.get_ref();
        let server_port: u16 = match data.server_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Server port was out of range: {}", e)))
        };

        let server = self.server.read();
        match server.remove_forward_port(server_port, false) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to remove port: {}", e)))
        }
    }

    async fn remove_reverse_port(&self, req: Request<RemoveReversePortRequest>) -> Result<Response<Void>, Status> {
        let data = req.get_ref();
        let device_port: u16 = match data.device_port.try_into() {
            Ok(port) => port,
            Err(e) => return Err(Status::invalid_argument(format!("Device port was out of range: {}", e)))
        };

        let server = self.server.read();
        match server.remove_reverse_port(device_port, false) {
            Ok(_) => Ok(Response::new(Void::default())),
            Err(e) => Err(Status::internal(format!("Failed to remove port: {}", e)))
        }
    }
}