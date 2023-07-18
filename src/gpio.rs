use std::{collections::HashMap, rc::Rc, cell::RefCell, fmt::Display};
use uuid::Uuid;

pub struct PinState {
    pin_number: u8,
    bcm_id: u8,
    leased: bool
}

impl PinState {
    pub fn new(pin_number: u8, bcm_id: u8) -> Self {
        PinState {
            pin_number: pin_number,
            bcm_id: bcm_id,
            leased: false
        }
    }

    pub fn pin_id(&self) -> u8 {
        self.pin_number
    }

    pub fn bcm_id(&self) -> u8 {
        self.bcm_id
    }
}

#[derive(Debug, PartialEq)]
pub enum GpioError {
    Busy(u8),
    PinNotFound(u8),
    LeaseNotFound,
    PermissionDenied(String),
    Other(String)
}

impl Display for GpioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self {
            GpioError::Busy(p) => format!("pin {} is busy", p),
            GpioError::PinNotFound(p) => format!("pin {} is not available", p),
            GpioError::LeaseNotFound => format!("specified lease does not exist"),
            GpioError::PermissionDenied(s) => format!("permission denied: {}", s),
            GpioError::Other(s) => format!("{}", s),
        })
    }
}

pub struct GpioBorrowChecker {
    pins: HashMap<u8, PinState>,
    leases: HashMap<Uuid, Vec<u8>>
}

impl GpioBorrowChecker {
    pub fn new(pins: HashMap<u8, PinState>) -> Self {
        GpioBorrowChecker { 
            pins: pins,
            leases: HashMap::new()
        }
    }

    pub fn new_rc(pins: HashMap<u8, PinState>) -> Rc<RefCell<Self>> {
        Rc::new(RefCell::new(GpioBorrowChecker::new(pins)))
    }

    pub fn get(&self, pin: &u8) -> Result<&PinState, GpioError> {
        match self.pins.contains_key(pin) {
            true => Ok(self.pins.get(pin).unwrap()),
            false => Err(GpioError::PinNotFound(pin.to_owned()))
        }
    }

    pub fn get_pins(&self) -> Vec<&PinState> {
        self.pins.values().collect()
    }

    pub fn get_borrowed(&self) -> Vec<&PinState> {
        self.pins.values().filter(|x| x.leased).collect()
    }

    pub fn borrow_one(&mut self, pin: u8) -> Result<Uuid, GpioError> {
        self.borrow_many(vec![pin])
    }

    pub fn has_pin(&self, pin: u8) -> bool {
        self.pins.contains_key(&pin)
    }

    pub fn has_lease(&self, borrow_id: &Uuid) -> bool {
        self.leases.contains_key(borrow_id)
    }

    pub fn can_borrow_one(&self, pin: u8) -> bool {
        match self.pins.contains_key(&pin) {
            true => !self.pins.get(&pin).unwrap().leased,
            false => false
        }
    }

    pub fn can_borrow_many(&self, pins: &[u8]) -> bool {
        for pin in pins {
            if !self.can_borrow_one(*pin) {
                return false;
            }
        }

        true
    }

    pub fn borrow_many(&mut self, pins: Vec<u8>) -> Result<Uuid, GpioError> {
        for pin in pins.iter() {
            if !self.pins.contains_key(&pin) {
                return Err(GpioError::PinNotFound(pin.to_owned()));
            }

            if self.pins.get(&pin).unwrap().leased {
                return Err(GpioError::Busy(pin.to_owned()));
            }
        }

        for pin in pins.iter() {
            let mut pin_state = self.pins.get_mut(&pin).unwrap();
            pin_state.leased = true;
        }

        let uuid = Uuid::new_v4();
        self.leases.insert(uuid, pins);
        Ok(uuid)
    }

    pub fn release(&mut self, borrow_id: &Uuid) -> Result<(), GpioError> {
        if !self.leases.contains_key(borrow_id) {
            return Err(GpioError::LeaseNotFound);
        }

        let lease = self.leases.get(borrow_id).unwrap();
        for pin in lease {
            let mut pin_state = self.pins.get_mut(&pin).unwrap();
            pin_state.leased = false;
        }

        self.leases.remove(borrow_id);
        Ok(())
    }
}