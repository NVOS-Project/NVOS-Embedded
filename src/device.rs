use uuid::Uuid;
use crate::bus::BusController;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Display;
use unbox_box::BoxExt;

#[derive(Debug, PartialEq)]
pub enum DeviceError {
    NotFound(Uuid),
    MissingController(String),
    DuplicateController,
    HardwareError,
    Other(String)
}

impl Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            DeviceError::NotFound(id) => format!("device with address {} is not registered", id),
            DeviceError::MissingController(name) => format!("bus controller \"{}\" was unavailable", name),
            DeviceError::DuplicateController => format!("controller of the same type is already registered"),
            DeviceError::HardwareError => format!("a hardware error has occurred"),
            DeviceError::Other(desc) => format!("an unknown error has occurred: {}", desc)
        })
    }
}

pub trait Device : Any  {
    fn load(&self, parent: &DeviceServer) -> Result<(), DeviceError>;
    fn unload(&self) -> Result<(), DeviceError>;
    fn as_any(&self) -> &dyn Any;
}

pub struct DeviceServer {
    bus_controllers: Vec<Box<dyn BusController>>,
    devices: HashMap<Uuid, Box<dyn Device>>
}

pub struct DeviceServerBuilder {
    bus_controllers: Vec<Box<dyn BusController>>,
    devices: Vec<Box<dyn Device>>
}

impl DeviceServerBuilder {
    pub fn configure() -> Self {
        DeviceServerBuilder { 
            bus_controllers: Vec::new(),
            devices: Vec::new()
        }
    }

    pub fn add_device<T: Device>(mut self, device: T) -> Self {
        self.devices.push(Box::new(device));
        self
    }

    pub fn add_bus<T: BusController>(mut self, bus: T) -> Self {
        self.bus_controllers.push(Box::new(bus));
        self
    }

    pub fn build(mut self) -> Result<DeviceServer, DeviceError> {
        let mut s = DeviceServer::new();
        while let Some(bus) = self.bus_controllers.pop() {
            s.register_bus(bus)?;
        }

        while let Some(device) = self.devices.pop() {
            s.register_device(device)?;
        }

        Ok(s)
    }
}

impl DeviceServer {
    pub fn new() -> Self {
        DeviceServer { 
            bus_controllers: Vec::new(),
            devices: HashMap::new()
        }
    }

    pub fn register_device(&mut self, device: Box<dyn Device>) -> Result<Uuid, DeviceError> {
        device.load(self)?;
        let id = Uuid::new_v4();
        self.devices.insert(id, device);
        Ok(id)
    }

    pub fn remove_device(&mut self, device_id: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(device_id) {
            return Err(DeviceError::NotFound(device_id.to_owned()));
        }

        let device = self.devices.get(device_id).unwrap();
        device.unload()?;
        self.devices.remove(device_id);
        Ok(())
    }

    pub fn register_bus(&mut self, bus: Box<dyn BusController>) -> Result<(), DeviceError> {
        for controller in &self.bus_controllers {
            if bus.as_any().type_id() == controller.as_any().type_id() {
                return Err(DeviceError::DuplicateController);
            }
        }

        self.bus_controllers.push(bus);
        Ok(())
    }

    pub fn get_bus<T: BusController>(&self) -> Option<&T> {
        for controller in &self.bus_controllers {
            if let Some(controller) = controller.as_any().downcast_ref::<T>() {
                return Some(controller);
            }
        }

        None
    }

    pub fn has_bus<T: BusController>(&self) -> bool {
        self.get_bus::<T>().is_some()
    }

    pub fn get_device(&self, address: &Uuid) -> Option<&dyn Device> {
        for (id, device) in &self.devices {
            if id == address {
                return Some(device.unbox_ref());
            }
        }

        None
    }

    pub fn has_device(&self, address: &Uuid) -> bool {
        self.get_device(address).is_some()
    }
}