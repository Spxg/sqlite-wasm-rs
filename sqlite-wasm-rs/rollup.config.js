import { defineConfig } from "rollup";
import { nodeResolve } from "@rollup/plugin-node-resolve";
import copy from "rollup-plugin-copy";
import terser from "@rollup/plugin-terser";

export default defineConfig([
  {
    input: "sqlite_sdk.js",
    output: {
      file: "src/wrapper/jswasm/sqlite3.js",
      format: "es",
    },
    treeshake: "smallest",
    plugins: [
      nodeResolve(),
      terser(),
      copy({
        targets: [
          {
            src: "./sqlite-wasm/sqlite3.wasm",
            dest: "src/wrapper/jswasm",
          },
        ],
      }),
    ],
  },
  {
    input:
      "./sqlite-wasm/sqlite3-opfs-async-proxy.js",
    output: {
      file: "src/wrapper/jswasm/sqlite3-opfs-async-proxy.js",
      format: "es",
    },
    plugins: [terser()],
  },
]);
