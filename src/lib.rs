#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(target_feature = "atomics", feature(stdarch_wasm_atomic_wait))]
#![allow(clippy::missing_safety_doc)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate alloc;

pub use sqlite_mem_vfs as memvfs;
pub use sqlite_wasm_rs_sys as bindings;
mod shim;

/// Low-level utilities, traits, and macros for implementing custom SQLite Virtual File Systems (VFS)
pub use sqlite_vfs as utils;

pub use self::utils::{bail, check_option, check_result};

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use bindings::*;

pub use self::shim::WasmOsCallback;
/// In-memory VFS implementation.
pub use memvfs::{MemVfsError, MemVfsUtil};
