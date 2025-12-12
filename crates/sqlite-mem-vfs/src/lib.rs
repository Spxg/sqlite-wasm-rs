#![no_std]

extern crate alloc;

mod memvfs;

#[doc(inline)]
pub use memvfs::{install, uninstall, MemVfsError, MemVfsUtil, OsCallback};
