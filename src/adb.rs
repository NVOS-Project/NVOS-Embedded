use log::{debug, error, warn};
use mozdevice::{AndroidStorageInput, Device, DeviceError, DeviceInfo, Host};
use parking_lot::{MappedMutexGuard, Mutex, MutexGuard, RwLock};
use std::{sync::Arc, time::Duration};
use tokio::sync::broadcast;
use tokio::time;

const DEFAULT_ADB_HOST: &str = "localhost";
const DEFAULT_ADB_PORT: u16 = 5037;
const DEFAULT_ADB_READ_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_ADB_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECTION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq)]
pub enum PortType {
    Forward,
    Reverse,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Port {
    port_type: PortType,
    local_port_num: u16,
    remote_port_num: u16,
}

impl Port {
    pub fn new(port_type: PortType, local_port_num: u16, remote_port_num: u16) -> Self {
        Self {
            port_type,
            local_port_num,
            remote_port_num,
        }
    }
}

#[derive(Clone, Debug)]
enum AdbMessage {
    Shutdown,
}

pub struct AdbServer {
    device: Arc<Mutex<Option<Device>>>,
    channel: broadcast::Sender<AdbMessage>,
    forwarded_connections: Arc<RwLock<Vec<Port>>>,
}

impl AdbServer {
    pub fn new(host: &str, port: u16) -> Self {
        Self::with_timeout(
            host,
            port,
            DEFAULT_ADB_READ_TIMEOUT,
            DEFAULT_ADB_WRITE_TIMEOUT,
        )
    }

    pub fn with_timeout(
        host: &str,
        port: u16,
        read_timeout: Duration,
        write_timeout: Duration,
    ) -> Self {
        let mut adb_host = Host::default();
        adb_host.host = Some(host.to_string());
        adb_host.port = Some(port);
        adb_host.read_timeout = Some(read_timeout);
        adb_host.write_timeout = Some(write_timeout);

        let (sender, receiver) = broadcast::channel::<AdbMessage>(16);
        let server = Self {
            device: Arc::new(Mutex::new(None)),
            channel: sender,
            forwarded_connections: Arc::new(RwLock::new(Vec::new())),
        };

        let device = server.device.clone();
        let forwarded_connections = server.forwarded_connections.clone();

        debug!("Spawning heartbeat thread");
        tokio::spawn(async move {
            AdbServerWorker::new(adb_host, device, forwarded_connections, receiver)
                .run()
                .await;
        });

        server
    }

    pub fn get_device(&self) -> Result<MappedMutexGuard<'_, Device>, DeviceError> {
        let guard = self.device.lock();
        match guard.is_some() {
            true => Ok(MutexGuard::map(guard, |v| v.as_mut().unwrap())),
            false => Err(DeviceError::Adb("device not connected".to_string())),
        }
    }

    pub fn has_device(&self) -> bool {
        self.device.lock().is_some()
    }

    pub fn forward_port(
        &self,
        port_type: PortType,
        local_port: u16,
        remote_port: u16,
        require_connection: bool
    ) -> Result<(), DeviceError> {
        debug!(
            "Adding port: {:?}, {}, {}",
            port_type, local_port, remote_port
        );

        if require_connection {
            // Caller wants the port forwarded NOW
            // This will fail if no device is connected to the network
            let device = self.get_device()?;

            match port_type {
                PortType::Forward => device.forward_port(local_port, remote_port),
                PortType::Reverse => device.reverse_port(remote_port, local_port),
            }?;
        }

        self.forwarded_connections.write().push(Port::new(
            port_type,
            local_port,
            remote_port,
        ));
        Ok(())
    }

    pub fn remove_forward_port(&self, local_port: u16, require_connection: bool) -> Result<(), DeviceError> {
        debug!("Removing forward port {}", local_port);

        let mut connections = self.forwarded_connections.write();
        let idx = match connections
            .iter()
            .position(|x| x.port_type == PortType::Forward && x.local_port_num == local_port)
        {
            Some(i) => i,
            None => {
                return Err(DeviceError::Adb(format!(
                    "local port {} is not in use",
                    local_port
                )))
            }
        };

        if require_connection {
            // See forward_port for details
            let device = self.get_device()?;
            device.kill_forward_port(local_port)?;
        }

        connections.remove(idx);
        Ok(())
        
    }

    pub fn remove_reverse_port(&self, remote_port: u16, require_connection: bool) -> Result<(), DeviceError> {
        debug!("Removing reverse port {}", remote_port);

        let mut connections = self.forwarded_connections.write();
        let idx = match connections
            .iter()
            .position(|x| x.port_type == PortType::Reverse && x.remote_port_num == remote_port)
        {
            Some(i) => i,
            None => {
                return Err(DeviceError::Adb(format!(
                    "remote port {} is not in use",
                    remote_port
                )))
            }
        };

        if require_connection {
            // See forward_port for details
            let device = self.get_device()?;
            device.kill_reverse_port(remote_port)?;
        }
        
        connections.remove(idx);
        Ok(())
    }

    pub fn is_connected(&self) -> bool {
        self.device.lock().is_some()
    }

    pub fn shutdown(&self) {
        debug!("Shutting down ADB server");
        let _ = self.channel.send(AdbMessage::Shutdown);
    }
}

impl Default for AdbServer {
    fn default() -> Self {
        Self::new(DEFAULT_ADB_HOST, DEFAULT_ADB_PORT)
    }
}

impl Drop for AdbServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct AdbServerWorker {
    host: Host,
    device: Arc<Mutex<Option<Device>>>,
    forwarded_connections: Arc<RwLock<Vec<Port>>>,
    channel: broadcast::Receiver<AdbMessage>,
    is_connected: bool,
}

impl AdbServerWorker {
    fn new(
        host: Host,
        device: Arc<Mutex<Option<Device>>>,
        forwarded_connections: Arc<RwLock<Vec<Port>>>,
        channel: broadcast::Receiver<AdbMessage>,
    ) -> Self {
        Self {
            host,
            device,
            forwarded_connections,
            channel,
            is_connected: false,
        }
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                _ = time::sleep(CONNECTION_HEARTBEAT_INTERVAL) => {
                    self.run_checks().await;
                },
                signal = self.channel.recv() => {
                    if !signal.is_ok() {
                        continue;
                    }

                    match signal.unwrap() {
                        AdbMessage::Shutdown => {
                            debug!("Received shutdown signal, stopping...");
                            break;
                        },
                        other => {
                            error!("ADB server worker received unsupported signal: {:?}", other);
                            break;
                        }
                    }
                }
            }
        }
    }

    async fn run_checks(&mut self) {
        if !self.is_connected {
            match self.connect_server().await {
                true => self.is_connected = true,
                false => return
            };
        }

        if self.device.lock().is_none() {
            if !self.connect_device().await {
                return;
            }

            self.restore_port_map().await;
            return;
        }

        let mut device = self.device.lock();
        // Equivalent to a server heartbeat
        let devices = match self.host.devices::<Vec<DeviceInfo>>() {
            Ok(devices) => devices,
            Err(e) => {
                debug!("Lost server connection: {}", e);
                *device = None;
                self.is_connected = false;
                return;
            }
        };

        if devices.len() == 0 && device.is_some() {
            debug!("Lost device connection");
            *device = None;
        }
    }

    async fn connect_server(&mut self) -> bool {
        match self.host.connect() {
            Ok(_) => {
                debug!("Connected to server");
                true
            }
            Err(e) => {
                debug!("Failed to connect to server: {}", e);
                false
            }
        }
    }

    async fn connect_device(&mut self) -> bool {
        if !self.is_connected {
            error!("Failed to connect to device: not connected to adb server");
            return false;
        }

        let host = &self.host;
        let host_cloned = Host {
            host: host.host.to_owned(),
            port: host.port,
            read_timeout: host.read_timeout,
            write_timeout: host.write_timeout,
        };

        match host_cloned.connect() {
            Ok(_) => {},
            Err(e) => {
                error!("Failed to create a device tunnel: {}", e);
                return false;
            }
        }

        match host_cloned.device_or_default::<String>(None, AndroidStorageInput::Auto) {
            Ok(device) => {
                debug!("Got a device! serial: {}", device.serial);
                let mut guard = self.device.lock();
                *guard = Some(device);
                return true;
            }
            Err(e) => {
                debug!("Failed to obtain a device connection: {}", e);
                return false;
            }
        }
    }

    async fn restore_port_map(&mut self) {
        let connections = self.forwarded_connections.read().clone();

        if connections.len() == 0 {
            debug!("No connections to restore, aborting.");
            return;
        }

        let guard = self.device.lock();
        let device = match *guard {
            Some(ref d) => d,
            None => {
                error!("Failed to restore ADB connection mappings: device not connected");
                return;
            }
        };

        for port in connections {
            let r = match port.port_type {
                PortType::Forward => device.forward_port(port.local_port_num, port.local_port_num),
                PortType::Reverse => device.reverse_port(port.remote_port_num, port.local_port_num),
            };

            match r {
                Ok(_) => debug!("Restored port mapping: {:?}", port),
                Err(_) => warn!("Failed to restore mapping {:?}: {}", port, r.unwrap_err())
            }
        }
    }
}
