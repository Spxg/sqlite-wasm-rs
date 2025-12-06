rustup toolchain install 1.78.0
rustup target add wasm32-unknown-unknown
SQLITE_WASM_RS_UPDATE_BINDGEN=1 cargo +1.78.0 build --target wasm32-unknown-unknown --features bindgen
SQLITE_WASM_RS_UPDATE_BINDGEN=1 cargo +1.78.0 build --target wasm32-unknown-unknown --features bindgen,sqlite3mc
