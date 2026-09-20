# C host

With Clang (including its Wasm backend), Rust's `wasm32-unknown-unknown` target,
and Node.js 20 or later installed, run from this directory:

```sh
cargo build --target wasm32-unknown-unknown
node run.mjs
```
