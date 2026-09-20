//! Generated SQLite bindings and handwritten Rust compatibility helpers.

#[cfg(all(not(feature = "bindgen"), feature = "sqlite3mc"))]
mod sqlite3mc_bindgen;

#[cfg(all(not(feature = "bindgen"), not(feature = "sqlite3mc")))]
mod sqlite3_bindgen;

mod bindgen {
    #[cfg(feature = "bindgen")]
    include!(concat!(env!("OUT_DIR"), "/bindgen.rs"));

    #[cfg(all(not(feature = "bindgen"), feature = "sqlite3mc"))]
    pub use super::sqlite3mc_bindgen::*;

    #[cfg(all(not(feature = "bindgen"), not(feature = "sqlite3mc")))]
    pub use super::sqlite3_bindgen::*;
}

mod error;

pub use bindgen::*;
pub use error::*;

use core::mem;

/// Tells SQLite to borrow the supplied buffer without freeing it.
///
/// The caller must keep the buffer valid for as long as the receiving API requires.
#[must_use]
pub fn SQLITE_STATIC() -> sqlite3_destructor_type {
    None
}

/// Tells SQLite to copy the supplied buffer before the receiving call returns.
///
/// This is a SQLite sentinel, not a callable destructor.
#[must_use]
pub fn SQLITE_TRANSIENT() -> sqlite3_destructor_type {
    // SQLite uses -1 as a sentinel for "make your own copy".
    Some(unsafe {
        mem::transmute::<isize, unsafe extern "C" fn(*mut core::ffi::c_void)>(-1_isize)
    })
}

impl Default for sqlite3_vtab {
    fn default() -> Self {
        // SAFETY: All fields are integers or raw pointers, valid when zeroed.
        unsafe { mem::zeroed() }
    }
}

impl Default for sqlite3_vtab_cursor {
    fn default() -> Self {
        // SAFETY: The only field is a raw pointer, valid when zeroed.
        unsafe { mem::zeroed() }
    }
}
