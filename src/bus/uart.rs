use std::collections::HashMap;
use std::fmt::Display;
use std::path::Path;
use std::sync::Arc;
use parking_lot::RwLock;
use rppal::uart::{Uart, Parity, Error};
use uuid::Uuid;
use std::any::Any;
use crate::gpio::GpioBorrowChecker;
use crate::bus::BusController;

pub struct UARTDefinition {
    path: String,
    rx: u8,
    tx: u8
}

impl UARTDefinition {
    pub fn new(path: &str, rx: u8, tx: u8) -> Self {
        UARTDefinition { path: path.to_string(), rx, tx }
    }

    pub fn overlap(&self, other: &Self) -> bool {
        self.path == other.path ||
        self.tx == other.tx ||
        self.rx == other.rx ||
        self.tx == other.rx ||
        self.rx == other.tx
    }

    pub fn to_vec(&self) -> Vec<u8> {
        vec![self.rx, self.tx]
    }

    pub fn to_arr(&self) -> [u8; 2] {
        [self.rx, self.tx]
    }
}

struct UartInfo {
    path: String,
    lease_id: Option<Uuid>
}

impl UartInfo {
    fn new(path: &str) -> Self {
        UartInfo { path: path.to_string(), lease_id: None }
    }

    fn with_lease(path: &str, lease_id: Uuid) -> Self {
        UartInfo { path: path.to_string(), lease_id: Some(lease_id) }
    }
}

#[derive(Debug, PartialEq)]
pub enum UARTError {
    InvalidConfig(String),
    PortNotFound,
    LeaseNotFound,
    Busy,
    HardwareError(String),
    NotSupported,
    Other(String)
}

impl Display for UARTError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            UARTError::InvalidConfig(msg) => format!("invalid config: {}", msg),
            UARTError::PortNotFound => format!("specified internal UART channel does not exist"),
            UARTError::LeaseNotFound => format!("specified internal UART channel is not open"),
            UARTError::Busy => format!("UART channel is busy"),
            UARTError::HardwareError(msg) => format!("hardware error: {}", msg),
            UARTError::NotSupported => format!("not supported"),
            UARTError::Other(msg) => format!("{}", msg),
        })
    }
}

pub struct UARTBusController {
    gpio_borrow: Arc<RwLock<GpioBorrowChecker>>,
    owned_ports: HashMap<String, UartInfo>,
    internal_ports: HashMap<u8, UARTDefinition>
}

impl BusController for UARTBusController {
    fn name(&self) -> String {
        "UART".to_string()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

fn rppal_map_err(err: Error, default_err_msg: &str) -> UARTError {
    match err {
        Error::Io(e) => UARTError::HardwareError(format!("I/O error: {}", e)),
        Error::Gpio(e) => UARTError::HardwareError(format!("GPIO error: {}", e)),
        Error::InvalidValue => UARTError::NotSupported,
    }
}

impl UARTBusController {
    pub fn new(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>) -> Self {
        UARTBusController { 
            gpio_borrow: gpio_borrow.clone(), 
            owned_ports: HashMap::new(), 
            internal_ports: HashMap::new() 
        }
    }

    pub fn with_internals(gpio_borrow: &Arc<RwLock<GpioBorrowChecker>>, internal_ports: HashMap<u8, UARTDefinition>) -> Result<Self, UARTError> {
        let gpio_checker = gpio_borrow.read();

        for (id, definition) in &internal_ports {
            if definition.rx == definition.tx {
                return Err(UARTError::InvalidConfig(
                    format!("UART port is attempting to use the same pin twice: port {} (at {}) -> (RX: {}. TX: {})",
                    id, definition.path, definition.rx, definition.tx
                )));
            }

            if !gpio_checker.has_pin(definition.rx) {
                return Err(UARTError::InvalidConfig(
                    format!("UART port is attempting to use invalid pin: port {} (at {}) pin {} (RX)",
                    id, definition.path, definition.rx
                )));
            }

            if !gpio_checker.has_pin(definition.tx) {
                return Err(UARTError::InvalidConfig(
                    format!("UART port is attempting to use invalid pin: port {} (at {}) pin {} (TX)",
                    id, definition.path, definition.tx
                )));
            }

            for (other_id, other_definition) in &internal_ports {
                if id != other_id && definition.overlap(other_definition) {
                    return Err(UARTError::InvalidConfig(
                        format!("UART port definitions overlap: port {} (at {}) -> (RX: {}, TX: {}) with port {} (at {}) -> (RX: {}, TX: {})",
                        id, definition.path, definition.rx, definition.tx, other_id, other_definition.path, other_definition.rx, other_definition.tx
                    )));
                }
            }
        }

        Ok(UARTBusController { 
            gpio_borrow: gpio_borrow.clone(), 
            internal_ports: internal_ports, 
            owned_ports: HashMap::new()
        })
    }

    pub fn open(&mut self, port: u8, baud_rate: u32, parity: Parity, data_bits: u8, stop_bits: u8) -> Result<Uart, UARTError> {
        let definition = match self.internal_ports.get(&port) {
            Some(definition) => definition,
            None => return Err(UARTError::PortNotFound)
        };

        if self.owned_ports.contains_key(&definition.path) {
            return Err(UARTError::Busy);
        }

        let mut borrow_checker = self.gpio_borrow.write();
        if !borrow_checker.can_borrow_many(&definition.to_arr()) {
            return Err(UARTError::Busy);
        }

        let uart = Uart::with_path(
            Path::new(&definition.path),
            baud_rate,
            parity,
            data_bits,
            stop_bits)
            .map_err(|err| rppal_map_err(err, &format!("Internal RPPAL error while opening UART port {} (at {})", port, definition.path)))?;

        let borrow_id = borrow_checker.borrow_many(definition.to_vec())
            .map_err(|err| UARTError::HardwareError(err.to_string()))?;

        let uart_info = UartInfo::with_lease(&definition.path, borrow_id);
        self.owned_ports.insert(definition.path.to_string(), uart_info);
        Ok(uart)
    }

    pub fn open_path(&mut self, path: String, baud_rate: u32, parity: Parity, data_bits: u8, stop_bits: u8) -> Result<Uart, UARTError> {
        if self.owned_ports.contains_key(&path) {
            return Err(UARTError::Busy);
        }

        let uart = Uart::with_path(
            Path::new(&path),
            baud_rate,
            parity,
            data_bits,
            stop_bits)
            .map_err(|err| rppal_map_err(err, &format!("Internal RPPAL error while opening UART device {}", path)))?;

        let uart_info = UartInfo::new(&path);
        self.owned_ports.insert(path, uart_info);
        Ok(uart)
    }

    pub fn close(&mut self, port: u8) -> Result<(), UARTError> {
        match self.internal_ports.get(&port) {
            Some(definition) => self.close_path(definition.path.to_string()),
            None => Err(UARTError::PortNotFound)
        }
    }

    pub fn close_path(&mut self, path: String) -> Result<(), UARTError> {
        let info = match self.owned_ports.get(&path) {
            Some(info) => info,
            None => return Err(UARTError::LeaseNotFound)
        };

        if info.lease_id.is_some() {
            // Internal port, needs to be released.
            let mut borrow_checker = self.gpio_borrow.write();
            borrow_checker.release(&info.lease_id.unwrap())
                .map_err(|err| UARTError::HardwareError(err.to_string()))?;    
        }
        
        self.owned_ports.remove(&path);
        Ok(())
    }
}