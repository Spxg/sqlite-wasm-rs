#![doc = include_str!("../README.md")]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

#[rustfmt::skip]
#[allow(clippy::type_complexity)]
mod libsqlite3;

mod shim;

/// Virtual File System implementations for different browser storage backends.
mod vfs;

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use libsqlite3::*;

/// In-memory VFS implementation.
pub use vfs::memory as mem_vfs;

/// Utility functions and types to help with creating custom VFS implementations.
pub use vfs::utils;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);
