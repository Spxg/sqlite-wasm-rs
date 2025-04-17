#![doc = include_str!("../README.md")]

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

/// vfs implementation
#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

// sqlite3 bindings
pub use libsqlite3::*;

// relaxed idb vfs implementation
pub use vfs::relaxed_idb as relaxed_idb_vfs;

// opfs sync access handle vfs implementation
pub use vfs::sahpool as sahpool_vfs;

// some tools for implementing VFS
pub use vfs::utils;

// `pub use` to avoid optimization
#[cfg(feature = "custom-libc")]
pub use sqlite_wasm_libc;

/// To be compatible with previous versions.
pub mod export {
    pub use crate::libsqlite3::*;
    pub use crate::vfs::sahpool::{
        install as install_opfs_sahpool, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder,
        OpfsSAHPoolUtil,
    };
}
