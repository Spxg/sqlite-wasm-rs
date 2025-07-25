//! Memory VFS, used as the default VFS

use crate::libsqlite3::*;
use crate::vfs::utils::{
    check_import_db, ImportDbError, MemChunksFile, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile,
    VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::collections::HashMap;

type Result<T> = std::result::Result<T, MemVfsError>;

enum MemFile {
    Main(MemChunksFile),
    Temp(MemChunksFile),
}

impl MemFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemChunksFile::default())
        } else {
            Self::Main(MemChunksFile::waiting_for_write())
        }
    }

    fn file(&self) -> &MemChunksFile {
        let (MemFile::Main(file) | MemFile::Temp(file)) = self;
        file
    }

    fn file_mut(&mut self) -> &mut MemChunksFile {
        let (MemFile::Main(file) | MemFile::Temp(file)) = self;
        file
    }
}

impl VfsFile for MemFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<bool> {
        self.file().read(buf, offset)
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        self.file_mut().write(buf, offset)
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        self.file_mut().truncate(size)
    }

    fn flush(&mut self) -> VfsResult<()> {
        self.file_mut().flush()
    }

    fn size(&self) -> VfsResult<usize> {
        self.file().size()
    }
}

type MemAppData = RwLock<HashMap<String, MemFile>>;

struct MemStore;

impl VfsStore<MemFile, MemAppData> for MemStore {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) -> VfsResult<()> {
        let app_data = unsafe { Self::app_data(vfs) };
        app_data.write().insert(file.into(), MemFile::new(flags));
        Ok(())
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<bool> {
        let app_data = unsafe { Self::app_data(vfs) };
        Ok(app_data.read().contains_key(file))
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<()> {
        let app_data = unsafe { Self::app_data(vfs) };
        if app_data.write().remove(file).is_none() {
            return Err(VfsError::new(
                SQLITE_IOERR_DELETE,
                format!("{file} not found"),
            ));
        }
        Ok(())
    }

    fn with_file<F: Fn(&MemFile) -> VfsResult<i32>>(
        vfs_file: &SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.read().get(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut MemFile) -> VfsResult<i32>>(
        vfs_file: &SQLiteVfsFile,
        f: F,
    ) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.write().get_mut(name) {
            Some(file) => f(file),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }
}

struct MemIoMethods;

impl SQLiteIoMethods for MemIoMethods {
    type File = MemFile;
    type AppData = MemAppData;
    type Store = MemStore;

    const VERSION: ::std::os::raw::c_int = 1;
}

struct MemVfs;

impl SQLiteVfs<MemIoMethods> for MemVfs {
    const VERSION: ::std::os::raw::c_int = 1;
}

static APP_DATA: OnceCell<&'static VfsAppData<MemAppData>> = OnceCell::new();

fn app_data() -> &'static VfsAppData<MemAppData> {
    APP_DATA.get_or_init(|| unsafe { &*VfsAppData::new(MemAppData::default()).leak() })
}

#[derive(thiserror::Error, Debug)]
pub enum MemVfsError {
    #[error(transparent)]
    ImportDb(#[from] ImportDbError),
    #[error("Generic error: {0}")]
    Generic(String),
}

/// MemVfs management tools exposed to clients.
pub struct MemVfsUtil(&'static VfsAppData<MemAppData>);

impl Default for MemVfsUtil {
    fn default() -> Self {
        MemVfsUtil::new()
    }
}

impl MemVfsUtil {
    /// Get management tool
    pub fn new() -> Self {
        MemVfsUtil(app_data())
    }
}

impl MemVfsUtil {
    fn import_db_unchecked_impl(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<()> {
        if self.exists(filename) {
            return Err(MemVfsError::Generic(format!(
                "{filename} file already exists"
            )));
        }

        self.0.write().insert(filename.into(), {
            let mut file = MemFile::Main(MemChunksFile::new(page_size));
            file.write(bytes, 0).unwrap();
            if clear_wal {
                file.write(&[1, 1], 18).unwrap();
            }
            file
        });

        Ok(())
    }

    /// Import the database.
    ///
    /// If the database is imported with WAL mode enabled,
    /// it will be forced to write back to legacy mode, see
    /// <https://sqlite.org/forum/forumpost/67882c5b04>
    ///
    /// If the imported database is encrypted, use `import_db_unchecked` instead.
    pub fn import_db(&self, filename: &str, bytes: &[u8]) -> Result<()> {
        let page_size = check_import_db(bytes)?;
        self.import_db_unchecked_impl(filename, bytes, page_size, true)
    }

    /// `import_db` without checking, can be used to import encrypted database.
    pub fn import_db_unchecked(
        &self,
        filename: &str,
        bytes: &[u8],
        page_size: usize,
    ) -> Result<()> {
        self.import_db_unchecked_impl(filename, bytes, page_size, false)
    }

    /// Export the database.
    pub fn export_db(&self, filename: &str) -> Result<Vec<u8>> {
        let name2file = self.0.read();

        if let Some(file) = name2file.get(filename) {
            let file_size = file.size().unwrap();
            let mut ret = vec![0; file_size];
            file.read(&mut ret, 0).unwrap();
            Ok(ret)
        } else {
            Err(MemVfsError::Generic(
                "The file to be exported does not exist".into(),
            ))
        }
    }

    /// Delete the specified database, please make sure that the database is closed.
    pub fn delete_db(&self, filename: &str) {
        self.0.write().remove(filename);
    }

    /// Delete all database, please make sure that all database is closed.
    pub fn clear_all(&self) {
        std::mem::take(&mut *self.0.write());
    }

    /// Does the database exists.
    pub fn exists(&self, filename: &str) -> bool {
        self.0.read().contains_key(filename)
    }

    /// List all files.
    pub fn list(&self) -> Vec<String> {
        self.0.read().keys().cloned().collect()
    }

    /// Number of files.
    pub fn count(&self) -> usize {
        self.0.read().len()
    }
}

pub(crate) fn install() -> ::std::os::raw::c_int {
    let app_data = app_data();
    let vfs = Box::leak(Box::new(MemVfs::vfs(
        c"memvfs".as_ptr().cast(),
        app_data as *const _ as *mut _,
    )));
    unsafe { sqlite3_vfs_register(vfs, 1) }
}

#[cfg(test)]
mod tests {
    use crate::{
        mem_vfs::{MemAppData, MemFile, MemStore},
        utils::{test_suite::test_vfs_store, VfsAppData},
    };
    use wasm_bindgen_test::wasm_bindgen_test;

    #[wasm_bindgen_test]
    fn test_memory_vfs_store() {
        test_vfs_store::<MemAppData, MemFile, MemStore>(VfsAppData::new(MemAppData::default()))
            .unwrap();
    }
}
