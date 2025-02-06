#![doc = include_str!("../README.md")]

pub(crate) mod fragile;
pub(crate) mod lock_api;

#[cfg(feature = "wrapper")]
mod wrapper;

#[cfg(feature = "polyfill")]
mod polyfill;

#[cfg(feature = "wrapper")]
pub use wrapper::export;

#[cfg(feature = "polyfill")]
pub use polyfill::export;
