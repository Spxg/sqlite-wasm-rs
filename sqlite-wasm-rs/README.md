# SQLite Wasm Rust

Wrap the official [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm), and expect to provide a usable C-like API.

## Usage

```rust
use sqlite_wasm_rs::export as ffi;
use std::ffi::CString;

async fn open_db() -> anyhow::Result<()> {
    // Before using CAPI, you must initialize sqlite.
    //
    // Initializing the database is a one-time operation during
    // the life of the program.
    let sqlite = ffi::init_sqlite().await?;

    let mut db = std::ptr::null_mut();
    let filename = CString::new("mydb").unwrap();
    // Persistent Storage is supported, use opfs vfs.
    // This support is only available when sqlite is loaded from a
    // Worker thread, whether it's loaded in its own dedicated worker
    // or in a worker together with client code.
    //
    // See <https://sqlite.org/wasm/doc/trunk/persistence.md#opfs>
    let vfs = CString::new("opfs").unwrap();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            // Using std::ptr::null() is a memory DB
            vfs.as_ptr(),
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);

    // support `opfs-sahpool` vfs
    //
    // See <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool>
    sqlite.install_opfs_sahpool(None).await?;

    let mut db = std::ptr::null_mut();
    let filename = CString::new("mydb").unwrap();
    let vfs = CString::new("opfs-sahpool").unwrap();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            filename.as_ptr(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            vfs.as_ptr(),
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);

    Ok(())
}
```


## Why vendor sqlite-wasm

* sqlite-wasm wrap some codes that are very convenient for JS, but difficult to use for rust.
* Some sqlite C-API are not exported.

Change history: <https://github.com/Spxg/sqlite>

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
