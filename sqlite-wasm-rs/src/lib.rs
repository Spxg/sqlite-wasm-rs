#![doc = include_str!("../README.md")]

#[cfg(feature = "wrapper")]
mod wrapper;

#[cfg(feature = "polyfill")]
mod polyfill;

#[cfg(feature = "wrapper")]
pub use wrapper::export;

#[cfg(feature = "polyfill")]
pub use polyfill::export;
