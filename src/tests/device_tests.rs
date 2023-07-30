use std::any::{Any, TypeId};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::bus::BusController;
use crate::capabilities::Capability;
use crate::device::{Device, DeviceError, DeviceServer, DeviceServerBuilder};
use intertrait::cast_to;
use parking_lot::RwLock;
use uuid::Uuid;

struct StubController {}
impl BusController for StubController {
    fn name(&self) -> String {
        "STUB".to_string()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl StubController {
    fn new() -> Self {
        StubController {  }
    }

    fn do_thing(&self) -> String {
        "hello".to_string()
    }
}

struct FunController {
    fun_count: u32,
}

impl FunController {
    pub fn new() -> Self {
        Self { fun_count: 0 }    
    }

    pub fn increase_fun(&mut self) -> Option<u32> {
        if self.fun_count >= 10 {
            return None;
        }

        self.fun_count += 1;
        Some(self.fun_count)
    }

    pub fn get_fun_count(&self) -> u32 {
        self.fun_count
    }
}

impl BusController for FunController {
    fn name(&self) -> String {
        "FUN".to_string()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

trait FunCapable: Capability {
    fn have_fun(&mut self) -> String;
    fn how_much_fun(&self) -> u32;
}

trait SleepCapable: Capability {
    fn sleep(&mut self) -> String;
    fn wake_up(&mut self) -> String;
}

struct NoCapDevice {
    address: Option<Uuid>
}
struct FunDevice {
    address: Option<Uuid>,
    fun_controller: Option<Arc<RwLock<FunController>>>
}
struct SleepyDevice {
    address: Option<Uuid>,
    is_resting: bool
}

impl Device for NoCapDevice {
    fn load(&mut self, _parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError> {
        self.address = Some(address);
        Ok(())
    }

    fn unload(&mut self, _parent: &mut DeviceServer) -> Result<(), DeviceError> {
        self.address = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl NoCapDevice {
    fn new() -> Self {
        NoCapDevice {
            address: None
        }
    }
}

impl Device for FunDevice {
    fn load(
        &mut self, parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError> {
        self.address = Some(address);
        self.fun_controller = match parent.get_bus_ptr() {
            Some(c) => Some(c),
            None => return Err(DeviceError::MissingController("FUN".to_string()))
        };
        Ok(())
    }

    fn unload(&mut self, _parent: &mut DeviceServer) -> Result<(), DeviceError> {
        self.address = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cast_to]
impl FunCapable for FunDevice {
    fn have_fun(&mut self) -> String {
        let mut controller = self.fun_controller.as_ref().expect("device not initialized").write();

        let fun_count = match controller.increase_fun() {
            Some(c) => c,
            None => return "had too much fun!".to_string(),
        };

        if fun_count < 3 {
            "slightly fun".to_string()
        } else if fun_count >= 3 && fun_count < 7 {
            "pretty fun".to_string()
        } else if fun_count >= 7 && fun_count <= 10 {
            "very fun".to_string()
        } else {
            panic!("Invalid fun_count");
        }
    }

    fn how_much_fun(&self) -> u32 {
        let controller = self.fun_controller.as_ref().expect("device not initialized").read();
        controller.get_fun_count()
    }
}
impl Capability for FunDevice {}
impl FunDevice {
    fn new() -> Self {
        FunDevice {
            address: None,
            fun_controller: None
        }
    }
}

impl Device for SleepyDevice {
    fn load(
        &mut self, _parent: &mut DeviceServer, address: Uuid) -> Result<(), DeviceError> {
        self.address = Some(address);
        Ok(())
    }

    fn unload(&mut self, _parent: &mut DeviceServer) -> Result<(), DeviceError> {
        self.address = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[cast_to]
impl SleepCapable for SleepyDevice {
    fn sleep(&mut self) -> String {
        if self.is_resting {
            return "I'm already asleep... zzz..".to_string();
        }

        self.is_resting = true;
        "Going to sleep... Zzz...".to_string()
    }

    fn wake_up(&mut self) -> String {
        if !self.is_resting {
            return "I'm not sleeping!".to_string();
        }

        self.is_resting = false;
        "Good morning".to_string()
    }
}
impl Capability for SleepyDevice {}
impl SleepyDevice {
    fn new() -> Self {
        SleepyDevice {
            address: None,
            is_resting: false
        }
    }
}

#[test]
fn ds_build_auto() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(FunDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    assert_eq!(server.get_buses().len(), 1);
    assert_eq!(server.get_devices().len(), 3);
}

#[test]
fn ds_build_manual() {
    let mut server = DeviceServer::new();
    assert_eq!(server.get_buses().len(), 0);
    assert_eq!(server.get_devices().len(), 0);

    server.register_bus(Arc::new(RwLock::new(FunController::new()))).expect("failed to register bus");
    assert_eq!(server.get_buses().len(), 1);

    server.register_bus(Arc::new(RwLock::new(FunController::new()))).expect_err("duplicate bus check failed");
    assert_eq!(server.get_buses().len(), 1);

    let id = server.register_device(Box::new(NoCapDevice::new())).expect("failed to add NoCapDevice");
    assert_eq!(server.get_devices().len(), 1);

    server.register_device(Box::new(FunDevice::new())).expect("failed to add FunDevice");
    assert_eq!(server.get_devices().len(), 2);

    server.remove_device(&id).expect("failed to remove NoCapDevice");
    assert_eq!(server.get_devices().len(), 1);
}

#[test]
fn ds_has_bus() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(FunDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    assert!(server.has_bus::<FunController>());
    assert!(!server.has_bus::<StubController>());
}

#[test]
fn ds_has_device() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(FunDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let device_ids = server.get_devices().iter().map(|(k,_)| *k).collect::<Vec<&Uuid>>();
    assert_eq!(device_ids.len(), 3);
    
    for id in device_ids {
        assert!(server.has_device(id));
    }
}

#[test]
fn ds_get_device() {
    let mut server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(FunDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let id = server.register_device(Box::new(NoCapDevice::new())).expect("failed to register device");
    let device = server.get_device(&id);
    assert!(device.is_some());
    let device = device.unwrap();
    assert_eq!(device.as_ref().type_id(), TypeId::of::<NoCapDevice>());
}

#[test]
fn device_has_capability() {
    let mut server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let id = server.register_device(Box::new(SleepyDevice::new())).expect("failed to register device");
    let device = server.get_device(&id).expect("failed to get device");
    assert!(device.has_capability::<dyn SleepCapable>());
    assert!(!device.has_capability::<dyn FunCapable>());
}

#[test]
fn device_as_capability() {
    let mut server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let id = server.register_device(Box::new(SleepyDevice::new())).expect("failed to register device");
    let device = server.get_device(&id).expect("failed to get device");
    device.as_capability_ref::<dyn SleepCapable>().expect("failed to cast device");
}

#[test]
fn device_as_capability_mut() {
    let mut server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let id = server.register_device(Box::new(SleepyDevice::new())).expect("failed to register device");
    let device = server.get_device_mut(&id).expect("failed to get device");
    let sleepy = device.as_capability_mut::<dyn SleepCapable>().expect("failed to cast device");

    // go to sleep
    assert_eq!(sleepy.sleep(), "Going to sleep... Zzz...");
    assert_eq!(sleepy.sleep(), "I'm already asleep... zzz..");

    // and try to wake up
    assert_eq!(sleepy.wake_up(), "Good morning");
    assert_eq!(sleepy.wake_up(), "I'm not sleeping!");
}

#[test]
fn device_bus_access() {
    let mut server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let id = server.register_device(Box::new(FunDevice::new())).expect("failed to register device");
    let device = server.get_device_mut(&id).expect("failed to get device");
    let fun = device.as_capability_mut::<dyn FunCapable>().expect("failed to cast device");

    for i in 0..10 {
        let fun_status = fun.have_fun();
        assert_eq!(match i {
            0 => fun_status == "slightly fun",
            3 => fun_status == "pretty fun",
            7 => fun_status == "very fun",
            _ => true
        }, true);
    }
}

#[test]
fn ds_bus_ptr_safety_check() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    assert!(server.get_bus_ptr::<StubController>().is_none());
    assert!(server.get_bus_ptr::<FunController>().is_some());
}

#[test]
fn ds_bus_ptr_ref_eq() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_bus(StubController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let stub1 = server.get_bus_ptr::<StubController>().expect("failed to get stub ptr");
    let stub2 = server.get_bus_ptr::<StubController>().expect("failed to get stub ptr");
    assert_eq!(Arc::strong_count(&stub1), Arc::strong_count(&stub2));
    assert!(Arc::ptr_eq(&stub1, &stub2));

    let fun1 = server.get_bus_ptr::<FunController>().expect("failed to get fun ptr");
    let fun2 = server.get_bus_ptr::<FunController>().expect("failed to get fun ptr");
    assert_eq!(Arc::strong_count(&fun1), Arc::strong_count(&fun2));
    assert!(Arc::ptr_eq(&fun1, &fun2));

    let stub1_ptr = Arc::into_raw(stub1) as *const RwLock<dyn BusController>;
    let stub2_ptr = Arc::into_raw(stub2) as *const RwLock<dyn BusController>;
    let fun1_ptr = Arc::into_raw(fun1) as *const RwLock<dyn BusController>;
    let fun2_ptr = Arc::into_raw(fun2) as *const RwLock<dyn BusController>;

    assert_eq!(stub1_ptr, stub2_ptr);
    assert_eq!(fun1_ptr, fun2_ptr);
    assert_ne!(stub1_ptr, fun1_ptr);
    assert_ne!(stub2_ptr, fun2_ptr);

    // prevent memory leak
    unsafe {
        Arc::from_raw(stub1_ptr as *const RefCell<StubController>);
        Arc::from_raw(stub2_ptr as *const RefCell<StubController>);
        Arc::from_raw(fun1_ptr as *const RefCell<FunController>);
        Arc::from_raw(fun2_ptr as *const RefCell<FunController>);
    }
}

#[test]
fn ds_bus_ptr_access() {
    let server = DeviceServerBuilder::configure()
        .add_bus(FunController::new())
        .add_bus(StubController::new())
        .add_device(NoCapDevice::new())
        .add_device(SleepyDevice::new())
        .build().expect("failed to build server");

    let stub = server.get_bus_ptr::<StubController>().expect("failed to get stub ptr");
    let fun = server.get_bus_ptr::<FunController>().expect("failed to get fun ptr");

    // test stub
    assert_eq!(stub.read().name(), "STUB");
    assert_eq!(stub.read().do_thing(), "hello");

    // test fun
    assert_eq!(fun.read().name(), "FUN");
    assert_eq!(fun.read().get_fun_count(), 0);
    assert_eq!(fun.write().increase_fun().unwrap(), 1);
    assert_eq!(fun.read().get_fun_count(), 1);
}