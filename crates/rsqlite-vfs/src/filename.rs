use core::ffi::{CStr, c_char};

use crate::{OpenOptions, ffi};

/// A filename borrowed for a backend call. Only names supplied by SQLite
/// carry URI metadata; constructing this from a Rust string never fabricates
/// the special allocation expected by SQLite's URI functions.
/// The default `xOpen` exposes metadata for main databases, their rollback
/// journals and WAL files. Journal/WAL parameter lookup requires SQLite 3.31.0+.
/// Copy any name or parameter that the backend needs to retain after the call.
#[derive(Clone, Copy, Debug)]
pub struct VfsFilename<'a> {
    path: &'a str,
    // Preserve the original allocation's pointer: SQLite's URI helpers read
    // metadata before/after the pathname, outside a pathname-only CStr borrow.
    sqlite: Option<*const c_char>,
}

// SAFETY: SQLite owns immutable filename/URI bytes for the entire borrow. The
// private raw pointer only permits reads; `from_sqlite` ties it to that lifetime.
unsafe impl Send for VfsFilename<'_> {}
unsafe impl Sync for VfsFilename<'_> {}

impl<'a> VfsFilename<'a> {
    /// Wraps a plain path without parsing a URI or attaching URI metadata.
    pub const fn new(path: &'a str) -> Self {
        Self { path, sqlite: None }
    }

    // Caller guarantees the original SQLite filename allocation (not a copied
    // C string), including all metadata, remains live and immutable for 'a.
    // A present pointer must be non-null and valid for SQLite's URI helpers.
    pub(crate) unsafe fn from_sqlite(path: &'a str, sqlite: Option<*const c_char>) -> Self {
        Self { path, sqlite }
    }

    pub const fn path(self) -> &'a str {
        self.path
    }

    /// Returns the parameter value supplied by SQLite, without assuming UTF-8.
    /// This is not the original URI text. Returns `None` for a missing parameter
    /// or a plain path; a parameter without a value yields an empty C string.
    pub fn parameter(self, key: &CStr) -> Option<&'a CStr> {
        let filename = self.sqlite?;
        unsafe {
            let value = ffi::sqlite3_uri_parameter(filename, key.as_ptr());
            (!value.is_null()).then(|| CStr::from_ptr(value))
        }
    }

    /// Uses SQLite's boolean conversion. Missing or unrecognized values, and
    /// plain paths without URI metadata, return `default`.
    /// See <https://www.sqlite.org/c3ref/uri_boolean.html> for accepted values.
    pub fn boolean(self, key: &CStr, default: bool) -> bool {
        self.sqlite.map_or(default, |filename| unsafe {
            ffi::sqlite3_uri_boolean(filename, key.as_ptr(), i32::from(default)) != 0
        })
    }

    /// Uses SQLite's signed 64-bit integer conversion. Missing parameters and
    /// plain paths return `default`; non-integer values follow SQLite's parsing
    /// rules, not Rust's `str::parse`.
    /// See <https://www.sqlite.org/c3ref/uri_boolean.html>.
    pub fn integer(self, key: &CStr, default: i64) -> i64 {
        self.sqlite.map_or(default, |filename| unsafe {
            ffi::sqlite3_uri_int64(filename, key.as_ptr(), default)
        })
    }
}

/// A named open or an anonymous temporary-file request. A temporary request
/// has no filename; the backend chooses how to create and remove its resource.
#[derive(Clone, Copy, Debug)]
pub struct OpenRequest<'a> {
    pub filename: Option<VfsFilename<'a>>,
    pub options: OpenOptions,
}

impl<'a> OpenRequest<'a> {
    /// Requests a plain path without URI metadata, preserving `options` as given.
    pub const fn named(path: &'a str, options: OpenOptions) -> Self {
        Self {
            filename: Some(VfsFilename::new(path)),
            options,
        }
    }

    /// Requests an anonymous file without changing `options`. Direct callers
    /// must select creation and delete-on-close flags themselves when needed.
    pub const fn temporary(options: OpenOptions) -> Self {
        Self {
            filename: None,
            options,
        }
    }
}
