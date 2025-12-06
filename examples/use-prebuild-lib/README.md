# `Use prebuild libsqlite3.a`

This example shows how to link sqlite3 with built libsqlite3.a

## Usage

```sh
RUSTFLAGS="-L $(pwd)" CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner cargo test --target wasm32-unknown-unknown
```
