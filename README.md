# SQLite Wasm Rust

[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

Provide sqlite solution for `wasm32-unknown-unknown` target.

## Shim Usage

```toml
[dependencies]
sqlite-wasm-rs = "0.2"
```

```rust
use sqlite_wasm_rs::export::{self as ffi, install_opfs_sahpool};

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

Then see [`Wrapper Usage`](https://github.com/Spxg/sqlite-wasm-rs/blob/bc5285fe6d2f3a4e5eb946f5d0500fa26714f5ab/README.md#usage)

## Multithreading

When `target-feature=+atomics` is enabled, `sqlite-wasm-rs` support multithreading, see [`multithread example`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/multithreading).

## Shim VS Wrapper

### Shim

Provides the highest performance by linking to sqlite3.

The following vfs have been implemented:

* [`memory-vfs`](https://github.com/Spxg/sqlite-wasm-rs/blob/master/sqlite-wasm-rs/src/shim/vfs/memory.rs): as the default vfs, no additional conditions are required, just use.
* [`opfs-sahpool`](https://github.com/Spxg/sqlite-wasm-rs/blob/master/sqlite-wasm-rs/src/shim/vfs/sahpool.rs): ported from sqlite-wasm, it provides the best performance persistent storage method.

See <https://github.com/Spxg/sqlite-wasm-rs/blob/master/VFS.md>

### Wrapper

Wrap the official [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm), and expect to provide a usable C-like API. There are a variety of official persistent VFS implementations to choose from. (memvfs, opfs, opfs-sahpool, kvvfs).

## Use external libc (shim only)

As mentioned below, sqlite is now directly linked to emscripten's libc. But we provide the ability to customize libc.

Cargo provides a [`links`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-links-field) field that can be used to specify which library to link to.

We created a new [`sqlite-wasm-libc`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-libc) library with no implementation and only a `links = "libc"` configuration.

Then with the help of [`Overriding Build Scripts`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts), you can overriding its configuration in your crate and link sqlite to your custom libc.

More see [`custom-libc example`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/custom-libc).

## Why vendor sqlite-wasm

* sqlite-wasm wrap some codes that are very convenient for JS, but difficult to use for rust.
* Some sqlite C-API are not exported.

Change history: <https://github.com/Spxg/sqlite>

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
