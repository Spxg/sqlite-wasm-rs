//! This is a tutorial on implementing VFS
//!
//! We want to implement a simple memory VFS

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

use parking_lot::RwLock;
use sqlite_wasm_rs::{
    sqlite3_close, sqlite3_exec, sqlite3_file, sqlite3_open_v2, sqlite3_vfs,
    utils::{
        register_vfs, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsFile, VfsResult, VfsStore,
    },
    SQLITE_IOERR_SHORT_READ, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE,
};
use std::collections::HashMap;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

/// Our data is simply stored in memory, and we can use Vec<u8>
#[derive(Default)]
struct MemFile(Vec<u8>);

/// Some basic capabilities of file
impl VfsFile for MemFile {
    /// Called by `xRead`
    ///
    /// We copy the data starting at offset in the memory file to buffer,
    /// and if it cannot be read completely, `SQLITE_IOERR_SHORT_READ` is returned.
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        let size = buf.len();
        let end = size + offset;
        if self.0.len() <= offset {
            buf.fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }

        let read_end = end.min(self.0.len());
        let read_size = read_end - offset;
        buf[..read_size].copy_from_slice(&self.0[offset..read_end]);

        if read_size < size {
            buf[read_size..].fill(0);
            return Ok(SQLITE_IOERR_SHORT_READ);
        }
        Ok(SQLITE_OK)
    }

    /// Called by `xWrite`
    ///
    /// We copy the data in the buffer to the memory file,
    /// and if the size is not enough, expand it.
    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let end = buf.len() + offset;
        if end > self.0.len() {
            self.0.resize(end, 0);
        }
        self.0[offset..end].copy_from_slice(buf);
        Ok(())
    }

    /// Called by `xTruncate`
    ///
    /// Truncate the memory file, which happens during vacuum
    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        self.0.truncate(size);
        Ok(())
    }

    /// Called by `xSync`
    ///
    /// Write the data back to "disk".
    ///
    /// Since we are in memory, the write operation takes effect immediately,
    /// so we return directly.
    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    /// Called by `xFileSize`
    ///
    /// Get the memory file size
    fn size(&self) -> VfsResult<usize> {
        Ok(self.0.len())
    }
}

/// This is where we store our data.
///
/// Since we will have multiple different databases,
/// we use hashmap to store the data with the file name as the key.
type MemAppData = RwLock<HashMap<String, MemFile>>;

/// Something that manages our memory files
struct MemFileStore;

/// Make changes to files
impl VfsStore<MemFile, MemAppData> for MemFileStore {
    /// Called by `xOpen`
    ///
    /// Add the file to the store
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        console_log!("add_file() called, vfs: {vfs:?} file: {file} flags: {flags}");
        let app_data = unsafe { Self::app_data(vfs) };
        app_data
            .write()
            .insert(file.to_string(), MemFile::default());
        Ok(())
    }

    /// Called by `xOpen` and `xAccess`
    ///
    /// Check if the file already exists, which will affect the behavior of opening the db
    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        console_log!("contains_file() called, vfs: {vfs:?} file: {file}");
        let app_data = unsafe { Self::app_data(vfs) };
        Ok(app_data.read().contains_key(file))
    }

    /// Called by `xDelete` and `xClose`
    ///
    /// Delete files, often used in temporary db
    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        console_log!("delete_file() called, vfs: {vfs:?} file: {file}");
        let app_data = unsafe { Self::app_data(vfs) };
        app_data.write().remove(file);
        Ok(())
    }

    /// Called by `xRead` and `xFileSize`
    ///
    /// Read the file contents
    fn with_file<F: Fn(&MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        let name = unsafe { vfs_file.name() };
        console_log!("with_file() called, vfs: {:?} file: {}", vfs_file.vfs, name);
        Ok(f(app_data.read().get(name).unwrap()))
    }

    /// Called by `xWrite`, `xTruncate` and `xSync`
    ///
    /// Write the file contents
    fn with_file_mut<F: Fn(&mut MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        let name = unsafe { vfs_file.name() };
        console_log!(
            "with_file_mut() called, vfs: {:?} file: {}",
            vfs_file.vfs,
            name
        );
        Ok(f(app_data.write().get_mut(name).unwrap()))
    }
}

/// Our io methods
struct MemIoMethods;

/// Implementing the io methods is very simple, just like this:
impl SQLiteIoMethods for MemIoMethods {
    type File = MemFile;
    type AppData = MemAppData;
    type Store = MemFileStore;

    const VERSION: ::std::os::raw::c_int = 1;

    /// You can still override the default implementation
    unsafe extern "C" fn xCheckReservedLock(
        _: *mut sqlite3_file,
        #[allow(non_snake_case)] pResOut: *mut ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int {
        *pResOut = 1;
        SQLITE_OK
    }
}

/// Our vfs
struct MemVfs;

/// Implementing vfs is just as simple, just like this
impl SQLiteVfs<MemIoMethods> for MemVfs {
    const VERSION: ::std::os::raw::c_int = 1;
    // As above, you can still override the default implementation
}

#[wasm_bindgen_test]
pub fn main() {
    // Register VFS to sqlite and register it as the default VFS, enjoy
    let vfs = register_vfs::<MemIoMethods, MemVfs>(
        "i_am_simply_implementing_a_mem_vfs",
        MemAppData::default(),
        true,
    )
    .unwrap();
    console_log!("register vfs success, ptr: {vfs:?}");

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

    let sql = c"
CREATE TABLE IF NOT EXISTS employees (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    salary REAL NOT NULL
);

INSERT INTO employees (name, salary) VALUES ('Alice', 50000);
INSERT INTO employees (name, salary) VALUES ('Bob', 60000);
UPDATE employees SET salary = 55000 WHERE id = 1;
        ";
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
        sqlite3_close(db);
    }
}
