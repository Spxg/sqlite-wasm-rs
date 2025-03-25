# Memory db

Data is stored in memory, this is the default vfs

```rust
use sqlite_wasm_rs as ffi;

// open with memory vfs
let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        c"mem.db".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        std::ptr::null()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);
```

# Built-in memory db

SQLite officially provides a memdb vfs

```rust
use sqlite_wasm_rs as ffi;

let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        // The first character of the name must be "/"
        // or else the object will be a separate memdb object.
        c"file:/mem.db?vfs=memdb".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        std::ptr::null()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);

```

# Opfs-sahpool

Persistent vfs, ported from sqlite-wasm, see <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool> for details

```rust
use sqlite_wasm_rs::{self as ffi, install_opfs_sahpool};

// install opfs-sahpool persistent vfs and set as default vfs
install_opfs_sahpool(None, true).await?;

// open with opfs-sahpool vfs
let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        c"opfs-sahpool.db".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        std::ptr::null()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);

let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        c"file:opfs-sahpool.db?vfs=opfs-sahpool".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        std::ptr::null()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);

let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        c"opfs-sahpool.db".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        c"opfs-sahpool".as_ptr().cast()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);
```

Support custom vfs and directory

```rust
use sqlite_wasm_rs::{
    self as ffi,
    install_opfs_sahpool,
    OpfsSAHPoolCfgBuilder
};

let cfg = OpfsSAHPoolCfgBuilder::new()
    .vfs_name("custom-vfs")
    .directory("custom/abc")
    .build();
install_opfs_sahpool(Some(&cfg), true).await?;

let mut db = std::ptr::null_mut();
let ret = unsafe {
    ffi::sqlite3_open_v2(
        c"custom-vfs.db".as_ptr().cast(),
        &mut db as *mut _,
        ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
        std::ptr::null()
    )
};
assert_eq!(ffi::SQLITE_OK, ret);
```
