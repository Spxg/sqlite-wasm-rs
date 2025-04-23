//! Memory VFS, used as the default VFS

use crate::libsqlite3::*;
use crate::vfs::utils::{
    check_db_and_page_size, check_import_db, page_read, ImportDbError, MemLinearFile,
    SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError, VfsFile, VfsResult, VfsStore,
};

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::collections::HashMap;

type Result<T> = std::result::Result<T, MemVfsError>;

type MemAppData = RwLock<HashMap<String, MemFile>>;

#[derive(Default)]
struct MemPageFile {
    pages: HashMap<usize, Vec<u8>>,
    file_size: usize,
    page_size: usize,
}

impl VfsFile for MemPageFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        Ok(page_read(
            buf,
            self.page_size,
            self.file_size,
            offset,
            |addr| self.pages.get(&addr),
            |page, buf, (start, end)| {
                buf.copy_from_slice(&page[start..end]);
            },
        ))
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        let page_size = buf.len();

        for fill in (self.file_size..offset).step_by(page_size) {
            self.pages.insert(fill, vec![0; page_size]);
        }
        if let Some(buffer) = self.pages.get_mut(&offset) {
            buffer.copy_from_slice(buf);
        } else {
            self.pages.insert(offset, buf.to_vec());
        }

        self.page_size = page_size;
        self.file_size = self.file_size.max(offset + page_size);

        Ok(())
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        for offset in size..self.file_size {
            self.pages.remove(&offset);
        }
        self.file_size = size;
        Ok(())
    }

    fn flush(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn size(&self) -> VfsResult<usize> {
        Ok(self.file_size)
    }
}

enum MemFile {
    Main(MemPageFile),
    Temp(MemLinearFile),
}

impl MemFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemLinearFile::default())
        } else {
            Self::Main(MemPageFile::default())
        }
    }
}

impl VfsFile for MemFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> VfsResult<i32> {
        match self {
            MemFile::Main(mem_page_file) => mem_page_file.read(buf, offset),
            MemFile::Temp(mem_linear_file) => mem_linear_file.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) -> VfsResult<()> {
        match self {
            MemFile::Main(mem_page_file) => mem_page_file.write(buf, offset),
            MemFile::Temp(mem_linear_file) => mem_linear_file.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) -> VfsResult<()> {
        match self {
            MemFile::Main(mem_page_file) => mem_page_file.truncate(size),
            MemFile::Temp(mem_linear_file) => mem_linear_file.truncate(size),
        }
    }

    fn flush(&mut self) -> VfsResult<()> {
        match self {
            MemFile::Main(mem_page_file) => mem_page_file.flush(),
            MemFile::Temp(mem_linear_file) => mem_linear_file.flush(),
        }
    }

    fn size(&self) -> VfsResult<usize> {
        match self {
            MemFile::Main(mem_page_file) => mem_page_file.size(),
            MemFile::Temp(mem_linear_file) => mem_linear_file.size(),
        }
    }
}

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
    *APP_DATA.get_or_init(|| unsafe { &*VfsAppData::new(MemAppData::default()).leak() })
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

impl MemVfsUtil {
    fn import_db_unchecked_impl(
        &self,
        path: &str,
        bytes: &[u8],
        page_size: usize,
        clear_wal: bool,
    ) -> Result<()> {
        check_db_and_page_size(bytes.len(), page_size)?;

        if self.exists(path) {
            return Err(MemVfsError::Generic(format!("{path} file already exists")));
        }

        let mut pages: HashMap<usize, Vec<u8>> = bytes
            .chunks(page_size)
            .enumerate()
            .map(|(idx, buffer)| (idx * page_size, buffer.to_vec()))
            .collect();

        if clear_wal {
            // header
            let header = pages.get_mut(&0).unwrap();
            header[18] = 1;
            header[19] = 1;
        }

        self.0.write().insert(
            path.into(),
            MemFile::Main(MemPageFile {
                file_size: pages.len() * page_size,
                page_size,
                pages,
            }),
        );

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
            if let MemFile::Main(file) = file {
                let file_size = file.file_size;
                let mut ret = vec![0; file.file_size];
                for (&offset, buffer) in &file.pages {
                    if offset >= file_size {
                        continue;
                    }
                    ret[offset..offset + file.page_size].copy_from_slice(buffer);
                }
                Ok(ret)
            } else {
                Err(MemVfsError::Generic(
                    "Does not support dumping temporary files".into(),
                ))
            }
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
