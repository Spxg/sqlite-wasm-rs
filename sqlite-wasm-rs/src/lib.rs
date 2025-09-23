#![doc = include_str!("../README.md")]

#[rustfmt::skip]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::type_complexity)]
mod libsqlite3;

#[cfg(not(feature = "custom-libc"))]
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod shim;

/// vfs implementation
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

// sqlite3 bindings
pub use libsqlite3::*;

// mem vfs implementation
pub use vfs::memory as mem_vfs;

// opfs sync access handle vfs implementation
pub use vfs::sahpool as sahpool_vfs;

// relaxed idb vfs implementation
#[cfg(feature = "relaxed-idb")]
pub use vfs::relaxed_idb as relaxed_idb_vfs;

// some tools for implementing VFS
pub use vfs::utils;

// `pub use` to avoid optimization
#[cfg(feature = "custom-libc")]
pub use sqlite_wasm_libc;

#[cfg(test)]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_dedicated_worker);
