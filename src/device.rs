use uuid::Uuid;
use crate::bus::BusController;
use crate::capabilities::Capability;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt::Display;
use std::rc::{Rc, Weak};
use unbox_box::BoxExt;
pub trait Device : Any  {
    fn load(&mut self, parent: Rc<RefCell<DeviceServer>>, address: Uuid) -> Result<(), DeviceError>;
    fn unload(&mut self) -> Result<(), DeviceError>;
    fn as_any_ref(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct DeviceBox {
    device: Box<dyn Device>
}

impl DeviceBox {
    pub fn new(device: Box<dyn Device>) -> Self {
        DeviceBox { device }
    }

    pub fn as_any(&self) -> &dyn Any {
        self.device.as_any_ref()
    }

    pub fn as_ref(&self) -> &dyn Device {
        self.device.unbox_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn Device {
        self.device.unbox_mut()
    }

    pub fn as_capability_ref<T: Capability + 'static>(&self) -> Option<&T> {
        let device = self.device.as_any_ref();
        device.downcast_ref::<T>()
    }

    pub fn as_capability_mut<T: Capability + 'static>(&mut self) -> Option<&mut T> {
        let device = self.device.as_any_mut();
        device.downcast_mut::<T>()
    }

    pub fn has_capability<T: Capability + 'static>(&self) -> bool {
        self.as_capability_ref::<T>().is_some()
    }
}

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
pub struct DeviceServer {
    bus_controllers: Vec<Box<dyn BusController>>,
    devices: HashMap<Uuid, DeviceBox>,
    self_ptr: Option<Weak<RefCell<Self>>>
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

    pub fn build(mut self) -> Result<Rc<RefCell<DeviceServer>>, DeviceError> {
        let server = DeviceServer::new();
        let mut s = server.borrow_mut();
        while let Some(bus) = self.bus_controllers.pop() {
            s.register_bus(bus)?;
        }

        while let Some(device) = self.devices.pop() {
            s.register_device(device)?;
        }

        drop(s);
        Ok(server)
    }
}

impl DeviceServer {
    pub fn new() -> Rc<RefCell<Self>> {
        let mut server = DeviceServer { 
            bus_controllers: Vec::new(),
            devices: HashMap::new(),
            self_ptr: None
        };

        let rc = Rc::new(RefCell::new(server));
        rc.borrow_mut().self_ptr = Some(Rc::downgrade(&rc));
        rc
    }

    pub fn register_device(&mut self, mut device: Box<dyn Device>) -> Result<Uuid, DeviceError> {
        let id = Uuid::new_v4();
        device.load(self.get_strong_ptr(), id)?;
        self.devices.insert(id, DeviceBox::new(device));
        Ok(id)
    }

    pub fn remove_device(&mut self, device_id: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(device_id) {
            return Err(DeviceError::NotFound(device_id.to_owned()));
        }

        let device = self.devices.get_mut(device_id).unwrap().as_mut();
        device.unload()?;
        self.devices.remove(device_id);
        Ok(())
    }

    pub fn register_bus(&mut self, bus: Box<dyn BusController>) -> Result<(), DeviceError> {
        for controller in &self.bus_controllers {
            if bus.as_any_ref().type_id() == controller.as_any_ref().type_id() {
                return Err(DeviceError::DuplicateController);
            }
        }

        self.bus_controllers.push(bus);
        Ok(())
    }

    pub fn get_bus<T: BusController>(&self) -> Option<&T> {
        for controller in &self.bus_controllers {
            if let Some(controller) = controller.as_any_ref().downcast_ref::<T>() {
                return Some(controller);
            }
        }

        None
    }

    pub fn get_bus_mut<T: BusController>(&mut self) -> Option<&mut T> {
        for controller in &mut self.bus_controllers {
            if let Some(controller) = controller.as_any_mut().downcast_mut::<T>() {
                return Some(controller);
            }
        }

        None
    }

    pub fn has_bus<T: BusController>(&self) -> bool {
        self.get_bus::<T>().is_some()
    }

    pub fn get_device(&self, address: &Uuid) -> Option<&DeviceBox> {
        for (id, device) in &self.devices {
            if id == address {
                return Some(device);
            }
        }

        None
    }

    pub fn get_device_mut(&mut self, address: &Uuid) -> Option<&mut DeviceBox> {
        for (id, device) in &mut self.devices {
            if id == address {
                return Some(device);
            }
        }

        None
    }

    pub fn has_device(&self, address: &Uuid) -> bool {
        self.get_device(address).is_some()
    }

    fn get_strong_ptr(&self) -> Rc<RefCell<Self>> {
        self.get_weak_ptr().upgrade().expect("object was disposed")
    }

    fn get_weak_ptr(&self) -> Weak<RefCell<Self>> {
        self.self_ptr.clone().expect("self pointer not set")
    }
}