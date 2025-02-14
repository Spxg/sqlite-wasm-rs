# SQLite Wasm Rust

[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

Provide sqlite solution for `wasm32-unknown-unknown` target.

## Shim Usage

```toml
[dependencies]
# using precompiled library
sqlite-wasm-rs = "0.2"
```

```toml
[dependencies]
# `bundled` causes us to automatically compile and link in an up to date
#
# requires the emscripten toolchain
sqlite-wasm-rs = { version = "0.2", features = ["bundled"] }
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

## Wrapper Usage (Deprecated)

```toml
[dependencies]
sqlite-wasm-rs = { version = "0.2", default-features = false, features = ["wrapper"] }
```

Then see [`Wrapper Usage`](https://github.com/Spxg/sqlite-wasm-rs/blob/bc5285fe6d2f3a4e5eb946f5d0500fa26714f5ab/README.md#usage)

## About multithreading

This library is not thread-safe:

* `JsValue` is not cross-threaded, see [`Ensure that JsValue isn't considered Send`](https://github.com/rustwasm/wasm-bindgen/pull/955) for details.
* sqlite is compiled with `-DSQLITE_THREADSAFE=0`

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

As mentioned above, sqlite is now directly linked to emscripten's libc. But we provide the ability to customize libc.

Cargo provides a [`links`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-links-field) field that can be used to specify which library to link to.

We created a new [`sqlite-wasm-libc`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-libc) library with no implementation and only a `links = "libc"` configuration.

Then with the help of [`Overriding Build Scripts`](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts), you can overriding its configuration in your crate and link sqlite to your custom libc.

More see [`custom-libc example`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/custom-libc).

## Why provide precompiled library

In the `shim` feature, since `wasm32-unknown-unknown` does not have libc, emscripten is used here for compilation, otherwise we need to copy a bunch of c headers required for sqlite3 compilation, which is a bit of a hack for me. If sqlite3 is compiled at compile time, the emscripten toolchain is required, and we cannot assume that all users have it installed. (Believe me, because rust mainly supports the `wasm32-unknown-unknown` target, most people do not have the emscripten toolchain). Considering that wasm is cross-platform, vendor compilation products are acceptable.

About security issues:

* You can specify the bundled feature to compile sqlite locally, which requires the emscripten toolchain.
* Currently all precompiled products are compiled and committed through Github Actions, which can be tracked, downloaded and compared.

Precompile workflow: <https://github.com/Spxg/sqlite-wasm-rs/blob/master/.github/workflows/precompile.yml>

Change History: <https://github.com/Spxg/sqlite-wasm-rs/commits/master/sqlite-wasm-rs/library>

Actions: <https://github.com/Spxg/sqlite-wasm-rs/actions?query=event%3Aworkflow_dispatch>

## Why vendor sqlite-wasm

* sqlite-wasm wrap some codes that are very convenient for JS, but difficult to use for rust.
* Some sqlite C-API are not exported.
* Compiling sqlite.wasm requires the entire sqlite source code and the emscripten toolchain, which are expensive to introduce into build.rs, see <https://sqlite.org/wasm/doc/trunk/building.md>

Change history: <https://github.com/Spxg/sqlite>

## Related Project

* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
