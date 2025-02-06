//! Wrap the Mutex and Rwlock lock.
//!
//! In a single thread, when atomics is not enabled, use the lock provided by the standard library.
//! There will be no deadlock unless there is a recursive call.
//!
//! In multithreading, when atomics is enabled, use parking_lot, it will not cause lock poisoning

#![allow(unused)]

#[cfg(target_feature = "atomics")]
use parking_lot::{Mutex as Mutex0, RwLock as RwLock0};

#[cfg(target_feature = "atomics")]
pub use parking_lot::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

#[cfg(not(target_feature = "atomics"))]
use std::sync::{Mutex as Mutex0, RwLock as RwLock0};

#[cfg(not(target_feature = "atomics"))]
pub use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

pub struct RwLock<T>(RwLock0<T>);

impl<T> RwLock<T> {
    pub fn new(t: T) -> Self {
        Self(RwLock0::new(t))
    }

    #[cfg(target_feature = "atomics")]
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.0.read()
    }

    #[cfg(not(target_feature = "atomics"))]
    pub fn read(&self) -> RwLockReadGuard<'_, T> {
        self.0.read().unwrap()
    }

    #[cfg(target_feature = "atomics")]
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.0.write()
    }

    #[cfg(not(target_feature = "atomics"))]
    pub fn write(&self) -> RwLockWriteGuard<'_, T> {
        self.0.write().unwrap()
    }
}

pub struct Mutex<T>(Mutex0<T>);

impl<T> Mutex<T> {
    pub fn new(t: T) -> Self {
        Self(Mutex0::new(t))
    }

    #[cfg(target_feature = "atomics")]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock()
    }

    #[cfg(not(target_feature = "atomics"))]
    pub fn lock(&self) -> MutexGuard<'_, T> {
        self.0.lock().unwrap()
    }
}
