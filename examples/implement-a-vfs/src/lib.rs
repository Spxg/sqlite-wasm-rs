//! This is a tutorial on implementing VFS
//!
//! We want to implement a simple memory VFS

use sqlite_wasm_rs::{
    sqlite3_close, sqlite3_exec, sqlite3_open_v2,
    utils::{
        ffi::{SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE},
        register_vfs, AccessMode, LockLevel, OpenAccess, OpenOptions, OpenedFile, SQLiteIoMethods,
        SQLiteVfs, SyncOptions, VfsError, VfsErrorCode, VfsFile, VfsResult, VfsStore,
    },
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm_bindgen_test::wasm_bindgen_test;

/// Each open owns a handle with its own access mode, sharing the file's bytes.
struct MemFile {
    data: Rc<RefCell<Vec<u8>>>,
    read_only: bool,
}

impl MemFile {
    fn check_writable(&self) -> VfsResult<()> {
        if self.read_only {
            return Err(VfsError::new(
                VfsErrorCode::ReadOnly,
                "File is read-only".into(),
            ));
        }
        Ok(())
    }
}

/// Some basic capabilities of file
impl VfsFile for MemFile {
    /// Called by `xRead`
    ///
    /// We copy the data starting at offset in the memory file to buffer,
    /// returning the number of bytes copied. `xRead` handles zero-filling
    /// and reports a short read if the buffer cannot be filled.
    fn read(&mut self, buf: &mut [u8], offset: u64) -> VfsResult<usize> {
        let size = buf.len();
        let data = self.data.borrow();
        if data.len() as u64 <= offset {
            return Ok(0);
        }

        let offset = offset as usize;
        let read_size = size.min(data.len() - offset);
        let read_end = offset + read_size;
        buf[..read_size].copy_from_slice(&data[offset..read_end]);

        Ok(read_size)
    }

    /// Called by `xWrite`
    ///
    /// We copy the data in the buffer to the memory file,
    /// and if the size is not enough, expand it.
    fn write(&mut self, buf: &[u8], offset: u64) -> VfsResult<()> {
        self.check_writable()?;
        let mut data = self.data.borrow_mut();
        let offset = usize::try_from(offset).map_err(|_| {
            VfsError::new(
                VfsErrorCode::Full,
                "File offset exceeds address space".into(),
            )
        })?;
        let end = offset
            .checked_add(buf.len())
            .filter(|&end| end <= isize::MAX as usize)
            .ok_or_else(|| {
                VfsError::new(VfsErrorCode::Full, "File size exceeds address space".into())
            })?;
        if end > data.len() {
            data.resize(end, 0);
        }
        data[offset..end].copy_from_slice(buf);
        Ok(())
    }

    /// Called by `xTruncate`
    ///
    /// Truncate the memory file, which happens during vacuum
    fn truncate(&mut self, size: u64) -> VfsResult<()> {
        self.check_writable()?;
        let size = usize::try_from(size).map_err(|_| {
            VfsError::new(VfsErrorCode::Full, "File size exceeds address space".into())
        })?;
        self.data.borrow_mut().truncate(size);
        Ok(())
    }

    /// Called by `xSync`
    ///
    /// Write the data back to "disk".
    ///
    /// Since we are in memory, the write operation takes effect immediately,
    /// so we return directly.
    fn sync(&mut self, _options: SyncOptions) -> VfsResult<()> {
        Ok(())
    }

    // This tutorial assumes exclusive application ownership of the database.
    // These no-ops do not coordinate transactions between SQLite connections.
    fn lock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }
    fn unlock(&mut self, _level: LockLevel) -> VfsResult<()> {
        Ok(())
    }
    fn check_reserved_lock(&self) -> VfsResult<bool> {
        Ok(true)
    }

    /// Called by `xFileSize`
    ///
    /// Get the memory file size
    fn size(&self) -> VfsResult<u64> {
        Ok(self.data.borrow().len() as u64)
    }
}

/// This is where we store our data.
///
/// Since we will have multiple different databases,
/// we use hashmap to store the data with the file name as the key.
#[derive(Default)]
struct MemAppData {
    files: RefCell<HashMap<String, Rc<RefCell<Vec<u8>>>>>,
    error: RefCell<Option<VfsError>>,
}

/// Something that manages our memory files
struct MemFileStore;

/// Make changes to files
impl VfsStore for MemFileStore {
    type File = MemFile;
    type AppData = MemAppData;
    fn record_error(data: &Self::AppData, error: VfsError) {
        data.error.replace(Some(error));
    }
    fn last_error(data: &Self::AppData) -> Option<VfsError> {
        data.error.borrow().clone()
    }
    /// Called by `xOpen`
    ///
    /// Return a fresh handle, creating the underlying data only when requested.
    fn open_file(
        app_data: &MemAppData,
        request: sqlite_wasm_rs::utils::OpenRequest<'_>,
    ) -> VfsResult<OpenedFile<MemFile>> {
        let options = request.options;
        let Some(filename) = request.filename else {
            return Ok(OpenedFile {
                file: MemFile {
                    data: Rc::new(RefCell::new(Vec::new())),
                    read_only: options.access() == OpenAccess::ReadOnly,
                },
                access: options.access(),
            });
        };
        let name = filename.path();
        let mut files = app_data.files.borrow_mut();
        let data = match files.get(name) {
            Some(_) if options.exclusive() => {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("{name} already exists").into(),
                ));
            }
            Some(data) => data.clone(),
            None if options.create() => {
                let data = Rc::new(RefCell::new(Vec::new()));
                files.insert(name.into(), data.clone());
                data
            }
            None => {
                return Err(VfsError::new(
                    VfsErrorCode::CantOpen,
                    format!("{name} not found").into(),
                ))
            }
        };
        Ok(OpenedFile {
            file: MemFile {
                data,
                read_only: options.access() == OpenAccess::ReadOnly,
            },
            access: options.access(),
        })
    }

    /// Called by `xAccess`
    ///
    /// Check if the file already exists, which will affect the behavior of opening the db
    fn access(app_data: &MemAppData, file: &str, _mode: AccessMode) -> VfsResult<bool> {
        Ok(app_data.files.borrow().contains_key(file))
    }

    fn full_pathname(_data: &MemAppData, name: &str) -> VfsResult<String> {
        Ok(name.into())
    }

    /// Called by `xDelete` and `xClose`
    ///
    /// Delete files, often used in temporary db
    fn delete_file(app_data: &MemAppData, file: &str, _sync_dir: bool) -> VfsResult<()> {
        app_data.files.borrow_mut().remove(file);
        Ok(())
    }

    /// Consume the handle on close; never delete a replacement with the same name.
    fn close_file(
        app_data: &MemAppData,
        name: Option<&str>,
        file: MemFile,
        options: OpenOptions,
    ) -> VfsResult<()> {
        let Some(name) = name else {
            return Ok(());
        };
        if options.delete_on_close() {
            let mut files = app_data.files.borrow_mut();
            if files
                .get(name)
                .is_some_and(|data| Rc::ptr_eq(data, &file.data))
            {
                files.remove(name);
            }
        }
        Ok(())
    }
}

/// Our io methods
struct MemIoMethods;

/// Implementing the io methods is very simple, just like this:
impl SQLiteIoMethods for MemIoMethods {
    type Store = MemFileStore;

    const VERSION: ::std::os::raw::c_int = 1;
}

/// Our vfs
struct MemVfs;

/// Implementing vfs is just as simple, just like this
impl SQLiteVfs<MemIoMethods> for MemVfs {
    type Os = sqlite_wasm_rs::WasmOsCallback;
    fn os(_: &MemAppData) -> &Self::Os {
        &sqlite_wasm_rs::WasmOsCallback
    }

    const VERSION: ::std::os::raw::c_int = 1;

    // As above, you can still override the default implementation
}

#[wasm_bindgen_test]
pub fn main() {
    // Register VFS to sqlite and register it as the default VFS, enjoy
    // SAFETY: MemVfs uses the default constructor and file layout. MemIoMethods
    // and MemFileStore agree on MemAppData, retained by registration. This
    // example only uses the VFS from this thread.
    let vfs = unsafe {
        register_vfs::<MemIoMethods, MemVfs>(
            "i_am_simply_implementing_a_mem_vfs",
            MemAppData::default(),
            true,
        )
    }
    .unwrap();

    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"test.db".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

    let sql = c"CREATE TABLE notes (text TEXT); INSERT INTO notes VALUES ('Hello VFS');";
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);

    unsafe {
        assert_eq!(sqlite3_close(db), SQLITE_OK);
    }
    // SAFETY: The only connection was closed, and no callbacks, borrowed app
    // data or external VFS pointers remain in use on this single thread.
    unsafe {
        vfs.unregister().unwrap();
    }
}
