# `multithreading`

This is a demo that runs SQLite in multithreading (multi-Worker) mode.

## Usage

Compile wasm and start the web server:

```
rustup target add wasm32-unknown-unknown
# Add wasm32-unknown-unknown toolchain

cargo install wasm-pack
# Install the wasm-pack toolchain

wasm-pack build --target web --features wrapper
wasm-pack build --target web --features polyfill
# Build wasm

python3 server.py
# Start server
```

Next, try it on the web page: [on the web page](http://localhost:8000)

Then open the browser console and you can see how it works.
