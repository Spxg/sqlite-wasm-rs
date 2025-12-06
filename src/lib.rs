#![doc = include_str!("../README.md")]
#![cfg_attr(
    target_feature = "atomics",
    feature(thread_local, stdarch_wasm_atomic_wait)
)]

#[rustfmt::skip]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::type_complexity)]
mod libsqlite3;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod shim;

/// Virtual File System implementations for different browser storage backends.
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use libsqlite3::*;

/// In-memory VFS implementation.
pub use vfs::memory as mem_vfs;

/// Origin Private File System (OPFS) VFS implementation using `SyncAccessHandle`.
pub use vfs::sahpool as sahpool_vfs;

/// IndexedDB VFS implementation with relaxed durability guarantees.
#[cfg(feature = "relaxed-idb")]
pub use vfs::relaxed_idb as relaxed_idb_vfs;

/// Utility functions and types to help with creating custom VFS implementations.
pub use vfs::utils;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);
