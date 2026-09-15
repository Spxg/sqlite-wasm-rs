[![Crates.io](https://img.shields.io/crates/v/sqlite-wasm-vfs.svg)](https://crates.io/crates/sqlite-wasm-vfs)

Some experimental VFS implementations.

## Features

Both implementations are enabled by default:

| Feature | Module | Storage |
| --- | --- | --- |
| `sahpool` | `sqlite_wasm_vfs::sahpool` | Origin Private File System (OPFS), using a pool of `SyncAccessHandle`s |
| `relaxed-idb` | `sqlite_wasm_vfs::relaxed_idb` | IndexedDB, with relaxed durability guarantees |

To use only the OPFS SAH pool, without the IndexedDB implementation and its
`indexed_db_futures` dependency:

```toml
[dependencies]
sqlite-wasm-vfs = { version = "0.2", default-features = false, features = ["sahpool"] }
```

To use only IndexedDB, without the OPFS implementation and its `web-sys`
filesystem bindings:

```toml
[dependencies]
sqlite-wasm-vfs = { version = "0.2", default-features = false, features = ["relaxed-idb"] }
```
