# C host without wasm-bindgen

[`host.c`](./host.c) implements the five hooks declared in
[`sqlite-wasm-rs.h`](../../sqlite-wasm-rs.h). `build.rs` compiles and statically
links this adapter into the Wasm module. Rust still targets
`wasm32-unknown-unknown`; only the adapter imports WASI preview1 services for
UTC time, secure randomness and synchronous sleep.

With Clang (including its Wasm backend), Rust's `wasm32-unknown-unknown` target,
and Node.js 20 or later installed, run from this directory:

```sh
cargo build --target wasm32-unknown-unknown
node run.mjs
```

The loader uses Node's built-in WASI implementation; it does not implement the
SQLite hooks in JavaScript. No wasm-bindgen, generated glue or WASI SDK is needed.
The example inserts a row into an in-memory database, queries it alongside
`datetime('now')`, prints `42` and closes SQLite. Use `--features sqlite3mc` for
encryption, or `--features bindgen` to generate SQLite bindings.

WASI preview1 has no local-time-zone service, so the localtime hook returns
`UNSUPPORTED`; SQL using the `localtime` modifier fails instead of silently
using UTC. This adapter does not add persistent storage or thread safety.
