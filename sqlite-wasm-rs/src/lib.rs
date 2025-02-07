#![doc = include_str!("../README.md")]

pub(crate) mod fragile;
pub(crate) mod locker;

#[cfg(feature = "wrapper")]
mod wrapper;

#[cfg(feature = "shim")]
mod shim;

#[cfg(feature = "wrapper")]
pub use wrapper::export;

#[cfg(feature = "shim")]
pub use shim::export;
