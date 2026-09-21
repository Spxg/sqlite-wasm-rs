use core::error;
use core::ffi::c_int;
use core::fmt;

/// Primary SQLite error categories.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorCode {
    /// Internal logic error in SQLite.
    InternalMalfunction,
    /// Access permission denied.
    PermissionDenied,
    /// Callback routine requested an abort.
    OperationAborted,
    /// The database file is locked.
    DatabaseBusy,
    /// A table in the database is locked.
    DatabaseLocked,
    /// Memory allocation failed.
    OutOfMemory,
    /// Attempt to write a read-only database.
    ReadOnly,
    /// Operation terminated by [`super::sqlite3_interrupt`].
    OperationInterrupted,
    /// A disk I/O error occurred.
    SystemIoFailure,
    /// The database disk image is malformed.
    DatabaseCorrupt,
    /// Unknown operation in [`super::sqlite3_file_control`].
    NotFound,
    /// Insertion failed because the database is full.
    DiskFull,
    /// Unable to open the database file.
    CannotOpen,
    /// Database lock protocol error.
    FileLockingProtocolFailed,
    /// The database schema changed.
    SchemaChanged,
    /// A string or BLOB exceeds the size limit.
    TooBig,
    /// Abort due to a constraint violation.
    ConstraintViolation,
    /// Data type mismatch.
    TypeMismatch,
    /// Library used incorrectly.
    ApiMisuse,
    /// Uses OS features not supported by the host.
    NoLargeFileSupport,
    /// Authorization denied.
    AuthorizationForStatementDenied,
    /// Parameter index for a `sqlite3_bind_*` function is out of range.
    ParameterOutOfRange,
    /// Opened file is not a database.
    NotADatabase,
    /// A generic SQL error or a code not mapped to another category.
    Unknown,
}

/// A SQLite result code and its primary error category.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// Primary category, or [`ErrorCode::Unknown`] for unmapped codes.
    pub code: ErrorCode,
    /// Original result code, including extended error bits.
    pub extended_code: c_int,
}

impl Error {
    /// Wraps a result code without validating that it represents an error.
    #[must_use]
    pub fn new(result_code: c_int) -> Self {
        let code = match result_code & 0xff {
            super::SQLITE_INTERNAL => ErrorCode::InternalMalfunction,
            super::SQLITE_PERM => ErrorCode::PermissionDenied,
            super::SQLITE_ABORT => ErrorCode::OperationAborted,
            super::SQLITE_BUSY => ErrorCode::DatabaseBusy,
            super::SQLITE_LOCKED => ErrorCode::DatabaseLocked,
            super::SQLITE_NOMEM => ErrorCode::OutOfMemory,
            super::SQLITE_READONLY => ErrorCode::ReadOnly,
            super::SQLITE_INTERRUPT => ErrorCode::OperationInterrupted,
            super::SQLITE_IOERR => ErrorCode::SystemIoFailure,
            super::SQLITE_CORRUPT => ErrorCode::DatabaseCorrupt,
            super::SQLITE_NOTFOUND => ErrorCode::NotFound,
            super::SQLITE_FULL => ErrorCode::DiskFull,
            super::SQLITE_CANTOPEN => ErrorCode::CannotOpen,
            super::SQLITE_PROTOCOL => ErrorCode::FileLockingProtocolFailed,
            super::SQLITE_SCHEMA => ErrorCode::SchemaChanged,
            super::SQLITE_TOOBIG => ErrorCode::TooBig,
            super::SQLITE_CONSTRAINT => ErrorCode::ConstraintViolation,
            super::SQLITE_MISMATCH => ErrorCode::TypeMismatch,
            super::SQLITE_MISUSE => ErrorCode::ApiMisuse,
            super::SQLITE_NOLFS => ErrorCode::NoLargeFileSupport,
            super::SQLITE_AUTH => ErrorCode::AuthorizationForStatementDenied,
            super::SQLITE_RANGE => ErrorCode::ParameterOutOfRange,
            super::SQLITE_NOTADB => ErrorCode::NotADatabase,
            _ => ErrorCode::Unknown,
        };

        Self {
            code,
            extended_code: result_code,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Error code {}: {}",
            self.extended_code,
            code_to_str(self.extended_code)
        )
    }
}

impl error::Error for Error {
    fn description(&self) -> &str {
        code_to_str(self.extended_code)
    }
}

/// Returns SQLite's static description of a result code.
#[must_use]
pub fn code_to_str(code: c_int) -> &'static str {
    let err_str = unsafe { super::sqlite3_errstr(code) };
    if err_str.is_null() {
        "Unknown error code"
    } else {
        // SQLite returns static, plain ASCII error messages.
        unsafe { core::ffi::CStr::from_ptr(err_str) }.to_str().unwrap()
    }
}
