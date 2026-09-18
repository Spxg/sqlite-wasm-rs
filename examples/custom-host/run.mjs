import { readFile } from 'node:fs/promises';
import { randomFillSync } from 'node:crypto';

const path = process.argv[2] ?? 'target/wasm32-unknown-unknown/debug/custom_host.wasm';
const module = await WebAssembly.compile(await readFile(path));
// The C ABI hooks are supplied directly by the Wasm runtime.
for (const entry of WebAssembly.Module.imports(module)) {
    if (entry.module !== 'env') throw new Error(`Unexpected import: ${entry.module}.${entry.name}`);
}
// Status codes from shim/host.h, unrelated to SQLite result codes.
const OK = 0, UNAVAILABLE = 2, INVALID_TIME = 3;
let memory;
function secureRandom(ptr, len) {
    try {
        randomFillSync(new Uint8Array(memory.buffer, ptr >>> 0, len >>> 0));
        return OK;
    } catch {
        return UNAVAILABLE;
    }
}
const { exports } = await WebAssembly.instantiate(module, {
    env: {
        rust_sqlite_wasm_host_epoch_timestamp_in_ms: ptr => {
            new DataView(memory.buffer).setBigInt64(ptr >>> 0, BigInt(Date.now()), true);
            return OK;
        },
        rust_sqlite_wasm_host_sleep: (seconds, nanoseconds) => {
            Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0,
                Number(BigInt.asUintN(64, seconds)) * 1000 + nanoseconds / 1e6);
        },
        rust_sqlite_wasm_host_random: (ptr, len) => secureRandom(ptr, len) === OK ? len : 0,
        rust_sqlite_wasm_host_fill_entropy: secureRandom,
        rust_sqlite_wasm_host_localtime: (seconds, ptr) => {
            const date = new Date(Number(seconds) * 1000);
            if (!Number.isFinite(date.getTime())) return INVALID_TIME;
            const year = date.getFullYear();
            const month = date.getMonth();
            const leap = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
            const yearday = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334][month]
                + date.getDate() - 1 + (month > 1 && leap ? 1 : 0);
            new Int32Array(memory.buffer, ptr >>> 0, 10).set([
                year, month + 1, date.getDate(), date.getHours(), date.getMinutes(),
                date.getSeconds(), date.getDay(), yearday, -1, -date.getTimezoneOffset() * 60,
            ]);
            return OK;
        },
    },
});
memory = exports.memory;
const value = exports.run();
if (value !== 42) throw new Error(`Unexpected query result: ${value}`);
console.log(value);
