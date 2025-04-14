//! Memory VFS, used as the default VFS

use crate::libsqlite3::*;
use crate::vfs::utils::{
    MemLinearStore, MemPageStore, SQLiteIoMethods, SQLiteVfs, SQLiteVfsFile, StoreControl,
    VfsAppData, VfsStore,
};

use parking_lot::RwLock;
use std::collections::HashMap;

type MemAppData = RwLock<HashMap<String, MemFile>>;

enum MemFile {
    Main(MemPageStore),
    Temp(MemLinearStore),
}

impl MemFile {
    fn new(flags: i32) -> Self {
        if flags & SQLITE_OPEN_MAIN_DB == 0 {
            Self::Temp(MemLinearStore::default())
        } else {
            Self::Main(MemPageStore::default())
        }
    }
}

impl VfsStore for MemFile {
    fn read(&self, buf: &mut [u8], offset: usize) -> i32 {
        match self {
            MemFile::Main(mem_page_store) => mem_page_store.read(buf, offset),
            MemFile::Temp(mem_linear_store) => mem_linear_store.read(buf, offset),
        }
    }

    fn write(&mut self, buf: &[u8], offset: usize) {
        match self {
            MemFile::Main(mem_page_store) => mem_page_store.write(buf, offset),
            MemFile::Temp(mem_linear_store) => mem_linear_store.write(buf, offset),
        }
    }

    fn truncate(&mut self, size: usize) {
        match self {
            MemFile::Main(mem_page_store) => mem_page_store.truncate(size),
            MemFile::Temp(mem_linear_store) => mem_linear_store.truncate(size),
        }
    }

    fn size(&self) -> usize {
        match self {
            MemFile::Main(mem_page_store) => mem_page_store.size(),
            MemFile::Temp(mem_linear_store) => mem_linear_store.size(),
        }
    }
}

struct MemStoreControl;

impl StoreControl<MemFile, MemAppData> for MemStoreControl {
    fn add_file(vfs: *mut sqlite3_vfs, file: &str, flags: i32) {
        unsafe {
            Self::app_data(vfs)
                .write()
                .insert(file.into(), MemFile::new(flags));
        }
    }

    fn contains_file(vfs: *mut sqlite3_vfs, file: &str) -> bool {
        unsafe { Self::app_data(vfs).read().contains_key(file) }
    }

    fn delete_file(vfs: *mut sqlite3_vfs, file: &str) -> Option<MemFile> {
        unsafe { Self::app_data(vfs).write().remove(file) }
    }

    fn with_file<F: Fn(&MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32> {
        Some(unsafe { f(Self::app_data(vfs_file.vfs).read().get(vfs_file.name())?) })
    }

    fn with_file_mut<F: Fn(&mut MemFile) -> i32>(vfs_file: &SQLiteVfsFile, f: F) -> Option<i32> {
        Some(unsafe {
            f(Self::app_data(vfs_file.vfs)
                .write()
                .get_mut(vfs_file.name())?)
        })
    }
}

struct MemIoMethods;

impl SQLiteIoMethods for MemIoMethods {
    type Store = MemFile;
    type AppData = MemAppData;
    type StoreControl = MemStoreControl;

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
