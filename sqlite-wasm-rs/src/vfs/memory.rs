//! Memory VFS, used as the default VFS

use crate::libsqlite3::*;
use crate::vfs::utils::{
    page_read, MemLinearFile, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, VfsAppData, VfsError,
    VfsFile, VfsResult, VfsStore,
};

use parking_lot::RwLock;
use std::collections::HashMap;

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
        unsafe {
            Self::app_data(vfs)
                .write()
                .insert(file.into(), MemFile::new(flags));
        }
        Ok(())
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> bool {
        unsafe { Self::app_data(vfs).read().contains_key(file) }
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> VfsResult<MemFile> {
        unsafe {
            match Self::app_data(vfs).write().remove(file) {
                Some(file) => Ok(file),
                None => Err(VfsError::new(
                    SQLITE_IOERR_DELETE,
                    format!("{file} not found"),
                )),
            }
        }
    }

    fn with_file<F: Fn(&MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        Ok(unsafe {
            let name = vfs_file.name();
            match Self::app_data(vfs_file.vfs).read().get(name) {
                Some(file) => f(file),
                None => return Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
            }
        })
    }

    fn with_file_mut<F: Fn(&mut MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> VfsResult<i32> {
        Ok(unsafe {
            let name = vfs_file.name();
            match Self::app_data(vfs_file.vfs)
                .write()
                .get_mut(vfs_file.name())
            {
                Some(file) => f(file),
                None => return Err(VfsError::new(SQLITE_IOERR, format!("{name} not found"))),
            }
        })
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

pub fn install() -> ::std::os::raw::c_int {
    unsafe {
        sqlite3_vfs_register(
            Box::leak(Box::new(MemVfs::vfs(
                c"memvfs".as_ptr().cast(),
                VfsAppData::new(MemAppData::default()).leak().cast(),
            ))),
            1,
        )
    }
}
