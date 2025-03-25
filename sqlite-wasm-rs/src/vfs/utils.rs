use crate::libsqlite3::{sqlite3_file, sqlite3_vfs};
use fragile::Fragile;
use js_sys::{Math, Number};
use std::ops::{Deref, DerefMut};

/// Wrap the pVfs pointer, which is often used in VFS implementation.
///
/// Use vfs pointer as the map key to find the corresponding vfs handle, such as `OpfsSAHPool`.
///
/// It is safe to implement `Send` and `Sync` for it, for internal use only.
#[derive(Hash, PartialEq, Eq)]
pub struct VfsPtr(pub *mut sqlite3_vfs);

unsafe impl Send for VfsPtr {}
unsafe impl Sync for VfsPtr {}

/// Wrap the pFile pointer, which is often used in VFS implementation.
///
/// Use file pointer as the map key to find the corresponding file handle, such as `MemFile`.
///
/// It is safe to implement `Send` and `Sync` for it, for internal use only.
#[derive(Hash, PartialEq, Eq)]
pub struct FilePtr(pub *mut sqlite3_file);

unsafe impl Send for FilePtr {}
unsafe impl Sync for FilePtr {}

/// A [`FragileComfirmed<T>`] wraps a non sendable `T` to be safely send to other threads.
///
/// Once the value has been wrapped it can be sent to other threads but access
/// to the value on those threads will fail.
pub struct FragileComfirmed<T> {
    fragile: Fragile<T>,
}

unsafe impl<T> Send for FragileComfirmed<T> {}
unsafe impl<T> Sync for FragileComfirmed<T> {}

impl<T> FragileComfirmed<T> {
    pub fn new(t: T) -> Self {
        FragileComfirmed {
            fragile: Fragile::new(t),
        }
    }
}

impl<T> Deref for FragileComfirmed<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.fragile.get()
    }
}

impl<T> DerefMut for FragileComfirmed<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.fragile.get_mut()
    }
}

/// get random name if zFileName is null and other cases
pub fn get_random_name() -> String {
    let random = Number::from(Math::random()).to_string(36).unwrap();
    random.slice(2, random.length()).as_string().unwrap()
}
