import sqlite3InitModule from "./sqlite-wasm";

const log = console.log;
const err_log = console.error;

export class SQLiteError extends Error {
  constructor(message, code) {
    super(message);
    this.code = code;
  }
}

export class SQLite {
  constructor(sqlite3) {
    if (typeof sqlite3 === "undefined") {
      throw new Error(
        "`sqliteObject` must be defined before calling constructor",
      );
    }
    this.sqlite3 = sqlite3;
  }

  static async init(opts) {
    return await sqlite3InitModule({
      print: log,
      printErr: err_log,
      ...opts,
    });
  }

  version() {
    return this.sqlite3.version;
  }
}
