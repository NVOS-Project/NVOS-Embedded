use intertrait::CastFrom;
use intertrait::cast::{CastRef, CastMut};
use uuid::Uuid;
use crate::bus::BusController;
use crate::capabilities::Capability;
use std::any::{Any, TypeId};
use std::cell::{RefCell, Ref, RefMut};
use std::collections::HashMap;
use std::fmt::Display;
use std::rc::{Rc, Weak};
use unbox_box::BoxExt;
pub trait Device : CastFrom  {
    fn load(&mut self, parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError>;
    fn unload(&mut self, parent: &mut DeviceServer) -> Result<(), DeviceError>;
    fn as_any(&self) -> &dyn Any;
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
    bus_controllers: Vec<Rc<RefCell<dyn BusController>>>,
    devices: HashMap<Uuid, DeviceBox>
}

pub struct DeviceServerBuilder {
    bus_controllers: Vec<Rc<RefCell<dyn BusController>>>,
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
        self.bus_controllers.push(Rc::new(RefCell::new(bus)));
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

    pub fn register_bus(&mut self, bus: Rc<RefCell<dyn BusController>>) -> Result<(), DeviceError> {
        for controller in &self.bus_controllers {
            if bus.borrow().as_any().type_id() == controller.borrow().as_any().type_id() {
                return Err(DeviceError::DuplicateController);
            }
        }
        
        self.bus_controllers.push(bus);
        Ok(())
    }

    pub fn get_bus<T: BusController>(&self) -> Option<Ref<'_, T>> {
        for controller in &self.bus_controllers {
            let r = controller.borrow();
            if (*r).as_any().is::<T>() {
                return Some(Ref::map(r, |x| x.as_any().downcast_ref::<T>().unwrap()));
            }
        }

        None
    }

    pub fn get_bus_mut<T: BusController>(&self) -> Option<RefMut<'_, T>> {
        for controller in &self.bus_controllers {
            let r = controller.borrow_mut();
            if (*r).as_any().is::<T>() {
                return Some(RefMut::map(r, |x| x.as_any_mut().downcast_mut::<T>().unwrap()));
            }
        }

        None
    }

    pub fn get_bus_ptr<T: BusController + 'static>(&self) -> Option<Rc<RefCell<T>>> {
        for controller in &self.bus_controllers {
            let _sanity_check = (*controller.borrow()).as_any().is::<T>();
            if _sanity_check {
                let rc = Rc::clone(controller);
                unsafe {
                    let rc_cast = Rc::from_raw(Rc::into_raw(rc) as *const RefCell<T>);
                    return Some(rc_cast);
                }
            }
        }

        None
    }

    pub fn get_buses(&self) -> Vec<Ref<'_, dyn BusController>> {
        self.bus_controllers.iter().map(|c| c.borrow()).collect()
    }

    pub fn has_bus<T: BusController>(&self) -> bool {
        for controller in &self.bus_controllers {
            if controller.borrow().as_any().is::<T>() {
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