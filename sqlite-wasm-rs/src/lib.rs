#![doc = include_str!("../README.md")]

pub(crate) mod fragile;
pub(crate) mod locker;

mod shim;

pub use shim::export;
