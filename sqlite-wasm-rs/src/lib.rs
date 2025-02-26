#![doc = include_str!("../README.md")]

pub(crate) mod fragile;
mod shim;

pub use shim::export;
