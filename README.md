# SQLite Wasm Rust

[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

Provide sqlite solution for `wasm32-unknown-unknown` target.

## Usage

```toml
[dependencies]
# Using `bundled` default feature causes us to automatically compile
# and link in an up to date, requires the emscripten toolchain.
sqlite-wasm-rs = "0.4"
```

```toml
[dependencies]
# If you don't have the emscripten toolchain, you can use the `precompiled` feature.
sqlite-wasm-rs = { version = "0.4", default-features = false, features = ["precompiled"] }
```

```toml
[dependencies]
# Encryption is supported by SQLite3MultipleCiphers, need to enable the bundled feature.
# See <https://utelle.github.io/SQLite3MultipleCiphers>
sqlite-wasm-rs = { version = "0.4", features = ["sqlite3mc"] }
```

```rust
use sqlite_wasm_rs::{
    self as ffi,
    sahpool_vfs::{install as install_opfs_sahpool, OpfsSAHPoolCfg},
};

async fn open_db() {
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
    install_opfs_sahpool(&OpfsSAHPoolCfg::default(), true)
        .await
        .unwrap();

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

## About VFS

The following vfs have been implemented:

* [`memory`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/src/vfs/memory.rs): as the default vfs, no additional conditions are required, store the database in memory.
* [`sahpool`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/src/vfs/sahpool.rs): ported from sqlite-wasm, store the database in opfs.
* [`relaxed-idb`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/src/vfs/relaxed_idb.rs): store the database in blocks in indexed db.

Go to [`here`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-rs/src/vfs/README.md) to check it out.

## About multithreading

This library is not thread-safe:

* `JsValue` is not cross-threaded, see <https://github.com/rustwasm/wasm-bindgen/pull/955> for details.
* sqlite is compiled with `-DSQLITE_THREADSAFE=0`

## Precompiled libsqlite3.a

Since `wasm32-unknown-unknown` does not have libc, emscripten is used here for compilation, otherwise we need to copy a bunch of c headers required for sqlite3 compilation. If sqlite3 is compiled at compile time, the emscripten toolchain is required, and we cannot assume that all users have it installed. (Believe me, because rust mainly supports the `wasm32-unknown-unknown` target, most people do not have the emscripten toolchain). Considering that wasm is cross-platform, vendor compilation products are acceptable.

About security:

* You can specify the bundled feature to compile sqlite locally, which requires the emscripten toolchain.
* Currently all precompiled products are compiled and committed through Github Actions, which can be tracked, downloaded and compared.

[Precompile Workflow](https://github.com/Spxg/sqlite-wasm-rs/blob/master/.github/workflows/precompile.yml) | [Change History](https://github.com/Spxg/sqlite-wasm-rs/commits/master/sqlite-wasm-rs/sqlite3) | [Actions](https://github.com/Spxg/sqlite-wasm-rs/actions?query=event%3Aworkflow_dispatch)

## Use external libc

As mentioned above, sqlite is now directly linked to emscripten's libc, but we provide the ability to customize libc, cargo provides a [`links`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-links-field) field that can be used to specify which library to link to.

We created a new [`sqlite-wasm-libc`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/sqlite-wasm-libc) library with no implementation and only a `links = "libc"` configuration, and then with the help of [overriding build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts), you can overriding its configuration in your crate and link sqlite to your custom libc.

More see [`custom-libc`](https://github.com/Spxg/sqlite-wasm-rs/tree/master/examples/custom-libc) example.

## Minimum supported Rust version (MSRV)

The minimal officially supported rustc version is 1.78.0.

## Nice Try

* [`diesel`](https://github.com/diesel-rs/diesel): A safe, extensible ORM and Query Builder for Rust.
* [`sqlight`](https://github.com/Spxg/sqlight): A SQLite Playground that runs completely locally.

## Related Project

* [`diesel`](https://github.com/diesel-rs/diesel): A safe, extensible ORM and Query Builder for Rust.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`wa-sqlite`](https://github.com/rhashimoto/wa-sqlite): WebAssembly SQLite with support for browser storage extensions.
* [`SQLite3MultipleCiphers`](https://github.com/utelle/SQLite3MultipleCiphers): SQLite3 encryption extension with support for multiple ciphers.
