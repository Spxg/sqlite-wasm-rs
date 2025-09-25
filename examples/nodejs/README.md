# `nodejs`

`sqlite-wasm-rs` can be used in nodejs (memvfs)

## Usage

```sh
cargo build --target wasm32-unknown-unknown --release
wasm-bindgen ../../target/wasm32-unknown-unknown/release/nodejs.wasm --out-dir pkg --nodejs
node pkg/nodejs.js
```
