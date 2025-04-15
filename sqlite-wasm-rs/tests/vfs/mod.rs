mod memory;
mod relaxed_idb;
mod sahpool;

use sqlite_wasm_rs::{sqlite3, sqlite3_exec, SQLITE_OK};
use wasm_bindgen_test::console_log;

pub fn check_persistent(db: *mut sqlite3) -> bool {
    let drop_or_create = drop_or_create_foo_table(db);
    if drop_or_create {
        console_log!("foo table not exists, created.");
    } else {
        console_log!("foo table exists, dropped.");
    }
    drop_or_create
}

pub fn drop_or_create_foo_table(db: *mut sqlite3) -> bool {
    let ret = unsafe {
        sqlite3_exec(
            db,
            c"DROP TABLE FOO;".as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    if SQLITE_OK == ret {
        return false;
    }

    let sql = c"CREATE TABLE IF NOT EXISTS FOO(
            ID INT PRIMARY KEY     NOT NULL,
            NAME           TEXT    NOT NULL );";

    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_OK, ret);

    true
}

pub fn prepare_simple_db(db: *mut sqlite3) {
    let sql = c"
CREATE TABLE IF NOT EXISTS employees (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    salary REAL NOT NULL
);

INSERT INTO employees (name, salary) VALUES ('Alice', 50000);
INSERT INTO employees (name, salary) VALUES ('Bob', 60000);
UPDATE employees SET salary = 55000 WHERE id = 1;
        ";
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
}
