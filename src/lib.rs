#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(
    all(feature = "wasm-bindgen", target_feature = "atomics"),
    feature(stdarch_wasm_atomic_wait)
)]
#![allow(clippy::missing_safety_doc)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate alloc;

pub mod host;
mod shim;
#[rustfmt::skip]
#[allow(clippy::type_complexity)]
mod bindings;

/// Types and utilities for implementing SQLite VFS.
#[doc(inline)]
pub use rsqlite_vfs as vfs;

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use bindings::*;

pub use host::WasmOsCallback;
