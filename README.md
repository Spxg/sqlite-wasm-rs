# SQLite Wasm Rust

[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

Provide sqlite solution for `wasm32-unknown-unknown` target.

## Shim Usage

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

Then see [`Wrapper Usage`](https://github.com/Spxg/sqlite-wasm-rs/blob/bc5285fe6d2f3a4e5eb946f5d0500fa26714f5ab/README.md#usage)

## Shim VS Wrapper

### Shim

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

## Use external libc (shim only)

As mentioned above, sqlite is now directly linked to emscripten's libc. But we provide the ability to customize libc.

Cargo provides a [`links`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-links-field) field that can be used to specify which library to link to.
We created a new [`sqlite-wasm-libc`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-libc) library with no implementation and only a links = "libc" configuration.
Then with the help of [`Overriding Build Scripts`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts), we can overriding its configuration on the upper layer and link sqlite to our custom libc

More see [`custom-libc example`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/custom-libc).


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
