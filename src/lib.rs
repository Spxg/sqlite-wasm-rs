#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(target_feature = "atomics", feature(stdarch_wasm_atomic_wait))]
#![allow(clippy::missing_safety_doc)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate alloc;

#[rustfmt::skip]
#[allow(clippy::type_complexity)]
mod bindings;
mod memvfs;
mod shim;

/// Low-level utilities, traits, and macros for implementing custom SQLite Virtual File Systems (VFS)
pub mod utils;

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use bindings::*;

/// In-memory VFS implementation.
pub use memvfs::{MemVfsError, MemVfsUtil};
