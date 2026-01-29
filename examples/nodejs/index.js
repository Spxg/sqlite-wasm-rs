if (!globalThis.crypto) {
    globalThis.crypto = require("node:crypto").webcrypto;
}

const wasm = require('./pkg/nodejs.js');
// wasm.main(); // main is executed automatically due to #[wasm_bindgen(start)]
