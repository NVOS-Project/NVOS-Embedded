use crate::gpio::{GpioBorrowChecker, GpioError, PinState};
use std::collections::HashMap;

#[test]
fn has_pin_test() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let gpio = GpioBorrowChecker::new(pin_map);

    // non-existent pins
    assert!(!gpio.has_pin(1));
    assert!(!gpio.has_pin(16));

    // test multiple times
    assert!(gpio.has_pin(2));
    assert!(gpio.has_pin(2));

    // test existing pins
    assert!(gpio.has_pin(3));
    assert!(gpio.has_pin(6));
}

#[test]
fn borrow_many() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert!(gpio.borrow_many(vec![2, 3]).is_ok());
    assert!(gpio.borrow_many(vec![4, 5]).is_ok());
    assert!(gpio.borrow_many(vec![6]).is_ok());
}

#[test]
fn notfound_borrow_many() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert_eq!(
        gpio.borrow_many(vec![3, 4, 7]),
        Err(GpioError::PinNotFound(7))
    );
    assert_eq!(gpio.borrow_many(vec![2, 1]), Err(GpioError::PinNotFound(1)));
}

#[test]
fn busy_borrow_many() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert!(gpio.borrow_many(vec![2, 3, 4, 6]).is_ok());
    assert_eq!(gpio.borrow_many(vec![3, 5]), Err(GpioError::Busy(3)));
}

#[test]
fn borrow_many_and_release() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    let lease1 = gpio.borrow_many(vec![2, 3, 4]).unwrap();
    let lease2 = gpio.borrow_many(vec![5, 6]).unwrap();

    assert_eq!(gpio.release(&lease1), Ok(()));
    assert!(!gpio.has_lease(&lease1));
    assert!(gpio.has_lease(&lease2));
    assert_eq!(gpio.release(&lease2), Ok(()));
    assert!(!gpio.has_lease(&lease2));
}

#[test]
fn borrow_one() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert!(gpio.borrow_one(2).is_ok());
}

#[test]
fn notfound_borrow_one() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert_eq!(gpio.borrow_one(1), Err(GpioError::PinNotFound(1)));
}

#[test]
fn busy_borrow_one() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    assert!(gpio.borrow_one(2).is_ok());
    assert_eq!(gpio.borrow_one(2), Err(GpioError::Busy(2)));
}

#[test]
fn borrow_one_and_release() {
    let mut pin_map = HashMap::new();
    pin_map.insert(2, PinState::new(2, 12));
    pin_map.insert(3, PinState::new(3, 13));
    pin_map.insert(4, PinState::new(4, 14));
    pin_map.insert(5, PinState::new(5, 15));
    pin_map.insert(6, PinState::new(6, 16));
    let mut gpio = GpioBorrowChecker::new(pin_map);

    let r = gpio.borrow_one(2);
    assert!(r.is_ok());

    let r = r.unwrap();
    assert_eq!(gpio.release(&r), Ok(()));
}
