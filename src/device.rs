use intertrait::CastFromSync;
use intertrait::cast::{CastRef, CastMut};
use log::warn;
use uuid::Uuid;
use crate::bus::BusController;
use crate::capabilities::{Capability, CapabilityId, get_device_capabilities};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use unbox_box::BoxExt;
use parking_lot::{RwLock, RwLockReadGuard, MappedRwLockReadGuard, RwLockWriteGuard, MappedRwLockWriteGuard};

pub trait Device : CastFromSync  {
    fn name(&self) -> String;
    fn load(&mut self, parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError>;
    fn unload(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct DeviceBox {
    device: Box<dyn Device>,
    capabilities: Vec<CapabilityId>
}

impl DeviceBox {
    pub fn new(device: Box<dyn Device>) -> Self {
        let cap_data = get_device_capabilities(device.unbox_ref());
        DeviceBox { device: device, capabilities: cap_data }
    }

    pub fn as_any(&self) -> &dyn Any {
        self.device.as_any()
    }

    pub fn as_ref(&self) -> &dyn Device {
        self.device.unbox_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn Device {
        self.device.unbox_mut()
    }

    pub fn as_capability_ref<T: Capability + 'static + ?Sized>(&self) -> Option<&T> {
        let device = self.device.as_ref();
        device.cast::<T>()
    }

    pub fn as_capability_mut<T: Capability + 'static + ?Sized>(&mut self) -> Option<&mut T> {
        let device = self.device.as_mut();
        device.cast::<T>()
    }

    pub fn has_capability<T: Capability + 'static + ?Sized>(&self) -> bool {
        self.as_capability_ref::<T>().is_some()
    }

    pub fn get_capabilities(&self) -> Vec<CapabilityId> {
        self.capabilities.clone()
    }
}

#[derive(Debug, PartialEq)]
pub enum DeviceError {
    NotFound(Uuid),
    MissingController(String),
    DuplicateController,
    HardwareError(String),
    InvalidOperation(String),
    InvalidConfig(String),
    Other(String)
}

impl Display for DeviceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            DeviceError::NotFound(id) => format!("device with address {} is not registered", id),
            DeviceError::MissingController(name) => format!("bus controller \"{}\" was unavailable", name),
            DeviceError::DuplicateController => format!("controller of the same type is already registered"),
            DeviceError::HardwareError(desc) => format!("a hardware error has occurred: {}", desc),
            DeviceError::InvalidOperation(desc) => format!("invalid operation: {}", desc),
            DeviceError::InvalidConfig(desc) => format!("invalid config: {}", desc),
            DeviceError::Other(desc) => format!("an unknown error has occurred: {}", desc)
        })
    }
}
pub struct DeviceServer {
    bus_controllers: Vec<Arc<RwLock<dyn BusController>>>,
    devices: HashMap<Uuid, DeviceBox>
}

pub struct DeviceServerBuilder {
    bus_controllers: Vec<Arc<RwLock<dyn BusController>>>,
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
        self.bus_controllers.push(Arc::new(RwLock::new(bus)));
        self
    }

    pub fn build(mut self) -> Result<DeviceServer, DeviceError> {
        let mut server = DeviceServer::new();

        while let Some(bus) = self.bus_controllers.pop() {
            server.register_bus(bus)?;
        }

        while let Some(device) = self.devices.pop() {
            server.register_device(device)?;
        }

        Ok(server)
    }
}

impl DeviceServer {
    pub fn new() -> Self {
        DeviceServer { 
            bus_controllers: Vec::new(),
            devices: HashMap::new()
        }
    }

    pub fn register_device(&mut self, mut device: Box<dyn Device>) -> Result<Uuid, DeviceError> {
        let id = Uuid::new_v4();
        device.load(self, id)?;
        self.devices.insert(id, DeviceBox::new(device));
        Ok(id)
    }

    pub fn remove_device(&mut self, device_id: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(device_id) {
            return Err(DeviceError::NotFound(device_id.to_owned()));
        }

        let mut device = self.devices.remove(device_id).unwrap();
        match device.as_mut().unload(self) {
            Ok(_) => Ok(()),
            Err(e) => {
                self.devices.insert(device_id.to_owned(), device);
                Err(e)
            }
        }
    }

    pub fn register_bus(&mut self, bus: Arc<RwLock<dyn BusController>>) -> Result<(), DeviceError> {
        for controller in &self.bus_controllers {
            let t1 = bus.read().as_any().type_id();
            let t2 = controller.read().as_any().type_id();
            if t1 == t2 {
                return Err(DeviceError::DuplicateController);
            }
        }
        
        self.bus_controllers.push(bus);
        Ok(())
    }

    pub fn get_bus<T: BusController>(&self) -> Option<MappedRwLockReadGuard<'_, T>> {
        for controller in &self.bus_controllers {
            if controller.is_locked_exclusive() {
                warn!("cannot access controller because it is borrowed mutably, all outstanding mutable references must be dropped first to prevent a deadlock");
                warn!("continuing to enumerate controllers, but this will yield the current controller invisible to the caller");
                continue;
            }

            let r = controller.read();
            if (*r).as_any().is::<T>() {
                return Some(RwLockReadGuard::map(r, |x| x.as_any().downcast_ref::<T>().unwrap()));
            }
        }

        None
    }

    pub fn get_bus_mut<T: BusController>(&self) -> Option<MappedRwLockWriteGuard<'_, T>> {
        for controller in &self.bus_controllers {
            if controller.is_locked_exclusive() {
                warn!("cannot access controller because it is borrowed mutably, all outstanding mutable references must be dropped first to prevent a deadlock");
                warn!("continuing to enumerate controllers, but this will yield the current controller invisible to the caller");
                continue;
            }

            let r = controller.write();
            if (*r).as_any().is::<T>() {
                return Some(RwLockWriteGuard::map(r, |x| x.as_any_mut().downcast_mut::<T>().unwrap()));
            }
        }

        None
    }

    pub fn get_bus_ptr<T: BusController + 'static>(&self) -> Option<Arc<RwLock<T>>> {
        for controller in &self.bus_controllers {
            if controller.is_locked_exclusive() {
                warn!("cannot access controller because it is borrowed mutably, all outstanding mutable references must be dropped first to prevent a deadlock");
                warn!("continuing to enumerate controllers, but this will yield the current controller invisible to the caller");
                continue;
            }

            let _sanity_check = (*controller.read()).as_any().is::<T>();
            if _sanity_check {
                let arc = Arc::clone(controller);
                unsafe {
                    let arc_cast = Arc::from_raw(Arc::into_raw(arc) as *const RwLock<T>);
                    return Some(arc_cast);
                }
            }
        }

        None
    }

    pub fn get_buses(&self) -> Vec<RwLockReadGuard<'_, dyn BusController>> {
        self.bus_controllers.iter().map(|c| c.read()).collect()
    }

    pub fn has_bus<T: BusController>(&self) -> bool {
        for controller in &self.bus_controllers {
            if controller.read().as_any().is::<T>() {
                return true;
            }
        }

        return false;
    }

    pub fn get_device(&self, address: &Uuid) -> Option<&DeviceBox> {
        for (id, device) in &self.devices {
            if id == address {
                return Some(device);
            }
        }

        None
    }

    pub fn get_devices(&self) -> HashMap<&Uuid, &DeviceBox> {
        self.devices.iter().collect()
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
}