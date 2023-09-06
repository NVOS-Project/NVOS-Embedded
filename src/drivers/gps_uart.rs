use crate::{
    bus::uart::UARTBusController,
    device::{Device, DeviceError}, config::{DeviceConfig, ConfigError}, capabilities::{GpsCapable, Capability},
};
use intertrait::cast_to;
use log::{debug, warn};
use nmea::{Nmea, Satellite};
use parking_lot::{Mutex, MutexGuard};
use rppal::uart::Uart;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::{
    any::Any,
    sync::{mpsc, Arc},
    thread,
    time::Duration
};
use uuid::Uuid;

const WORKER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const CYCLE_BUFFER_SIZE: usize = 256;
const MAX_PRECISION_DILUTION: f32 = 20.0;

// Serializeable implementation of the rppal parity
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Parity {
    /// No parity bit.
    None,
    /// Even parity.
    Even,
    /// Odd parity.
    Odd,
    /// Sets parity bit to `1`.
    Mark,
    /// Sets parity bit to `0`.
    Space,
}

impl From<Parity> for rppal::uart::Parity {
    fn from(value: Parity) -> Self {
        match value {
            Parity::None => rppal::uart::Parity::None,
            Parity::Even => rppal::uart::Parity::Even,
            Parity::Odd => rppal::uart::Parity::Odd,
            Parity::Mark => rppal::uart::Parity::Mark,
            Parity::Space => rppal::uart::Parity::Space,
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct UartGpsConfig {
    pub uart_port: u8,
    pub baud_rate: u32,
    pub parity: Parity,
    pub data_bits: u8,
    pub stop_bits: u8,
    pub polling_interval_ms: u32,
    pub peak_accuracy_meters: f32
}

impl Default for UartGpsConfig {
    fn default() -> Self {
        Self {
            uart_port: Default::default(),
            baud_rate: 11520,
            parity: Parity::None,
            data_bits: 8,
            stop_bits: 1,
            polling_interval_ms: 1000,
            peak_accuracy_meters: 3.0
        }
    }
}

enum WorkerMessage {
    Shutdown,
}

struct GpsWorker {
    device: Uart,
    command_channel: mpsc::Receiver<WorkerMessage>,
    shutdown_callback: mpsc::Sender<()>,
    poll_interval: u32,
    state: Arc<Mutex<Nmea>>
}

impl GpsWorker {
    fn new(
        device: Uart,
        command_channel: mpsc::Receiver<WorkerMessage>,
        shutdown_callback: mpsc::Sender<()>,
        poll_interval: u32,
        state: Arc<Mutex<Nmea>>
    ) -> Self {
        Self {
            device,
            command_channel,
            shutdown_callback,
            poll_interval,
            state
        }
    }

    fn run(&mut self) {
        let mut buffer = [0u8; CYCLE_BUFFER_SIZE];
        let mut partial_data = String::new();
        let poll_interval = Duration::from_millis(self.poll_interval as u64);
        loop {
            // Process Nmea data
            match self.device.read(&mut buffer) {
                Ok(bytes_read) => {
                    let received_data = String::from_utf8_lossy(&buffer[0..bytes_read]);
                    partial_data.push_str(&received_data);

                    let sentences: Vec<&str> = partial_data.split('\n').collect();
                    for i in 0..sentences.len() - 1 {
                        let sentence = sentences[i].trim();
                        if sentence.is_empty() {
                            warn!("Received an empty NMEA sentence, this is very weird.");
                            continue;
                        }

                        let mut state = self.state.lock();
                        if let Err(err) = state.parse(sentence) {
                            debug!("Failed to parse sentence: \"{}\": {}", sentence, err);
                        };
                    }

                    partial_data = sentences.last().map(|f| *f).unwrap_or("").to_string();
                },
                Err(err) => {
                    warn!("Failed to read data from device: {}", err);
                    continue;
                }
            };

            debug!("{}", self.state.lock().to_string());

            if let Ok(command) =  self.command_channel.recv_timeout(poll_interval) {
                match command {
                    WorkerMessage::Shutdown => {
                        debug!("Worker received shutdown request");
                        let _ = self.shutdown_callback.send(());
                        return;
                    },
                }
            };
        }
    }
}

pub struct UartGps {
    config: UartGpsConfig,
    address: Option<Uuid>,
    state: Option<Arc<Mutex<Nmea>>>,
    worker_channel: Option<Mutex<mpsc::Sender<WorkerMessage>>>,
    shutdown_callback: Option<Mutex<mpsc::Receiver<()>>>,
    is_loaded: bool,
}

impl UartGps {
    pub fn new(config: UartGpsConfig) -> Result<Self, DeviceError> {
        if config.data_bits < 5 || config.data_bits > 9 {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("data bit count is out of bounds: only 5-9 data bits are supported".to_string()).to_string()
            ));
        };

        if config.baud_rate == 0 {
            return Err(DeviceError::InvalidConfig(
               ConfigError::InvalidEntry("baud rate cannot be 0".to_string()).to_string() 
            ));
        }

        if config.stop_bits != 1 && config.stop_bits != 2 {
            return Err(DeviceError::InvalidConfig(
                ConfigError::InvalidEntry("stop bit count can be either 1 or 2".to_string()).to_string()
            ));
        }

        Ok(Self {
            config: config,
            address: None,
            state: None,
            worker_channel: None,
            shutdown_callback: None,
            is_loaded: false,
        })
    }

    pub fn from_config(config: &mut DeviceConfig) -> Result<Self, DeviceError> {
        let data: UartGpsConfig = match serde_json::from_value(config.data.clone()) {
            Ok(d) => d,
            Err(e) => {
                if config.data == Value::Null {
                    match serde_json::to_value(UartGpsConfig::default()) {
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

    fn get_state(&self) -> Result<MutexGuard<'_, Nmea>, DeviceError> {
        if !self.is_loaded || !self.state.is_some() {
            return Err(DeviceError::InvalidOperation(
                "device is in an invalid state".to_string(),
            ));
        }

        Ok(self.state.as_ref().unwrap().lock())
    }
}

impl Device for UartGps {
    fn name(&self) -> String {
        "gps_uart".to_string()
    }

    fn load(
        &mut self,
        parent: &mut crate::device::DeviceServer,
        address: Uuid,
    ) -> Result<(), DeviceError> {
        if self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device load requested but this device is already loaded".to_string(),
            ));
        }

        let mut uart = match parent.get_bus_mut::<UARTBusController>() {
            Some(bus) => bus,
            None => return Err(DeviceError::MissingController("uart".to_string())),
        };

        let config = &self.config;
        let device = match uart.open(
            config.uart_port,
            config.baud_rate,
            config.parity.clone().into(),
            config.data_bits,
            config.stop_bits,
        ) {
            Ok(c) => c,
            Err(e) => {
                return Err(DeviceError::HardwareError(format!(
                    "could not open uart channel: {}",
                    e
                )))
            }
        };

        self.address = Some(address);
        let state = Arc::new(Mutex::new(Nmea::default()));
        self.state = Some(state.clone());

        let (worker_sender, worker_receiver) = mpsc::channel::<WorkerMessage>();
        let (callback_sender, callback_receiver) = mpsc::channel::<()>();
        self.worker_channel = Some(Mutex::new(worker_sender));
        self.shutdown_callback = Some(Mutex::new(callback_receiver));
        let poll_interval = self.config.polling_interval_ms;

        debug!("Spawning worker thread");
        thread::spawn(move || {
            GpsWorker::new(device, 
                worker_receiver, 
                callback_sender,
                poll_interval,
            state).run();
        });

        self.is_loaded = true;
        Ok(())
    }

    fn unload(&mut self, parent: &mut crate::device::DeviceServer) -> Result<(), DeviceError> {
        if !self.is_loaded {
            return Err(DeviceError::InvalidOperation(
                "device unload requested but this device isn't loaded".to_string(),
            ));
        }

        match self.worker_channel.as_ref() {
            Some(channel) => {
                match channel.lock().send(WorkerMessage::Shutdown) {
                    Ok(_) => debug!("Worker shutdown requested"),
                    Err(e) => warn!("Failed to request worker shutdown: {e}"),
                };

                match self.shutdown_callback.as_ref()
                .and_then(|callback| callback.lock().recv_timeout(WORKER_SHUTDOWN_TIMEOUT).ok()) {
                    Some(_) => debug!("Worker shutdown complete"),
                    None => warn!("Could not receive a shutdown acknowledgement from the worker, this is possibly bad.")
                };

                self.worker_channel = None;
                self.shutdown_callback = None;
            }
            None => warn!("Worker thread has exited prior to unload"),
        };

        let mut uart = match parent.get_bus_mut::<UARTBusController>() {
            Some(bus) => bus,
            None => return Err(DeviceError::MissingController("uart".to_string())),
        };

        if let Err(e) = uart.close(self.config.uart_port) {
            warn!("Failed to close UART channel while shutting down: {}", e);
        }

        self.is_loaded = false;
        self.address = None;
        self.state = None;

        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl Capability for UartGps {}

#[cast_to]
impl GpsCapable for UartGps {
    fn get_location(&self) -> Result<(f64, f64), DeviceError> {
        let state = self.get_state()?;
        let lat = *state.latitude.as_ref().unwrap_or(&0.0);
        let lon = *state.longitude.as_ref().unwrap_or(&0.0);
        Ok((lat, lon))
    }

    fn get_altitude(&self) -> Result<f32, DeviceError> {
        let state = self.get_state()?;
        let alt = *state.altitude.as_ref().unwrap_or(&0.0);
        Ok(alt)
    }

    fn has_fix(&self) -> Result<bool, DeviceError> {
        let state = self.get_state()?;
        Ok(state.fix_date.is_some())
    }

    fn get_speed(&self) -> Result<f32, DeviceError> {
        let state = self.get_state()?;
        let speed = *state.speed_over_ground.as_ref().unwrap_or(&0.0);
        Ok(speed)
    }

    fn get_heading(&self) -> Result<f32, DeviceError> {
        let state = self.get_state()?;
        let heading = *state.true_course.as_ref().unwrap_or(&0.0);
        Ok(heading)
    }

    fn get_satellites(&self) -> Result<Vec<Satellite>, DeviceError> {
        let state = self.get_state()?;
        let satellites: Vec<Satellite> = state.satellites().iter()
            .map(|x| x.clone()).collect();

        Ok(satellites)
    }

    fn get_nmea(&self) -> Result<Nmea, DeviceError> {
        let state = self.get_state()?;
        let nmea = (*state).clone();
        Ok(nmea)
    }

    fn get_vertical_accuracy(&self) -> Result<f32, DeviceError> {
        let state = self.get_state()?;
        let dop = state.hdop.as_ref().unwrap_or(&MAX_PRECISION_DILUTION);
        let acc = self.config.peak_accuracy_meters * dop;
        Ok(acc)
    }

    fn get_horizontal_accuracy(&self) -> Result<f32, DeviceError> {
        let state = self.get_state()?;
        let dop = state.vdop.as_ref().unwrap_or(&MAX_PRECISION_DILUTION);
        let acc = self.config.peak_accuracy_meters * dop;
        Ok(acc)
    }
}