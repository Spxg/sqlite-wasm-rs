//! Reusable transfers of closed, standalone database images, not online backups.
//! Keep the database and its sidecars idle until the transfer is finished/dropped.

use alloc::vec::Vec;

use crate::SQLITE3_HEADER;

/// Database signature, size or page-layout errors, not a full integrity check.
#[derive(thiserror::Error, Debug)]
pub enum ImportDbError {
    #[error("invalid database size or page alignment")]
    InvalidDbSize,
    #[error("invalid SQLite database signature")]
    InvalidHeader,
    #[error("page size must be a power of two between 512 and 65536 bytes")]
    InvalidPageSize,
}

/// Common transfer failures, converted into the backend's error type.
#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error("database import expected {expected} bytes, got {actual}")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("database transfer offset exceeds u64")]
    OffsetOverflow,
    #[error("database import cannot finish after a failed write")]
    ImportFailed,
    #[error("file is too large to export into a contiguous memory buffer")]
    FileTooLarge,
    #[error("unable to allocate memory for database export")]
    OutOfMemory,
    #[error("database export expected {expected} bytes, got {actual}")]
    ShortRead { expected: usize, actual: usize },
}

/// Whether to validate the SQLite header and reset its read/write versions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportMode {
    Checked,
    /// Preserves all bytes, including encrypted headers. Still checks length.
    Unchecked,
}

/// An unpublished destination. Drop must reclaim it unless committed.
pub trait ImportTarget: Sized {
    type Error: From<TransferError>;

    /// Writes the entire buffer at a logical database offset, or returns an error.
    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), Self::Error>;

    /// Flushes as needed and publishes the completed image. Cleans up on failure.
    fn commit(self) -> Result<(), Self::Error>;

    /// Reclaims the destination, reporting cleanup failures.
    fn abort(self) -> Result<(), Self::Error>;

    /// Reclaims after a failure, preserving both errors if cleanup also fails.
    fn abort_with_error(self, error: Self::Error) -> Self::Error;
}

/// A database image kept available until this handle is dropped.
/// The caller must not modify it during export; backends may enforce this.
pub trait ExportSource {
    type Error: From<TransferError>;

    fn size(&self) -> u64;

    /// Returns the bytes read, at most `buf.len()`. Zero means EOF.
    /// Partial reads are allowed and retried by the common exporter.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize, Self::Error>;
}

/// Optional import/export support, independent of SQLite's VFS callbacks.
/// Checkpoint/recover and close the database first; keep it idle during transfer.
/// Only the main file is copied, without merging WAL or recovering journals.
pub trait DbTransfer {
    type Error: From<TransferError>;
    type Target<'a>: ImportTarget<Error = Self::Error>
    where
        Self: 'a;
    type Source<'a>: ExportSource<Error = Self::Error>
    where
        Self: 'a;

    /// Checks backend limits/name conflicts and reserves an unpublished target.
    fn create_import(&self, name: &str, size: u64) -> Result<Self::Target<'_>, Self::Error>;

    /// Checks backend export preconditions and retains the image for reading.
    fn open_export(&self, name: &str) -> Result<Self::Source<'_>, Self::Error>;

    /// Starts a checked import of exactly `size` bytes.
    fn begin_import(
        &self,
        name: &str,
        size: u64,
    ) -> Result<DbImport<Self::Target<'_>>, Self::Error> {
        validate_size(size).map_err(TransferError::from)?;
        DbImport::new(self.create_import(name, size)?, size, ImportMode::Checked)
    }

    /// Starts a byte-preserving import of exactly `size` bytes.
    fn begin_import_unchecked(
        &self,
        name: &str,
        size: u64,
    ) -> Result<DbImport<Self::Target<'_>>, Self::Error> {
        DbImport::new(self.create_import(name, size)?, size, ImportMode::Unchecked)
    }

    /// Validates layout (not integrity), imports, and resets read/write versions
    /// to rollback mode. Use `import_db_unchecked` for encrypted images.
    fn import_db(&self, name: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        check_import_header(bytes, bytes.len() as u64).map_err(TransferError::from)?;
        import_bytes(self.begin_import(name, bytes.len() as u64)?, bytes)
    }

    /// Imports arbitrary bytes without interpreting or modifying the header.
    /// Empty images are allowed; no page size/alignment checks are performed.
    fn import_db_unchecked(&self, name: &str, bytes: &[u8]) -> Result<(), Self::Error> {
        import_bytes(
            self.begin_import_unchecked(name, bytes.len() as u64)?,
            bytes,
        )
    }

    fn begin_export(&self, name: &str) -> Result<DbExport<Self::Source<'_>>, Self::Error> {
        Ok(DbExport::new(self.open_export(name)?))
    }

    /// Exports into one allocation, limited to `isize::MAX` bytes (below 2 GiB
    /// on wasm32). Use `begin_export` for bounded memory usage.
    fn export_db(&self, name: &str) -> Result<Vec<u8>, Self::Error> {
        self.begin_export(name)?.read_to_vec()
    }
}

fn import_bytes<T: ImportTarget>(mut import: DbImport<T>, bytes: &[u8]) -> Result<(), T::Error> {
    if let Err(error) = import.write(bytes) {
        return Err(import.target.abort_with_error(error));
    }
    import.finish()
}

fn validate_size(size: u64) -> Result<(), ImportDbError> {
    if size < 512 || size % 512 != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }
    Ok(())
}

// Checks the first 18 bytes and total length, not integrity or sidecar state.
fn check_import_header(header: &[u8], size: u64) -> Result<(), ImportDbError> {
    validate_size(size)?;
    if header.len() < 18 || !header.starts_with(SQLITE3_HEADER.as_bytes()) {
        return Err(ImportDbError::InvalidHeader);
    }

    let page_size = u16::from_be_bytes([header[16], header[17]]);
    let page_size = if page_size == 1 {
        65536
    } else {
        u64::from(page_size)
    };
    if !(page_size.is_power_of_two() && (512..=65536).contains(&page_size)) {
        return Err(ImportDbError::InvalidPageSize);
    }
    if size % page_size != 0 {
        return Err(ImportDbError::InvalidDbSize);
    }
    Ok(())
}

/// Sequential import. Finish publishes; dropping the target aborts.
#[must_use = "write the database and finish, or drop to abort"]
pub struct DbImport<T: ImportTarget> {
    target: T,
    size: u64,
    offset: u64,
    mode: ImportMode,
    header: [u8; 18],
    failed: bool,
}

impl<T: ImportTarget> DbImport<T> {
    pub fn new(target: T, size: u64, mode: ImportMode) -> Result<Self, T::Error> {
        if mode == ImportMode::Checked {
            if let Err(error) = validate_size(size) {
                return Err(target.abort_with_error(TransferError::from(error).into()));
            }
        }

        Ok(Self {
            target,
            size,
            offset: 0,
            mode,
            header: [0; 18],
            failed: false,
        })
    }

    /// Writes a complete chunk. Any error prevents subsequent writes/commit.
    /// Checked imports validate as soon as the first 18 bytes are available.
    pub fn write(&mut self, bytes: &[u8]) -> Result<(), T::Error> {
        if self.failed {
            return Err(TransferError::ImportFailed.into());
        }

        let result = (|| {
            let end = self
                .offset
                .checked_add(bytes.len() as u64)
                .ok_or(TransferError::OffsetOverflow)?;
            if end > self.size {
                return Err(TransferError::SizeMismatch {
                    expected: self.size,
                    actual: end,
                }
                .into());
            }

            if self.mode == ImportMode::Checked && self.offset < self.header.len() as u64 {
                let start = self.offset as usize;
                let count = bytes.len().min(self.header.len() - start);
                self.header[start..start + count].copy_from_slice(&bytes[..count]);
                if end >= self.header.len() as u64 {
                    check_import_header(&self.header, self.size).map_err(TransferError::from)?;
                }
            }

            if !bytes.is_empty() {
                self.target.write_at(self.offset, bytes)?;
            }
            self.offset = end;
            Ok(())
        })();
        self.failed = result.is_err();
        result
    }

    /// Checks completeness, resets checked headers to rollback mode, and commits.
    pub fn finish(mut self) -> Result<(), T::Error> {
        let result = if self.failed {
            Err(TransferError::ImportFailed.into())
        } else if self.offset != self.size {
            Err(TransferError::SizeMismatch {
                expected: self.size,
                actual: self.offset,
            }
            .into())
        } else if self.mode == ImportMode::Checked {
            self.target.write_at(18, &[1, 1])
        } else {
            Ok(())
        };

        match result {
            Ok(()) => self.target.commit(),
            Err(error) => Err(self.target.abort_with_error(error)),
        }
    }

    pub fn abort(self) -> Result<(), T::Error> {
        self.target.abort()
    }
}

/// Sequential export. Retains the source until dropped, including at EOF.
pub struct DbExport<S: ExportSource> {
    source: S,
    size: u64,
    offset: u64,
}

impl<S: ExportSource> DbExport<S> {
    pub fn new(source: S) -> Self {
        let size = source.size();
        Self {
            source,
            size,
            offset: 0,
        }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// Fills up to EOF. Returns zero for an empty buffer or at EOF.
    /// On error the buffer may be partially filled; the cursor is unchanged.
    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, S::Error> {
        let length = (self.size - self.offset).min(buf.len() as u64) as usize;
        let mut done = 0;
        while done < length {
            let count = self
                .source
                .read_at(self.offset + done as u64, &mut buf[done..length])?;
            if count == 0 || count > length - done {
                return Err(TransferError::ShortRead {
                    expected: length - done,
                    actual: count,
                }
                .into());
            }
            done += count;
        }
        self.offset += length as u64;
        Ok(length)
    }

    /// Collects the remaining bytes into a contiguous allocation.
    pub fn read_to_vec(mut self) -> Result<Vec<u8>, S::Error> {
        let size = usize::try_from(self.size - self.offset)
            .ok()
            .filter(|&size| size <= isize::MAX as usize)
            .ok_or(TransferError::FileTooLarge)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| TransferError::OutOfMemory)?;
        bytes.resize(size, 0);
        self.read(&mut bytes)?;
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::{check_import_header, ImportDbError};

    #[test]
    fn test_import_rejects_invalid_header_and_page_boundaries() {
        let mut header = [0; 18];
        header[..16].copy_from_slice(b"SQLite format 3\0");
        header[16..18].copy_from_slice(&1u16.to_be_bytes());
        assert!(check_import_header(&header, 65536).is_ok());
        assert!(matches!(
            check_import_header(&header, 512),
            Err(ImportDbError::InvalidDbSize)
        ));

        for page_size in [0u16, 256, 513, 65535] {
            header[16..18].copy_from_slice(&page_size.to_be_bytes());
            assert!(matches!(
                check_import_header(&header, 65536),
                Err(ImportDbError::InvalidPageSize)
            ));
        }

        header[16..18].copy_from_slice(&512u16.to_be_bytes());
        assert!(check_import_header(&header, 512).is_ok());
        header[15] = b'x';
        assert!(matches!(
            check_import_header(&header, 65536),
            Err(ImportDbError::InvalidHeader)
        ));
    }
}
