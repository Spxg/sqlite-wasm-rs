#![doc = include_str!("../README.md")]

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
#[allow(clippy::type_complexity)]
mod libsqlite3;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod shim;

#[allow(non_upper_case_globals)]
#[allow(non_camel_case_types)]
#[allow(non_snake_case)]
mod vfs;

// sqlite3 bindings
pub use libsqlite3::*;

// opfs vfs implementation
pub use vfs::sahpool::{
    install_opfs_sahpool, OpfsSAHError, OpfsSAHPoolCfg, OpfsSAHPoolCfgBuilder, OpfsSAHPoolUtil,
};

// `pub use` to avoid optimization
#[cfg(feature = "custom-libc")]
pub use sqlite_wasm_libc;

/// To be compatible with previous versions.
pub mod export {
    pub use super::*;
}
