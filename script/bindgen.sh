#!/usr/bin/env bash
toolchain=1.85.0
target=wasm32-unknown-unknown

rustup toolchain install "$toolchain"
rustup +"$toolchain" target add "$target"

SQLITE_WASM_RS_UPDATE_BINDGEN=1 cargo +"$toolchain" build --target "$target" --features bindgen
SQLITE_WASM_RS_UPDATE_BINDGEN=1 cargo +"$toolchain" build --target "$target" --features bindgen,sqlite3mc
