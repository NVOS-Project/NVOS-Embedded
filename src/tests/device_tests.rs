use std::any::Any;
use std::cell::{Ref, RefCell, RefMut};
use std::rc::Rc;

use crate::bus::BusController;
use crate::capabilities::Capability;
use crate::device::{Device, DeviceError, DeviceServer, DeviceServerBuilder};
use uuid::Uuid;

struct FunController {
    fun_count: u32,
}

impl FunController {
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
    address: Option<Uuid>,
    parent: Option<Rc<RefCell<DeviceServer>>>,
}
struct FunDevice {
    address: Option<Uuid>,
    parent: Option<Rc<RefCell<DeviceServer>>>,
}
struct SleepyDevice {
    address: Option<Uuid>,
    parent: Option<Rc<RefCell<DeviceServer>>>,
    is_resting: bool,
}

impl Device for NoCapDevice {
    fn load(
        &mut self,
        parent: Rc<RefCell<DeviceServer>>,
        address: Uuid,
    ) -> Result<(), DeviceError> {
        self.address = Some(address);
        self.parent = Some(parent);
        Ok(())
    }

    fn unload(&mut self) -> Result<(), crate::device::DeviceError> {
        self.address = None;
        self.parent = None;
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
            address: None,
            parent: None,
        }
    }
}

impl Device for FunDevice {
    fn load(
        &mut self,
        parent: Rc<RefCell<DeviceServer>>,
        address: Uuid,
    ) -> Result<(), DeviceError> {
        self.address = Some(address);
        self.parent = Some(parent);
        Ok(())
    }

    fn unload(&mut self) -> Result<(), crate::device::DeviceError> {
        self.address = None;
        self.parent = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
impl FunCapable for FunDevice {
    fn have_fun(&mut self) -> String {
        let mut parent = self.get_parent_mut();
        let controller = match parent.get_bus_mut::<FunController>() {
            Some(b) => b,
            None => panic!("bus not found"),
        };

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
        let parent = self.get_parent();
        let controller = match parent.get_bus::<FunController>() {
            Some(b) => b,
            None => panic!("bus not found"),
        };

        controller.get_fun_count()
    }
}
impl Capability for FunDevice {}
impl FunDevice {
    fn new() -> Self {
        FunDevice {
            address: None,
            parent: None,
        }
    }

    fn get_parent(&self) -> Ref<'_, DeviceServer> {
        self.parent
            .as_ref()
            .expect("device not initialized")
            .borrow()
    }

    fn get_parent_mut(&mut self) -> RefMut<'_, DeviceServer> {
        self.parent
            .as_ref()
            .expect("device not initialized")
            .borrow_mut()
    }
}

impl Device for SleepyDevice {
    fn load(
        &mut self,
        parent: Rc<RefCell<DeviceServer>>,
        address: Uuid,
    ) -> Result<(), DeviceError> {
        self.address = Some(address);
        self.parent = Some(parent);
        Ok(())
    }

    fn unload(&mut self) -> Result<(), crate::device::DeviceError> {
        self.address = None;
        self.parent = None;
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}
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
            parent: None,
            is_resting: false,
        }
    }
}
