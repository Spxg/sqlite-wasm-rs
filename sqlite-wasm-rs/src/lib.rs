#![doc = include_str!("../README.md")]

#[cfg(feature = "wrapper")]
mod wrapper;

#[cfg(feature = "link")]
mod link;

#[cfg(feature = "wrapper")]
pub use wrapper::export;

#[cfg(feature = "link")]
pub use link::export;
