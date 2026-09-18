[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-rs.svg)](https://crates.io/crates/sqlite-wasm-rs)

`wasm32-unknown-unknown` bindings to the libsqlite3 library, with a default
wasm-bindgen host adapter and support for custom link-time host adapters.

## Usage 

```toml
[dependencies]
sqlite-wasm-rs = "0.5"
```

```toml
[dependencies]
# Encryption is supported by SQLite3MultipleCiphers
# See <https://utelle.github.io/SQLite3MultipleCiphers>
sqlite-wasm-rs = { version = "0.5", features = ["sqlite3mc"] }
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
# It requires sqlite-wasm-rs 0.5.2 or higher to be used,
# for version 0.5.1, use version 0.1 instead.
sqlite-wasm-vfs = "0.2"
```

The OPFS SAH pool is enabled by default through the `sahpool` feature. See the
[`sqlite-wasm-vfs` feature documentation](./crates/sqlite-wasm-vfs/README.md#features)
for details.

The following vfs have been implemented:

* [`memory`](./crates/rsqlite-vfs/src/memvfs.rs): as the default vfs, no additional conditions are required, store the database in memory.
* [`sahpool`](./crates/sqlite-wasm-vfs/src/sahpool.rs): ported from sqlite-wasm, store the database in opfs.

### VFS Comparison

||MemoryVFS|SyncAccessHandlePoolVFS|
|-|-|-|
|Storage|RAM|OPFS|
|Contexts|All|Dedicated Worker|
|No COOP/COEP requirements|✅|✅|

### How to implement a VFS

Here is an example showing how to use `sqlite-wasm-rs` to implement a simple in-memory VFS, see [`implement-a-vfs`](./examples/implement-a-vfs) example.

## About multithreading

This library is not thread-safe: SQLite is compiled with `-DSQLITE_THREADSAFE=0`.
Using a custom host adapter does not change this or the memory VFS's
single-threaded access requirements.

With the default adapter and without Wasm atomics, `sqlite3_sleep` does not block.
With atomics, synchronous sleep requires a host context that permits waiting,
such as a browser worker;
enabling atomics does not make SQLite thread-safe.

## Custom hosts without wasm-bindgen

```toml
sqlite-wasm-rs = { version = "0.5", default-features = false }
```

Provide the five C ABI hooks declared in [`shim/host.h`](./shim/host.h)
for time, sleep, VFS randomness, secure entropy and local-time conversion.
The core handles the C shim and default memory VFS; the adapter chooses how to
communicate with its environment. No runtime host registration is needed.

See [`custom-host`](./examples/custom-host) for ordinary Wasm imports and a plain
Node loader without generated glue. Keep `wasm-bindgen` disabled throughout the
dependency graph, since Cargo features are additive. Hooks can be linked from
C or Rust, or supplied directly as Wasm imports from module `env`.
The example configures the linker to allow exactly these imports.
The `bindgen` feature only generates
SQLite C bindings and does not enable wasm-bindgen.

## Use prebuild libsqlite3.a

We provide the ability to use prebuild `libsqlite3.a`, cargo provides a [`links`](https://doc.rust-lang.org/cargo/reference/manifest.html#the-links-field) field that can be used to specify which library to link to. With the help of [overriding build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html#overriding-build-scripts), you can overriding its configuration in your crate and link sqlite to your prebuild `libsqlite3.a`.

More see [`use-prebuild-lib`](./examples/use-prebuild-lib) example.

This build-script override must not be combined with the `bindgen` feature:
it skips binding generation as well as C compilation. Use the checked-in Rust
bindings with a compatible static library instead.

## Minimum supported Rust version (MSRV)

The minimal officially supported rustc version is 1.85.0.

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
