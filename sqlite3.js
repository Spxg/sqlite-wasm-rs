import sqlite3InitModule from "@sqlite.org/sqlite-wasm";

export class SQLite {
  constructor(sqlite3) {
    this.sqlite3 = sqlite3;
  }

  static async init(opts) {
    return await sqlite3InitModule({
      print: console.log,
      printErr: console.error,
      ...opts,
    });
  }

  version() {
    return this.sqlite3.version;
  }
}
