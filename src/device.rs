use intertrait::CastFromSync;
use intertrait::cast::{CastRef, CastMut};
use log::warn;
use uuid::Uuid;
use crate::bus::BusController;
use crate::capabilities::{Capability, CapabilityId, get_device_capabilities};
use crate::config::DeviceConfig;
use std::any::Any;
use std::collections::HashMap;
use std::fmt::Display;
use std::sync::Arc;
use unbox_box::BoxExt;
use parking_lot::{RwLock, RwLockReadGuard, MappedRwLockReadGuard, RwLockWriteGuard, MappedRwLockWriteGuard};

fn assert_controller_locked(controller: &Arc<parking_lot::lock_api::RwLock<parking_lot::RawRwLock, dyn BusController>>) -> bool {
    if controller.is_locked_exclusive() {
        warn!("cannot access controller because it is borrowed mutably, all outstanding mutable references must be dropped first to prevent a deadlock");
        warn!("continuing to enumerate controllers, but this will yield the current controller invisible to the caller");
        return true;
    }
    
    return false;
}

pub trait DeviceDriver : CastFromSync  {
    fn name(&self) -> String;
    fn is_running(&self) -> bool;
    fn new(config: Option<&mut DeviceConfig>) -> Result<Self, DeviceError> where Self : Sized;
    fn start(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError>;
    fn stop(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

pub struct Device {
    address: Uuid,
    name: String,
    driver: Box<dyn DeviceDriver>,
    capabilities: Vec<CapabilityId>
}

impl Device {
    pub fn from_driver(address: Uuid, driver: Box<dyn DeviceDriver>, friendly_name: Option<String>) -> Result<Self, DeviceError> {
        if friendly_name.is_some_and(|x| x.is_empty()) {
            return Err(DeviceError::InvalidConfig("invalid device name".to_string()))
        }

        let name = friendly_name.unwrap_or(format!("{}-{}", driver.name(), address));
        let cap_data = get_device_capabilities(driver.unbox_ref());

        Ok(Device { 
            address: address, 
            name: name, 
            driver: driver,
            capabilities: cap_data
        })
    }

    pub fn from_config<T: DeviceDriver>(address: Uuid, config: &mut DeviceConfig) -> Result<Self, DeviceError> {
        let driver: Box<dyn DeviceDriver> = Box::new(T::new(Some(config))?) as Box<dyn DeviceDriver>;
        Self::from_driver(address, driver, config.friendly_name)
    }

    pub fn new<T: DeviceDriver>(address: Uuid, friendly_name: Option<String>) -> Result<Self, DeviceError> {
        let driver: Box<dyn DeviceDriver> = Box::new(T::new(None)?) as Box<dyn DeviceDriver>;
        Self::from_driver(address, driver, friendly_name)
    }

    pub fn address(&self) -> Uuid {
        self.address
    }

    pub fn device_name(&self) -> String {
        self.name
    }

    pub fn driver_name(&self) -> String {
        self.driver.name()
    }

    pub fn is_running(&self) -> bool {
        self.driver.is_running()
    }

    pub fn as_any(&self) -> &dyn Any {
        self.driver.as_any()
    }

    pub fn as_ref(&self) -> &dyn DeviceDriver {
        self.driver.unbox_ref()
    }

    pub fn as_mut(&mut self) -> &mut dyn DeviceDriver {
        self.driver.unbox_mut()
    }

    pub fn as_capability_ref<T: Capability + 'static + ?Sized>(&self) -> Option<&T> {
        let device = self.driver.as_ref();
        device.cast::<T>()
    }

    pub fn as_capability_mut<T: Capability + 'static + ?Sized>(&mut self) -> Option<&mut T> {
        let device = self.driver.as_mut();
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
    DuplicateDevice(String),
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
            DeviceError::DuplicateController => format!("bus controller of the same type is already registered"),
            DeviceError::DuplicateDevice(desc) => format!("duplicate device: {}", desc),
            DeviceError::HardwareError(desc) => format!("a hardware error has occurred: {}", desc),
            DeviceError::InvalidOperation(desc) => format!("invalid operation: {}", desc),
            DeviceError::InvalidConfig(desc) => format!("invalid config: {}", desc),
            DeviceError::Other(desc) => format!("an unknown error has occurred: {}", desc)
        })
    }
}
pub struct DeviceServer {
    bus_controllers: Vec<Arc<RwLock<dyn BusController>>>,
    devices: HashMap<Uuid, Device>
}

pub struct DeviceServerBuilder {
    bus_controllers: Vec<Arc<RwLock<dyn BusController>>>,
    devices: Vec<Device>
}

impl DeviceServerBuilder {
    pub fn configure() -> Self {
        DeviceServerBuilder { 
            bus_controllers: Vec::new(),
            devices: Vec::new()
        }
    }

    pub fn add_device(mut self, device: Device) -> Self {
        self.devices.push(device);
        self
    }

    pub fn add_bus<T: BusController>(mut self, bus: T) -> Self {
        self.bus_controllers.push(Arc::new(RwLock::new(bus)));
        self
    }

    pub fn build(mut self, start_devices: bool) -> Result<DeviceServer, DeviceError> {
        let mut server = DeviceServer::new();

        while let Some(bus) = self.bus_controllers.pop() {
            server.register_bus(bus)?;
        }

        while let Some(device) = self.devices.pop() {
            server.register_device(device, start_devices)?;
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

    pub fn register_device(&mut self, mut device: Device, start_device: bool) -> Result<(), DeviceError> {
        if self.devices.contains_key(&device.address) {
            return Err(DeviceError::DuplicateDevice(format!("device with address {} already registered", device.address)));
        }

        
        
        if start_device && !device.as_ref().is_running() {
            device.as_mut().start(self)?;    
        }

        self.devices.insert(device.address, device);
        Ok(())
    }

    pub fn remove_device(&mut self, address: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(address) {
            return Err(DeviceError::NotFound(address.to_owned()));
        }


        let mut device = self.devices.get_mut(address).unwrap().as_mut();
        if device.is_running() {
            device.stop(self)?;
        }

        self.devices.remove(address);
        Ok(())
    }

    pub fn start_device(&mut self, address: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(address) {
            return Err(DeviceError::NotFound(address.to_owned()));
        }

        let mut device = self.devices.get_mut(&address).unwrap().as_mut();
        if device.is_running() {
            return Err(DeviceError::InvalidOperation("device is already running".to_owned()));
        }

        device.start(self)
    }

    pub fn stop_device(&mut self, address: &Uuid) -> Result<(), DeviceError> {
        if !self.devices.contains_key(address) {
            return Err(DeviceError::NotFound(address.to_owned()));
        }

        let mut device = self.devices.get_mut(&address).unwrap().as_mut();
        if !device.is_running() {
            return Err(DeviceError::InvalidOperation("device is not currently running".to_owned()));
        }

        device.stop(self)
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
            if assert_controller_locked(controller) {
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
            if assert_controller_locked(controller) {
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
            if assert_controller_locked(controller) {
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

    pub fn get_device(&self, address: &Uuid) -> Option<&Device> {
        self.devices.get(address)
    }

    pub fn get_devices(&self) -> HashMap<&Uuid, &Device> {
        self.devices.iter().collect()
    }

    pub fn get_device_mut(&mut self, address: &Uuid) -> Option<&mut Device> {
        self.devices.get_mut(address)
    }

    pub fn has_device(&self, address: &Uuid) -> bool {
        self.devices.contains_key(address)
    }
}