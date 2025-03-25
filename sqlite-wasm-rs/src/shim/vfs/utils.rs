use crate::shim::libsqlite3::{sqlite3_file, sqlite3_vfs};
use js_sys::{Math, Number};

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

/// get random name if zFileName is null and other cases
pub fn get_random_name() -> String {
    let random = Number::from(Math::random()).to_string(36).unwrap();
    random.slice(2, random.length()).as_string().unwrap()
}
