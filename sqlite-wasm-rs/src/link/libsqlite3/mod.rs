//! This module is copied from libsqlite-sys for compatibility
//! with various ORM libraries, such as `diesel`

mod bindings;
mod error;

pub use bindings::*;
pub use error::*;

use std::mem;

#[must_use]
pub fn SQLITE_STATIC() -> sqlite3_destructor_type {
    None
}

#[must_use]
pub fn SQLITE_TRANSIENT() -> sqlite3_destructor_type {
    Some(unsafe { mem::transmute::<isize, unsafe extern "C" fn(*mut std::ffi::c_void)>(-1_isize) })
}
