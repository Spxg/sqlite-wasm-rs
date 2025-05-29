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
            Self::Temp(MemChunksFile::new(512))
        } else {
            Self::Main(MemChunksFile::waiting_for_write())
        }
    }
}

impl VfsFile for MemFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        match self {
            MemFile::Main(file) => file.read(buf, offset),
            MemFile::Temp(file) => file.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        match self {
            MemFile::Main(file) => file.write(buf, offset),
            MemFile::Temp(file) => file.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        match self {
            MemFile::Main(file) => file.truncate(size),
            MemFile::Temp(file) => file.truncate(size),
        }
    }

    fn flush(&mut self) -> VfsResult<()> {
        match self {
            MemFile::Main(file) => file.flush(),
            MemFile::Temp(file) => file.flush(),
        }
    }

    fn size(&self) -> VfsResult<usize> {
        match self {
            MemFile::Main(file) => file.size(),
            MemFile::Temp(file) => file.size(),
        }
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

    fn with_file<F: Fn(&MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.read().get(name) {
            Some(file) => Ok(f(file)),
            None => Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
        }
    }

    fn with_file_mut<F: Fn(&mut MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        let name = unsafe { vfs_file.name() };
        let app_data = unsafe { Self::app_data(vfs_file.vfs) };
        match app_data.write().get_mut(name) {
            Some(file) => Ok(f(file)),
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
    fn import_db_unchecked_impl(
        &self,
        path: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<()> {
        if self.exists(path) {
            return Err(MemVfsError::Generic(format!("{path} file already exists")));
        }

        self.0.write().insert(path.into(), {
            let mut file = MemFile::Main(MemChunksFile::new(page_size));
            file.write(bytes, 0).unwrap();
            if clear_wal {
                file.write(&[1, 1], 18).unwrap();
            }
            file
        });

        Ok(())
    }

    /// Get management tool
    pub fn new() -> Self {
        MemVfsUtil(app_data())
    }

    /// Import the db file
    ///
    /// If the database is imported with WAL mode enabled,
    /// it will be forced to write back to legacy mode, see
    /// <https://sqlite.org/forum/forumpost/67882c5b04>
    ///
    /// If the imported DB is encrypted, use `import_db_unchecked` instead.
    pub fn import_db(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let page_size = check_import_db(bytes)?;
        self.import_db_unchecked_impl(path, bytes, page_size, true)
    }

    /// Can be used to import encrypted DB
    pub fn import_db_unchecked(&self, path: &str, bytes: &[u8], page_size: usize) -> Result<()> {
        self.import_db_unchecked_impl(path, bytes, page_size, false)
    }

    /// Export database
    pub fn export_db(&self, name: &str) -> Result<Vec<u8>> {
        let name2file = self.0.read();

        if let Some(file) = name2file.get(name) {
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

    /// Delete the specified db, please make sure that the db is closed.
    pub fn delete_db(&self, name: &str) {
        self.0.write().remove(name);
    }

    /// Delete all dbs, please make sure that all dbs is closed.
    pub fn clear_all(&self) {
        std::mem::take(&mut *self.0.write());
    }

    /// Does the DB exist.
    pub fn exists(&self, file: &str) -> bool {
        self.0.read().contains_key(file)
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
