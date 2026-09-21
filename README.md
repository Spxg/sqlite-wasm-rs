[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

`wasm32-unknown-unknown` bindings to the libsqlite3 library.

## Usage

```toml
[dependencies]
sqlite-wasm-rs = { version = "0.6", features = ["wasm-bindgen"] }
```

```toml
[dependencies]
# Encryption is supported by SQLite3MultipleCiphers
# See <https://utelle.github.io/SQLite3MultipleCiphers>
sqlite-wasm-rs = { version = "0.6", features = ["wasm-bindgen", "sqlite3mc"] }
```

```rust
use sqlite_wasm_rs as ffi;

fn open_db() {
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
    assert_eq!(unsafe { ffi::sqlite3_close(db) }, ffi::SQLITE_OK);
}
```

## About VFS

```toml
[dependencies]
sqlite-wasm-vfs = { version = "0.3", features = ["sahpool"] }
```

The following vfs have been implemented:

* [`memory`](./crates/rsqlite-vfs/src/memvfs.rs): as the default vfs, no additional conditions are required, store the database in memory.
* [`sahpool`](./crates/sqlite-wasm-vfs/src/sahpool.rs): ported from sqlite-wasm, store the database in opfs.

### How to implement a VFS

Here is an example showing how to implement a simple in-memory VFS, see [`implement-a-vfs`](./crates/rsqlite-vfs/examples/implement-a-vfs.rs) example.

```sh
cargo run -p rsqlite-vfs --example implement-a-vfs
```

## About multithreading

Multithreading is not supported, SQLite is compiled with `-DSQLITE_THREADSAFE=0`.

## Use without wasm-bindgen

No features are enabled by default, provide your own host functions. See [JS Host](./examples/host-js) or [C Host](./examples/host-c) example.

## Use custom SQLite sources

Point `SQLITE_WASM_RS_SOURCE_DIR` to your `sqlite3.c/.h` files (`sqlite3mc_amalgamation.c/.h` for `sqlite3mc`):

```sh
SQLITE_WASM_RS_SOURCE_DIR=/path/to/sqlite cargo build --target wasm32-unknown-unknown --features bindgen
```

## Minimum supported Rust version (MSRV)

The minimal officially supported rustc version is 1.81.0.

## Extensions

|Extension|About|
|-|-|
|[sqlite-vec](./extensions/sqlite-vec)|A vector search SQLite extension that runs anywhere!|

Contributions are welcome!

## Related Project

* [`diesel`](https://github.com/diesel-rs/diesel): A safe, extensible ORM and Query Builder for Rust.
* [`rusqlite`](https://github.com/rusqlite/rusqlite): Ergonomic bindings to SQLite for Rust.
* [`sqlite-wasm`](https://github.com/sqlite/sqlite-wasm): SQLite Wasm conveniently wrapped as an ES Module.
* [`sqlite-web-rs`](https://github.com/xmtp/sqlite-web-rs): A SQLite WebAssembly backend for Diesel.
* [`wa-sqlite`](https://github.com/rhashimoto/wa-sqlite): WebAssembly SQLite with support for browser storage extensions.
* [`SQLite3MultipleCiphers`](https://github.com/utelle/SQLite3MultipleCiphers): SQLite3 encryption extension with support for multiple ciphers.

## Friends

- [moli](https://github.com/lexmount/moli) - Best browser for AI Agent, written in pure Rust.
