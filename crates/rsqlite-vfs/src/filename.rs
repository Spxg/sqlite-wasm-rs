use core::ffi::{c_char, CStr};

use crate::{ffi, OpenOptions};

/// Borrowed filename with URI metadata only when supplied by SQLite.
///
/// Default `xOpen` exposes metadata for main databases, journals and WAL files
/// (journal/WAL lookup needs SQLite 3.31.0+). Copy values retained after the call.
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

    /// Returns the filename without URI metadata.
    pub const fn path(self) -> &'a str {
        self.path
    }

    /// Returns the parameter value supplied by SQLite, without assuming UTF-8.
    ///
    /// This is not the original URI text. Returns `None` for a missing parameter
    /// or a plain path; a parameter without a value yields an empty C string.
    pub fn parameter(self, key: &CStr) -> Option<&'a CStr> {
        let filename = self.sqlite?;
        unsafe {
            let value = ffi::sqlite3_uri_parameter(filename, key.as_ptr());
            (!value.is_null()).then(|| CStr::from_ptr(value))
        }
    }

    /// Reads a parameter using [SQLite's boolean conversion](https://www.sqlite.org/c3ref/uri_boolean.html).
    ///
    /// Missing or unrecognized values, and plain paths, return `default`.
    pub fn boolean(self, key: &CStr, default: bool) -> bool {
        self.sqlite.map_or(default, |filename| unsafe {
            ffi::sqlite3_uri_boolean(filename, key.as_ptr(), i32::from(default)) != 0
        })
    }

    /// Reads a parameter using [SQLite's integer conversion](https://www.sqlite.org/c3ref/uri_boolean.html).
    ///
    /// Missing parameters and plain paths return `default`. Parsing follows
    /// SQLite's rules, not Rust's [`str::parse`].
    pub fn integer(self, key: &CStr, default: i64) -> i64 {
        self.sqlite.map_or(default, |filename| unsafe {
            ffi::sqlite3_uri_int64(filename, key.as_ptr(), default)
        })
    }
}

/// A named open or an anonymous temporary-file request.
///
/// For anonymous files, the backend chooses how to create and remove the resource.
#[derive(Clone, Copy, Debug)]
pub struct OpenRequest<'a> {
    /// Filename and optional URI metadata; `None` for an anonymous temporary file.
    pub filename: Option<VfsFilename<'a>>,
    /// Requested access, file kind and creation flags.
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
