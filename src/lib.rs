#![doc = include_str!("../README.md")]
#![no_std]
#![cfg_attr(target_feature = "atomics", feature(stdarch_wasm_atomic_wait))]
#![allow(clippy::missing_safety_doc)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]

extern crate alloc;

mod shim;
#[rustfmt::skip]
#[allow(clippy::type_complexity)]
mod bindings;

/// Low-level utilities and traits for implementing custom SQLite Virtual File Systems (VFS)
pub mod utils {
    #[doc(inline)]
    pub use rsqlite_vfs::{
        AccessMode, DeviceCharacteristics, FileKind, ImportDbError, LockLevel, MemChunksFile,
        OpenAccess, OpenOptions, OpenRequest, OpenedFile, OsCallback, RawVfsErrorCode,
        RegisterVfsError, SQLITE3_HEADER, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, SectorSize,
        SyncMode, SyncOptions, SystemErrorCode, VfsAppData, VfsError, VfsErrorCode, VfsFile,
        VfsFilename, VfsRegistration, VfsResult, VfsStore, check_db_and_page_size, check_import_db,
        random_name, register_vfs, registered_vfs,
    };

    pub use rsqlite_vfs::ffi;

    #[doc(hidden)]
    pub use rsqlite_vfs::test_suite;
}

/// Raw C-style bindings to the underlying `libsqlite3` library.
pub use bindings::*;

/// Wasm platform implementation
pub use self::shim::WasmOsCallback;
/// In-memory VFS implementation.
pub use rsqlite_vfs::memvfs::{MemVfsError, MemVfsUtil};
