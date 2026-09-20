//! Named SQLite errors with an explicit, lossless raw-code escape hatch.

use crate::ffi::*;

/// A nonzero, platform-specific error number, such as Unix `errno` or Windows
/// `GetLastError`, for `sqlite3_system_errno`. This is not a SQLite result code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemErrorCode(core::num::NonZeroI32);

impl SystemErrorCode {
    /// Wraps an OS error number. Zero means no system error and returns `None`.
    ///
    /// Capture the number at the failing operation, before another call changes it.
    pub const fn from_raw(code: i32) -> Option<Self> {
        match core::num::NonZeroI32::new(code) {
            Some(code) => Some(Self(code)),
            None => None,
        }
    }

    pub const fn as_raw(self) -> i32 {
        self.0.get()
    }
}

/// A validated raw SQLite error code, obtained through [`VfsErrorCode::from_raw`].
///
/// Its private field prevents bypassing validation for [`VfsErrorCode::Other`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RawVfsErrorCode(i32);

impl RawVfsErrorCode {
    pub const fn as_raw(self) -> i32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VfsErrorCode {
    Error,
    Permission,
    Busy,
    Locked,
    NoMemory,
    ReadOnly,
    Interrupt,
    Io,
    Corrupt,
    NotFound,
    Full,
    CantOpen,
    Protocol,
    Misuse,
    Auth,
    IoRead,
    IoShortRead,
    IoWrite,
    IoSync,
    IoDirectorySync,
    IoTruncate,
    IoStat,
    IoUnlock,
    IoReadLock,
    IoDelete,
    IoAccess,
    IoCheckReservedLock,
    IoLock,
    IoClose,
    IoDeleteNoEntry,
    /// Other/extended SQLite errors, created through [`Self::from_raw`].
    Other(RawVfsErrorCode),
}

impl VfsErrorCode {
    pub const fn as_raw(self) -> i32 {
        match self {
            Self::Error => SQLITE_ERROR,
            Self::Permission => SQLITE_PERM,
            Self::Busy => SQLITE_BUSY,
            Self::Locked => SQLITE_LOCKED,
            Self::NoMemory => SQLITE_NOMEM,
            Self::ReadOnly => SQLITE_READONLY,
            Self::Interrupt => SQLITE_INTERRUPT,
            Self::Io => SQLITE_IOERR,
            Self::Corrupt => SQLITE_CORRUPT,
            Self::NotFound => SQLITE_NOTFOUND,
            Self::Full => SQLITE_FULL,
            Self::CantOpen => SQLITE_CANTOPEN,
            Self::Protocol => SQLITE_PROTOCOL,
            Self::Misuse => SQLITE_MISUSE,
            Self::Auth => SQLITE_AUTH,
            Self::IoRead => SQLITE_IOERR_READ,
            Self::IoShortRead => SQLITE_IOERR_SHORT_READ,
            Self::IoWrite => SQLITE_IOERR_WRITE,
            Self::IoSync => SQLITE_IOERR_FSYNC,
            Self::IoDirectorySync => SQLITE_IOERR_DIR_FSYNC,
            Self::IoTruncate => SQLITE_IOERR_TRUNCATE,
            Self::IoStat => SQLITE_IOERR_FSTAT,
            Self::IoUnlock => SQLITE_IOERR_UNLOCK,
            Self::IoReadLock => SQLITE_IOERR_RDLOCK,
            Self::IoDelete => SQLITE_IOERR_DELETE,
            Self::IoAccess => SQLITE_IOERR_ACCESS,
            Self::IoCheckReservedLock => SQLITE_IOERR_CHECKRESERVEDLOCK,
            Self::IoLock => SQLITE_IOERR_LOCK,
            Self::IoClose => SQLITE_IOERR_CLOSE,
            Self::IoDeleteNoEntry => SQLITE_IOERR_DELETE_NOENT,
            Self::Other(code) => code.as_raw(),
        }
    }

    /// Converts an error code, preserving extended bits. Prefer named variants.
    ///
    /// Rejects negative values and unknown/non-error primary codes, including
    /// `SQLITE_OK`, `SQLITE_ROW` and `SQLITE_DONE`.
    ///
    /// ```
    /// use rsqlite_vfs::{VfsErrorCode, ffi::*};
    /// assert_eq!(VfsErrorCode::from_raw(SQLITE_OK), None);
    /// let code = VfsErrorCode::from_raw(SQLITE_IOERR_AUTH).unwrap();
    /// assert_eq!(code.as_raw(), SQLITE_IOERR_AUTH);
    /// ```
    pub const fn from_raw(code: i32) -> Option<Self> {
        // SQLite stores the primary result code in the low eight bits.
        let primary = code & 0xff;
        if code < 0 || primary < SQLITE_ERROR || primary > SQLITE_WARNING {
            return None;
        }
        Some(match code {
            SQLITE_ERROR => Self::Error,
            SQLITE_PERM => Self::Permission,
            SQLITE_BUSY => Self::Busy,
            SQLITE_LOCKED => Self::Locked,
            SQLITE_NOMEM => Self::NoMemory,
            SQLITE_READONLY => Self::ReadOnly,
            SQLITE_INTERRUPT => Self::Interrupt,
            SQLITE_IOERR => Self::Io,
            SQLITE_CORRUPT => Self::Corrupt,
            SQLITE_NOTFOUND => Self::NotFound,
            SQLITE_FULL => Self::Full,
            SQLITE_CANTOPEN => Self::CantOpen,
            SQLITE_PROTOCOL => Self::Protocol,
            SQLITE_MISUSE => Self::Misuse,
            SQLITE_AUTH => Self::Auth,
            SQLITE_IOERR_READ => Self::IoRead,
            SQLITE_IOERR_SHORT_READ => Self::IoShortRead,
            SQLITE_IOERR_WRITE => Self::IoWrite,
            SQLITE_IOERR_FSYNC => Self::IoSync,
            SQLITE_IOERR_DIR_FSYNC => Self::IoDirectorySync,
            SQLITE_IOERR_TRUNCATE => Self::IoTruncate,
            SQLITE_IOERR_FSTAT => Self::IoStat,
            SQLITE_IOERR_UNLOCK => Self::IoUnlock,
            SQLITE_IOERR_RDLOCK => Self::IoReadLock,
            SQLITE_IOERR_DELETE => Self::IoDelete,
            SQLITE_IOERR_ACCESS => Self::IoAccess,
            SQLITE_IOERR_CHECKRESERVEDLOCK => Self::IoCheckReservedLock,
            SQLITE_IOERR_LOCK => Self::IoLock,
            SQLITE_IOERR_CLOSE => Self::IoClose,
            SQLITE_IOERR_DELETE_NOENT => Self::IoDeleteNoEntry,
            code => Self::Other(RawVfsErrorCode(code)),
        })
    }
}

impl core::fmt::Display for VfsErrorCode {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.as_raw().fmt(f)
    }
}
