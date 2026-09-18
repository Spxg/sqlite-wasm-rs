//! Reusable checks for developers implementing `VfsFile` and `VfsStore`.
//! These ordinary functions are available to dependent crates, including with
//! `no_std` and `alloc`. Call individual cases or the aggregate entry points.
//!
//! File checks overwrite their input. Store checks require an isolated, writable
//! namespace without concurrent users; names starting with `___test_vfs_store`
//! are reserved. Successfully opened handles are closed, with best-effort cleanup
//! on failure or unwinding. Backend panics are not caught.
//!
//! Contract failures return `VfsErrorCode::Io` with case/operation details.
//! Backend errors retain their SQLite and system codes, with added context.
//! These checks do not establish durability, locking or crash-recovery safety.

use alloc::{format, vec};
use core::fmt::Debug;

use crate::{
    AccessMode, FileKind, OpenAccess, OpenOptions, OpenRequest, VfsAppData, VfsError, VfsErrorCode,
    VfsFile, VfsResult, VfsStore,
};

fn context<T>(label: &str, result: VfsResult<T>) -> VfsResult<T> {
    result.map_err(|error| {
        let mut contextual =
            VfsError::new(error.code(), format!("{label}: {}", error.message()).into());
        if let Some(code) = error.system_error() {
            contextual = contextual.with_system_error(code);
        }
        contextual
    })
}

fn check<T: Debug + PartialEq>(operation: &str, actual: T, expected: T) -> VfsResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(VfsError::new(
            VfsErrorCode::Io,
            format!("{operation}: expected {expected:?}, got {actual:?}").into(),
        ))
    }
}

fn check_size(file: &impl VfsFile, expected: u64) -> VfsResult<()> {
    check("file size", context("size", file.size())?, expected)
}

fn write(file: &mut impl VfsFile, offset: u64, bytes: &[u8]) -> VfsResult<()> {
    context(
        &format!("write at offset {offset}, length {}", bytes.len()),
        file.write(bytes, offset),
    )
}

fn truncate(file: &mut impl VfsFile, size: u64) -> VfsResult<()> {
    context(&format!("truncate to {size}"), file.truncate(size))?;
    check_size(file, size)
}

fn check_read(
    file: &mut impl VfsFile,
    offset: u64,
    length: usize,
    expected: &[u8],
) -> VfsResult<()> {
    let mut actual = vec![99; length];
    let operation = format!("read at offset {offset}, length {length}");
    let count = context(&operation, file.read(&mut actual, offset))?;
    check(&format!("{operation}, byte count"), count, expected.len())?;
    if let Some((index, (&actual, &expected))) = actual
        .iter()
        .zip(expected)
        .enumerate()
        .find(|(_, (actual, expected))| actual != expected)
    {
        check(
            &format!("byte at offset {}", offset + index as u64),
            actual,
            expected,
        )?;
    }
    Ok(())
}

fn exists<S: VfsStore>(data: &S::AppData, name: &str) -> VfsResult<bool> {
    context("access", S::access(data, name, AccessMode::Exists))
}

fn require_absent<S: VfsStore>(data: &S::AppData, name: &str) -> VfsResult<()> {
    check(
        &format!("test file {name:?} exists before creation"),
        exists::<S>(data, name)?,
        false,
    )
}

// Owns an opened handle and, for named cases, cleanup of the reserved name.
// Name ownership survives close/reopen failures; unexpected successful opens
// of an existing file only own their extra handle, not the existing name.
struct TestFile<'a, S: VfsStore> {
    data: &'a S::AppData,
    name: Option<&'a str>,
    options: OpenOptions,
    file: Option<S::File>,
    remove: bool,
}

impl<'a, S: VfsStore> TestFile<'a, S> {
    fn open(data: &'a S::AppData, name: Option<&'a str>, options: OpenOptions) -> VfsResult<Self> {
        let request = name.map_or_else(
            || OpenRequest::temporary(options),
            |name| OpenRequest::named(name, options),
        );
        let opened = context("open", S::open_file(data, request))?;
        let file = Self {
            data,
            name,
            options,
            file: Some(opened.file),
            remove: name.is_some(),
        };
        check("opened access mode", opened.access, options.access())?;
        Ok(file)
    }

    fn handle(&mut self) -> &mut S::File {
        self.file.as_mut().expect("test handle is open")
    }

    fn close_handle(&mut self) -> VfsResult<()> {
        if let Some(file) = self.file.take() {
            context(
                "close",
                S::close_file(self.data, self.name, file, self.options),
            )?;
        }
        Ok(())
    }

    fn reopen(&mut self, options: OpenOptions) -> VfsResult<()> {
        self.close_handle()?;
        let name = self.name.expect("only named test files are reopened");
        let opened = context(
            "reopen",
            S::open_file(self.data, OpenRequest::named(name, options)),
        )?;
        self.options = options;
        self.file = Some(opened.file);
        check("reopened access mode", opened.access, options.access())
    }

    fn remove(&mut self) -> VfsResult<()> {
        if !self.remove {
            return Ok(());
        }
        self.remove = false;
        let name = self.name.expect("only named test files need removal");
        // DELETEONCLOSE may unlink at open time. Do not demand a visible name
        // while open or delete an already-removed file after closing it.
        if self.options.delete_on_close() && matches!(exists::<S>(self.data, name), Ok(false)) {
            return Ok(());
        }
        context("delete", S::delete_file(self.data, name, false))
    }

    fn finish(mut self) -> VfsResult<()> {
        let close = self.close_handle();
        let remove = self.remove();
        close.and(remove)
    }
}

impl<S: VfsStore> Drop for TestFile<'_, S> {
    fn drop(&mut self) {
        let _ = self.close_handle();
        let _ = self.remove();
    }
}

fn expect_open_failure<S: VfsStore>(
    data: &S::AppData,
    name: &str,
    options: OpenOptions,
    remove_if_created: bool,
) -> VfsResult<()> {
    match S::open_file(data, OpenRequest::named(name, options)) {
        Err(_) => Ok(()),
        Ok(opened) => {
            let _file = TestFile::<S> {
                data,
                name: Some(name),
                options,
                file: Some(opened.file),
                remove: remove_if_created,
            };
            check("open result", "success", "error")
        }
    }
}

/// Checks reads, sparse writes, lengths and EOF, overwriting a writable file.
/// The caller retains responsibility for closing and removing the file.
#[doc(hidden)]
pub fn test_vfs_file_read_write(file: &mut impl VfsFile) -> VfsResult<()> {
    context(
        "read_write",
        (|| {
            truncate(file, 0)?;
            let mut bytes = vec![42; 64 * 1024];
            bytes[..12].copy_from_slice(b"hello world!");
            write(file, 0, &bytes)?;
            check_size(file, bytes.len() as u64)?;
            check_read(file, 0, bytes.len() + 16, &bytes)?;

            let offset = 1024 * 1024;
            write(file, offset as u64, &bytes)?;
            let mut expected = bytes.clone();
            expected.resize(offset, 0);
            expected.extend_from_slice(&bytes);
            check_size(file, expected.len() as u64)?;
            check_read(file, 0, expected.len(), &expected)?;
            let eof = expected.len() as u64;
            check_read(file, eof, 16, &[])?;
            check_read(file, eof + 1, 16, &[])?;
            check_read(file, 0, 0, &[])?;
            check_read(file, eof, 0, &[])
        })(),
    )
}

/// Checks middle overwrites preserve surrounding bytes and file length.
/// Overwrites a writable file; the caller must close and remove it afterwards.
#[doc(hidden)]
pub fn test_vfs_file_overwrite(file: &mut impl VfsFile) -> VfsResult<()> {
    context(
        "overwrite",
        (|| {
            truncate(file, 0)?;
            let mut expected = [42; 32];
            write(file, 0, &expected)?;
            write(file, 7, &[1, 2, 3, 4, 5])?;
            expected[7..12].copy_from_slice(&[1, 2, 3, 4, 5]);
            check_size(file, expected.len() as u64)?;
            check_read(file, 0, expected.len(), &expected)
        })(),
    )
}

/// Checks shrinking, retained bytes, zero-filled gaps after shrinking, and reuse
/// after truncation to zero. Overwrites a writable file; the caller closes it.
#[doc(hidden)]
pub fn test_vfs_file_truncate(file: &mut impl VfsFile) -> VfsResult<()> {
    context(
        "truncate",
        (|| {
            truncate(file, 0)?;
            write(file, 0, &[7; 8192])?;
            truncate(file, 513)?;
            check_read(file, 0, 529, &[7; 513])?;
            check_read(file, 513, 16, &[])?;
            write(file, 1025, &[9; 3])?;
            let mut expected = vec![7; 513];
            expected.resize(1025, 0);
            expected.extend_from_slice(&[9; 3]);
            check_size(file, expected.len() as u64)?;
            check_read(file, 0, expected.len(), &expected)?;
            truncate(file, 0)?;
            check_read(file, 0, 16, &[])?;
            write(file, 0, &[1, 2, 3])?;
            check_size(file, 3)?;
            check_read(file, 0, 8, &[1, 2, 3])
        })(),
    )
}

/// Runs all file checks. Overwrites a writable file and leaves it open.
/// The caller must close it and remove any test file afterwards.
#[doc(hidden)]
pub fn test_vfs_file<File: VfsFile>(file: &mut File) -> VfsResult<()> {
    test_vfs_file_read_write(file)?;
    test_vfs_file_overwrite(file)?;
    test_vfs_file_truncate(file)
}

/// Checks missing-file, exclusive-create and read-only opens, including content
/// preservation after rejected operations. Requires an isolated writable store.
#[doc(hidden)]
pub fn test_vfs_store_open_modes<S: VfsStore>(data: &S::AppData) -> VfsResult<()> {
    context(
        "open_modes",
        (|| {
            let name = "___test_vfs_store_open_modes___";
            require_absent::<S>(data, name)?;
            let writable = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb);
            let readonly = OpenOptions::new(OpenAccess::ReadOnly, FileKind::MainDb);
            for options in [writable, readonly] {
                context(
                    "missing file without CREATE",
                    expect_open_failure::<S>(data, name, options, true),
                )?;
                require_absent::<S>(data, name)?;
            }
            let mut file = TestFile::<S>::open(data, Some(name), writable.with_create_new())?;
            write(file.handle(), 0, &[41, 42, 43])?;
            file.close_handle()?;
            context(
                "exclusive create of existing file",
                expect_open_failure::<S>(data, name, writable.with_create_new(), false),
            )?;
            file.reopen(readonly)?;
            check(
                "read-only write at offset 0, length 1 rejected",
                file.handle().write(&[99], 0).is_err(),
                true,
            )?;
            check(
                "read-only truncate to 0 rejected",
                file.handle().truncate(0).is_err(),
                true,
            )?;
            check_size(file.handle(), 3)?;
            check_read(file.handle(), 0, 8, &[41, 42, 43])?;
            file.reopen(writable)?;
            check_size(file.handle(), 3)?;
            check_read(file.handle(), 0, 8, &[41, 42, 43])?;
            file.finish()
        })(),
    )
}

/// Checks close/reopen preserves contents and length without truncating or
/// recreating the file. This is not a crash-recovery or durability test.
#[doc(hidden)]
pub fn test_vfs_store_reopen<S: VfsStore>(data: &S::AppData) -> VfsResult<()> {
    context(
        "reopen",
        (|| {
            let name = "___test_vfs_store_reopen___";
            require_absent::<S>(data, name)?;
            let options = OpenOptions::new(OpenAccess::ReadWrite, FileKind::MainDb);
            let mut file = TestFile::<S>::open(data, Some(name), options.with_create_new())?;
            write(file.handle(), 0, &[41, 42, 43])?;
            file.reopen(options)?;
            check_size(file.handle(), 3)?;
            check_read(file.handle(), 0, 8, &[41, 42, 43])?;
            file.finish()
        })(),
    )
}

/// Checks DELETEONCLOSE without requiring the name to remain visible while open.
/// Requires an isolated writable store; leftover named files are cleaned up.
#[doc(hidden)]
pub fn test_vfs_store_delete_on_close<S: VfsStore>(data: &S::AppData) -> VfsResult<()> {
    context(
        "delete_on_close",
        (|| {
            let name = "___test_vfs_store_delete_on_close___";
            require_absent::<S>(data, name)?;
            let options = OpenOptions::new(OpenAccess::ReadWrite, FileKind::TempDb)
                .with_create_new()
                .with_delete_on_close();
            let mut file = TestFile::<S>::open(data, Some(name), options)?;
            write(file.handle(), 0, &[41, 42, 43])?;
            check_read(file.handle(), 0, 8, &[41, 42, 43])?;
            file.close_handle()?;
            check(
                "file exists after DELETEONCLOSE",
                exists::<S>(data, name)?,
                false,
            )?;
            file.finish()
        })(),
    )
}

/// Checks anonymous temporary-file I/O and closing. The backend owns temporary
/// resource cleanup; no assumptions are made about its naming or storage scheme.
#[doc(hidden)]
pub fn test_vfs_store_temporary<S: VfsStore>(data: &S::AppData) -> VfsResult<()> {
    context(
        "temporary",
        (|| {
            let options =
                OpenOptions::new(OpenAccess::ReadWrite, FileKind::TempDb).with_delete_on_close();
            let mut file = TestFile::<S>::open(data, None, options)?;
            test_vfs_file(file.handle())?;
            file.finish()
        })(),
    )
}

/// Runs file checks on named main-database/temporary files, followed by the open
/// mode, reopen, DELETEONCLOSE and anonymous-file cases. See the module's isolation
/// and cleanup requirements. This calls the store directly, not SQLite callbacks.
#[doc(hidden)]
pub fn test_vfs_store<S: VfsStore>(vfs_data: VfsAppData<S::AppData>) -> VfsResult<()> {
    for (name, kind) in [
        ("___test_vfs_store#1___", FileKind::MainDb),
        ("___test_vfs_store#2___", FileKind::TempDb),
    ] {
        context(
            name,
            (|| {
                require_absent::<S>(&vfs_data, name)?;
                let options = OpenOptions::new(OpenAccess::ReadWrite, kind).with_create_new();
                let mut file = TestFile::<S>::open(&vfs_data, Some(name), options)?;
                check(
                    "file exists after creation",
                    exists::<S>(&vfs_data, name)?,
                    true,
                )?;
                test_vfs_file(file.handle())?;
                file.finish()?;
                check(
                    "file exists after deletion",
                    exists::<S>(&vfs_data, name)?,
                    false,
                )
            })(),
        )?;
    }
    test_vfs_store_open_modes::<S>(&vfs_data)?;
    test_vfs_store_reopen::<S>(&vfs_data)?;
    test_vfs_store_delete_on_close::<S>(&vfs_data)?;
    test_vfs_store_temporary::<S>(&vfs_data)
}
