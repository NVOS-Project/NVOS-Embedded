use std::{sync::{Arc, atomic::{AtomicBool, Ordering}}, net::SocketAddr, time::Duration};
use log::debug;
use tokio::sync::broadcast;
use tokio::time;
use mozdevice::{Host, DeviceInfo};
use parking_lot::{Mutex, MutexGuard};
use tonic::server;

const DEFAULT_ADB_HOST: &str = "localhost";
const DEFAULT_ADB_PORT: u16 = 5037;
const DEFAULT_ADB_READ_TIMEOUT: Duration = Duration::from_secs(1);
const DEFAULT_ADB_WRITE_TIMEOUT: Duration = Duration::from_secs(1);
const CONNECTION_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(1);

pub struct AdbServer {
    connection: Arc<Mutex<Host>>,
    is_connected: Arc<AtomicBool>,
    shutdown_channel: broadcast::Sender<()>
}

impl AdbServer {
    pub fn new(host: &str, port: u16) -> Self {
        Self::with_timeout(host, port, DEFAULT_ADB_READ_TIMEOUT, DEFAULT_ADB_WRITE_TIMEOUT)
    }

    pub fn with_timeout(host: &str, port: u16, read_timeout: Duration, write_timeout: Duration) -> Self {
        let mut adb_host = Host::default();
        adb_host.host = Some(host.to_string());
        adb_host.port = Some(port);
        adb_host.read_timeout = Some(read_timeout);
        adb_host.write_timeout = Some(write_timeout);

        let (shutdown_sender, shutdown_receiver) = broadcast::channel::<()>(1);
        let server = Self {
            connection: Arc::new(Mutex::new(adb_host)),
            is_connected: Arc::new(AtomicBool::new(false)),
            shutdown_channel: shutdown_sender
        };

        let adb_host = server.connection.clone();
        let is_connected_channel = server.is_connected.clone();

        debug!("Spawning heartbeat thread");
        tokio::spawn(async move {
            AdbServerWorker::new(
                adb_host,
                is_connected_channel,
                shutdown_receiver
            ).run().await;
        });

        server
    }

    pub fn get(&mut self) -> Option<MutexGuard<'_, Host>> {
        match self.is_connected.load(Ordering::Relaxed) {
            true => Some(self.connection.lock()),
            false => None
        }
    }

    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    pub fn shutdown(&self) {
        debug!("Shutting down ADB server");
        let _ = self.shutdown_channel.send(());
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
    host: Arc<Mutex<Host>>,
    is_connected: Arc<AtomicBool>,
    shutdown_receiver: broadcast::Receiver<()>
}

impl AdbServerWorker {
    fn new(host: Arc<Mutex<Host>>, is_connected_channel: Arc<AtomicBool>, shutdown_channel: broadcast::Receiver<()>) -> Self {
        Self { 
            host: host,
            is_connected: is_connected_channel, 
            shutdown_receiver: shutdown_channel 
        }
    }

    async fn run(&mut self) {
        loop {
            tokio::select! {
                _ = time::sleep(CONNECTION_HEARTBEAT_INTERVAL) => {
                    self.do_heartbeat().await;
                },
                _ = self.shutdown_receiver.recv() => {
                    debug!("Received shutdown signal, stopping...");
                    break;
                }
            }
        }
    }

    async fn do_heartbeat(&mut self) {
        let host = self.host.lock();
        let is_connected_flag = &self.is_connected;
        if is_connected_flag.load(Ordering::Relaxed) {
            match host.devices::<Vec<DeviceInfo>>() {
                Ok(devices) => {
                    return;
                },
                Err(e) => {
                    debug!("ADB server died: {}", e);
                    is_connected_flag.store(false, Ordering::Relaxed);
                }
            }
        }

        debug!("Connecting to ADB server");
        match host.connect() {
            Ok(_) => {
                debug!("Connected to server");
                is_connected_flag.store(true, Ordering::Relaxed);
            },
            Err(e) => {
                debug!("Failed to connect to server: {}", e);
            },
        }
    }
}