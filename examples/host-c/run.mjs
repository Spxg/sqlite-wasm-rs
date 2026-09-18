import { readFile } from 'node:fs/promises';
import { WASI } from 'node:wasi';

const path = process.argv[2] ?? 'target/wasm32-unknown-unknown/debug/host_c.wasm';
const module = await WebAssembly.compile(await readFile(path));
// The five SQLite host hooks are linked C functions, not runtime imports.
for (const entry of WebAssembly.Module.imports(module)) {
    if (entry.module !== 'wasi_snapshot_preview1') {
        throw new Error(`Unexpected import: ${entry.module}.${entry.name}`);
    }
}
const wasi = new WASI({ version: 'preview1' });
const instance = await WebAssembly.instantiate(module, wasi.getImportObject());
wasi.initialize(instance);
const value = instance.exports.run();
if (value !== 42) throw new Error(`Unexpected query result: ${value}`);
console.log(value);
