#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(target_feature = "atomics", feature(stdarch_wasm_atomic_wait))]

extern crate alloc;

pub use rsqlite_vfs::memvfs;
pub use wsqlite3_sys as bindings;
mod shim;

/// Low-level utilities, traits, and macros for implementing custom SQLite Virtual File Systems (VFS)
pub mod utils {
    #[doc(inline)]
    pub use rsqlite_vfs::{
        bail, check_db_and_page_size, check_import_db, check_option, check_result, random_name,
        register_vfs, registered_vfs, ImportDbError, MemChunksFile, RegisterVfsError,
        SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult,
        VfsStore, SQLITE3_HEADER,
    };

    #[doc(hidden)]
    pub use rsqlite_vfs::test_suite;
}

#[doc(inline)]
pub use self::utils::{bail, check_option, check_result};

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use bindings::*;

pub use self::shim::WasmOsCallback;
/// In-memory VFS implementation.
pub use memvfs::{MemVfsError, MemVfsUtil};
