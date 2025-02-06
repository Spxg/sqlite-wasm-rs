# SQLite Wasm Rust

[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

Provide sqlite solution for `wasm32-unknown-unknown` target.

## Polyfill Usage

```toml
[dependencies]
sqlite-wasm-rs = "0.2"
```

```rust
use sqlite_wasm_rs::export::*;

async fn open_db() -> anyhow::Result<()> {
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
}
```

## Wrapper Usage

```toml
[dependencies]
sqlite-wasm-rs = { version = "0.2", default-features = false, features = ["wrapper"] }
```

```rust
use sqlite_wasm_rs::export as ffi;
use std::ffi::CString;

async fn open_db() -> anyhow::Result<()> {
    // Before using CAPI, you must initialize sqlite.
    //
    // Initializing the database is a one-time operation during
    // the life of the program.
    let sqlite = ffi::init_sqlite().await?;

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

    // support `opfs-sahpool` vfs
    //
    // See <https://sqlite.org/wasm/doc/trunk/persistence.md#vfs-opfs-sahpool>
    sqlite.install_opfs_sahpool(None).await?;

    // open with opfs-sahpool vfs
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

    // Persistent Storage is supported, use opfs vfs.
    // This support is only available when sqlite is loaded from a
    // Worker thread, whether it's loaded in its own dedicated worker
    // or in a worker together with client code.
    //
    // See <https://sqlite.org/wasm/doc/trunk/persistence.md#opfs>
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        ffi::sqlite3_open_v2(
            // equal to "file:opfs.db?vfs=opfs"
            c"opfs.db".as_ptr().cast(),
            &mut db as *mut _,
            ffi::SQLITE_OPEN_READWRITE | ffi::SQLITE_OPEN_CREATE,
            c"opfs".as_ptr().cast()
        )
    };
    assert_eq!(ffi::SQLITE_OK, ret);

    Ok(())
}
```

## Polyfill VS Wrapper

### Polyfill

Compile sqlite with `-DSQLITE_OS_OTHER`, linking and implement the external functions required by `sqlite` (`malloc`, `realloc`, `sqlite3_init_os` etc..). And because the `wasm32-unknown-unknown` target does not have `libc`, string functions such as `strcmp` need to be implemented. Finally, some web platform-specific functions need to be implemented, such as time-related functions.

Given that sqlite mainly supports emscripten, linking emscripten to `wasm32-unknown-unknown` is the best approach (otherwise you need to implement some methods of `libc` yourself). But here is a question, is `wasm32-unknown-unknown` now C-ABI compatible?

The rustwasm team has done a lot of work and is now compatible with the `-Zwasm-c-abi` compiler flag, see <https://github.com/rustwasm/wasm-bindgen/issues/3454>. But it doesn't mean that there will be problems if you don't use the `-Zwasm-c-abi` flags, see <https://github.com/rustwasm/wasm-bindgen/pull/2209>. At least after testing, it works without `-Zwasm-c-abi`.

Advantages
* No wrapper for `sqlite-wasm`, providing the highest performance.
* No need for calling `init_sqlite()` before use.

Disadvantages
* Requires additional VFS implementation (currently memvfs and opfs-sahpool have been implemented).
* More time is needed to confirm whether polyfill works properly.

### Wrapper

Wrap the official [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm), and expect to provide a usable C-like API.

Advantages
* There are a variety of official persistent VFS implementations to choose from. (memvfs, opfs, opfs-sahpool, kvvfs).

Disadvantages
* Interacting with `sqlite-wam` requires memory copies and additional memory management, which can affect performance in some scenarios.
* New interfaces need to be added manually, it only wraps some commonly used C-API for now, but it is enough.
* Need for calling `init_sqlite()` before use.

## Multithreading

When `target-feature=+atomics` is enabled, `sqlite-wasm-rs` support multithreading, see [`multithread example`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/multithreading).

## Why vendor sqlite-wasm

* sqlite-wasm wrap some codes that are very convenient for JS, but difficult to use for rust.
* Some sqlite C-API are not exported.

Change history: <https://github.com/Spxg/sqlite>

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
