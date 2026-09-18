# JavaScript host without wasm-bindgen

This example supplies the five C ABI host hooks directly as Wasm imports from
module `env`. Node provides time, sleep and secure randomness; there is
no Rust forwarding layer, wasm-bindgen dependency or generated JavaScript glue.
The ABI is documented in [`sqlite-wasm-rs.h`](../../sqlite-wasm-rs.h); linked C or Rust
adapters can implement the same hooks instead of importing them.
`build.rs` allows only the hook symbols listed in `imports.txt` to remain
undefined at link time; they become Wasm imports.

From this directory:

```sh
cargo build --target wasm32-unknown-unknown
node run.mjs
```

It creates an in-memory database, inserts a row, prints `42` and closes SQLite.
Use `--features sqlite3mc` to run it with encryption, or `--features bindgen` to
generate the SQLite C bindings (unrelated to wasm-bindgen).

The standalone workspace prevents the other examples from enabling the default
`wasm-bindgen` feature through Cargo feature unification.
