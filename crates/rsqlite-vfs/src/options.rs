//! Typed values used by the safe VFS delegates. Raw conversions belong at the
//! SQLite callback boundary or at explicit interoperability points.

use crate::{ffi::*, VfsError, VfsErrorCode, VfsResult};

/// A positive device sector size representable by SQLite's C interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SectorSize(i32);

impl SectorSize {
    pub const DEFAULT: Self = Self(4096);

    /// Validates only that `bytes` is positive and fits in a signed 32-bit integer.
    /// The caller must supply the actual storage guarantee; no power-of-two or
    /// hardware-sector validation is performed.
    pub const fn new(bytes: u32) -> Option<Self> {
        if bytes == 0 || bytes > i32::MAX as u32 {
            None
        } else {
            Some(Self(bytes as i32))
        }
    }

    pub const fn bytes(self) -> u32 {
        self.0 as u32
    }
}

/// Guarantees of the underlying storage, not requested features. Advertising
/// guarantees the backend cannot meet can cause database corruption.
/// See <https://www.sqlite.org/c3ref/c_iocap_atomic.html>.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeviceCharacteristics(i32);

impl DeviceCharacteristics {
    pub const NONE: Self = Self(0);
    pub const ATOMIC: Self = Self(SQLITE_IOCAP_ATOMIC);
    pub const ATOMIC512: Self = Self(SQLITE_IOCAP_ATOMIC512);
    pub const ATOMIC1K: Self = Self(SQLITE_IOCAP_ATOMIC1K);
    pub const ATOMIC2K: Self = Self(SQLITE_IOCAP_ATOMIC2K);
    pub const ATOMIC4K: Self = Self(SQLITE_IOCAP_ATOMIC4K);
    pub const ATOMIC8K: Self = Self(SQLITE_IOCAP_ATOMIC8K);
    pub const ATOMIC16K: Self = Self(SQLITE_IOCAP_ATOMIC16K);
    pub const ATOMIC32K: Self = Self(SQLITE_IOCAP_ATOMIC32K);
    pub const ATOMIC64K: Self = Self(SQLITE_IOCAP_ATOMIC64K);
    pub const SAFE_APPEND: Self = Self(SQLITE_IOCAP_SAFE_APPEND);
    pub const SEQUENTIAL: Self = Self(SQLITE_IOCAP_SEQUENTIAL);
    pub const UNDELETABLE_WHEN_OPEN: Self = Self(SQLITE_IOCAP_UNDELETABLE_WHEN_OPEN);
    pub const POWERSAFE_OVERWRITE: Self = Self(SQLITE_IOCAP_POWERSAFE_OVERWRITE);
    pub const IMMUTABLE: Self = Self(SQLITE_IOCAP_IMMUTABLE);
    pub const BATCH_ATOMIC: Self = Self(SQLITE_IOCAP_BATCH_ATOMIC);
    pub const SUBPAGE_READ: Self = Self(SQLITE_IOCAP_SUBPAGE_READ);

    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    pub const fn as_raw(self) -> i32 {
        self.0
    }
}

impl core::ops::BitOr for DeviceCharacteristics {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        self.union(rhs)
    }
}

/// SQLite database-file lock levels, ordered from least to most restrictive.
/// `lock` upgrades; `unlock` downgrades. These are not OS-specific lock values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LockLevel {
    None,
    Shared,
    Reserved,
    Pending,
    Exclusive,
}

impl LockLevel {
    pub const fn from_raw(value: i32) -> Option<Self> {
        match value {
            SQLITE_LOCK_NONE => Some(Self::None),
            SQLITE_LOCK_SHARED => Some(Self::Shared),
            SQLITE_LOCK_RESERVED => Some(Self::Reserved),
            SQLITE_LOCK_PENDING => Some(Self::Pending),
            SQLITE_LOCK_EXCLUSIVE => Some(Self::Exclusive),
            _ => None,
        }
    }
}

/// The question asked by SQLite's `xAccess`, not an open mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessMode {
    Exists,
    Read,
    ReadWrite,
}

impl AccessMode {
    pub const fn from_raw(value: i32) -> Option<Self> {
        match value {
            SQLITE_ACCESS_EXISTS => Some(Self::Exists),
            SQLITE_ACCESS_READ => Some(Self::Read),
            SQLITE_ACCESS_READWRITE => Some(Self::ReadWrite),
            _ => None,
        }
    }
}

/// `xSync` strength, not the setting of `PRAGMA synchronous`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode {
    /// Normal fsync semantics.
    Normal,
    /// macOS-style fullsync semantics where the platform supports them.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncOptions {
    pub mode: SyncMode,
    /// Data must be synchronized; inode metadata need not be. Synchronizing
    /// more than requested is permitted.
    pub data_only: bool,
}

impl SyncOptions {
    pub const fn from_raw_flags(flags: i32) -> Option<Self> {
        let mode = match flags & 0x0f {
            SQLITE_SYNC_NORMAL => SyncMode::Normal,
            SQLITE_SYNC_FULL => SyncMode::Full,
            _ => return None,
        };
        if flags & !(0x0f | SQLITE_SYNC_DATAONLY) != 0 {
            return None;
        }
        Some(Self {
            mode,
            data_only: flags & SQLITE_SYNC_DATAONLY != 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAccess {
    ReadOnly,
    ReadWrite,
}

/// The type of file SQLite is opening. This is independent of its access mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    MainDb,
    TempDb,
    TransientDb,
    MainJournal,
    TempJournal,
    SubJournal,
    SuperJournal,
    Wal,
}

impl FileKind {
    const fn as_raw(self) -> i32 {
        match self {
            Self::MainDb => SQLITE_OPEN_MAIN_DB,
            Self::TempDb => SQLITE_OPEN_TEMP_DB,
            Self::TransientDb => SQLITE_OPEN_TRANSIENT_DB,
            Self::MainJournal => SQLITE_OPEN_MAIN_JOURNAL,
            Self::TempJournal => SQLITE_OPEN_TEMP_JOURNAL,
            Self::SubJournal => SQLITE_OPEN_SUBJOURNAL,
            Self::SuperJournal => SQLITE_OPEN_SUPER_JOURNAL,
            Self::Wal => SQLITE_OPEN_WAL,
        }
    }
}

const FILE_KIND_MASK: i32 = SQLITE_OPEN_MAIN_DB
    | SQLITE_OPEN_TEMP_DB
    | SQLITE_OPEN_TRANSIENT_DB
    | SQLITE_OPEN_MAIN_JOURNAL
    | SQLITE_OPEN_TEMP_JOURNAL
    | SQLITE_OPEN_SUBJOURNAL
    | SQLITE_OPEN_SUPER_JOURNAL
    | SQLITE_OPEN_WAL;

/// Validated `xOpen` options. Unknown/platform-specific bits are retained for
/// lossless interoperability, but normal backend code uses the typed getters.
/// This is a VFS interface, not `sqlite3_open_v2`'s application-level flags API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenOptions(i32);

impl OpenOptions {
    /// Selects access and file kind only; creation and other flags start unset.
    pub const fn new(access: OpenAccess, kind: FileKind) -> Self {
        let access = match access {
            OpenAccess::ReadOnly => SQLITE_OPEN_READONLY,
            OpenAccess::ReadWrite => SQLITE_OPEN_READWRITE,
        };
        Self(access | kind.as_raw())
    }

    pub fn from_raw_flags(flags: i32) -> VfsResult<Self> {
        let access = flags & (SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE);
        if !matches!(access, SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE)
            || (flags & SQLITE_OPEN_CREATE != 0 && access != SQLITE_OPEN_READWRITE)
            || (flags & SQLITE_OPEN_EXCLUSIVE != 0 && flags & SQLITE_OPEN_CREATE == 0)
            || (flags & SQLITE_OPEN_DELETEONCLOSE != 0 && flags & SQLITE_OPEN_CREATE == 0)
            || (flags & FILE_KIND_MASK).count_ones() > 1
        {
            return Err(VfsError::new(
                VfsErrorCode::CantOpen,
                "invalid SQLite open flags".into(),
            ));
        }
        Ok(Self(flags))
    }

    /// Explicit raw interoperability, including bits not interpreted here.
    pub const fn raw_flags(self) -> i32 {
        self.0
    }

    pub const fn access(self) -> OpenAccess {
        if self.0 & SQLITE_OPEN_READONLY != 0 {
            OpenAccess::ReadOnly
        } else {
            OpenAccess::ReadWrite
        }
    }

    /// SQLite supplies one file kind. `None` also permits direct backend users
    /// to open an unclassified file, without inventing a SQLite file type.
    pub const fn kind(self) -> Option<FileKind> {
        match self.0 & FILE_KIND_MASK {
            SQLITE_OPEN_MAIN_DB => Some(FileKind::MainDb),
            SQLITE_OPEN_TEMP_DB => Some(FileKind::TempDb),
            SQLITE_OPEN_TRANSIENT_DB => Some(FileKind::TransientDb),
            SQLITE_OPEN_MAIN_JOURNAL => Some(FileKind::MainJournal),
            SQLITE_OPEN_TEMP_JOURNAL => Some(FileKind::TempJournal),
            SQLITE_OPEN_SUBJOURNAL => Some(FileKind::SubJournal),
            SQLITE_OPEN_SUPER_JOURNAL => Some(FileKind::SuperJournal),
            SQLITE_OPEN_WAL => Some(FileKind::Wal),
            _ => None,
        }
    }

    /// Enables creation if absent and selects read-write access.
    pub const fn with_create(self) -> Self {
        Self((self.0 & !SQLITE_OPEN_READONLY) | SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE)
    }

    /// Requires a newly created file and selects read-write access. An existing
    /// file is an error; exclusive creation is not a database-file lock.
    pub const fn with_create_new(self) -> Self {
        Self(self.with_create().0 | SQLITE_OPEN_EXCLUSIVE)
    }

    /// Requests delete-on-close, including the read-write/create flags required
    /// by SQLite for temporary files.
    pub const fn with_delete_on_close(self) -> Self {
        Self(self.with_create().0 | SQLITE_OPEN_DELETEONCLOSE)
    }

    pub const fn create(self) -> bool {
        self.0 & SQLITE_OPEN_CREATE != 0
    }

    pub const fn exclusive(self) -> bool {
        self.0 & SQLITE_OPEN_EXCLUSIVE != 0
    }

    pub const fn delete_on_close(self) -> bool {
        self.0 & SQLITE_OPEN_DELETEONCLOSE != 0
    }

    pub const fn uri(self) -> bool {
        self.0 & SQLITE_OPEN_URI != 0
    }

    pub const fn memory(self) -> bool {
        self.0 & SQLITE_OPEN_MEMORY != 0
    }

    pub const fn no_follow(self) -> bool {
        self.0 & SQLITE_OPEN_NOFOLLOW != 0
    }

    pub const fn auto_proxy(self) -> bool {
        self.0 & SQLITE_OPEN_AUTOPROXY != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_options_preserve_unknown_flags_and_reject_invalid_combinations() {
        let raw = SQLITE_OPEN_READONLY
            | SQLITE_OPEN_MAIN_DB
            | SQLITE_OPEN_URI
            | SQLITE_OPEN_MEMORY
            | SQLITE_OPEN_NOFOLLOW
            | SQLITE_OPEN_AUTOPROXY
            | SQLITE_OPEN_PRIVATECACHE
            | 0x40000000;
        let options = OpenOptions::from_raw_flags(raw).unwrap();
        assert_eq!(options.raw_flags(), raw);
        assert_eq!(options.access(), OpenAccess::ReadOnly);
        assert!(options.uri() && options.memory() && options.no_follow() && options.auto_proxy());
        for flags in [
            0,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_READWRITE,
            SQLITE_OPEN_READONLY | SQLITE_OPEN_CREATE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_EXCLUSIVE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_DELETEONCLOSE,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_MAIN_DB | SQLITE_OPEN_WAL,
        ] {
            assert_eq!(
                OpenOptions::from_raw_flags(flags).unwrap_err().code(),
                VfsErrorCode::CantOpen
            );
        }
    }
}
