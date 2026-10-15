# rsqlite-vfs

Traits and utilities for implementing SQLite VFS in Rust.
Includes a memory VFS, single-threaded unless the `threadsafe` feature is enabled.

See [implement-a-vfs](https://github.com/Spxg/sqlite-wasm-rs/blob/master/crates/rsqlite-vfs/examples/implement-a-vfs.rs) for a complete example. Run it from the repository root:

```sh
cargo run -p rsqlite-vfs --example implement-a-vfs
```

[API documentation](https://docs.rs/rsqlite-vfs)
