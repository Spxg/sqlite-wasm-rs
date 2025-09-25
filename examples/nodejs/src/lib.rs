use std::ffi::CStr;

use sqlite_wasm_rs::{
    sqlite3, sqlite3_column_count, sqlite3_column_double, sqlite3_column_int, sqlite3_column_text,
    sqlite3_column_type, sqlite3_exec, sqlite3_finalize, sqlite3_open_v2, sqlite3_prepare_v3,
    sqlite3_step, SQLITE_FLOAT, SQLITE_INTEGER, SQLITE_OK, SQLITE_OPEN_CREATE,
    SQLITE_OPEN_READWRITE, SQLITE_ROW, SQLITE_TEXT,
};
use wasm_bindgen::prelude::wasm_bindgen;

#[wasm_bindgen]
extern "C" {
    // Use `js_namespace` here to bind `console.log(..)` instead of just
    // `log(..)`
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

macro_rules! console_log {
    // Note that this is using the `log` function imported above during
    // `bare_bones`
    ($($t:tt)*) => (log(&format_args!($($t)*).to_string()))
}

#[wasm_bindgen(start)]
async fn main() {
    let mut db = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_open_v2(
            c"file:test_memory_vfs.db?vfs=memvfs".as_ptr().cast(),
            &mut db as *mut _,
            SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE,
            std::ptr::null(),
        )
    };
    assert_eq!(SQLITE_OK, ret);
    console_log!("db: {db:?}");
    prepare_simple_db(db);
    check_result(db);
}

fn prepare_simple_db(db: *mut sqlite3) {
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

pub fn check_result(db: *mut sqlite3) {
    let sql = c"SELECT * FROM employees;";
    let mut stmt = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_prepare_v3(
            db,
            sql.as_ptr().cast(),
            -1,
            0,
            &mut stmt as *mut _,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK);

    let ret = [(1, "Alice", 55000.0), (2, "Bob", 60000.0)];
    let mut idx = 0;

    unsafe {
        while sqlite3_step(stmt) == SQLITE_ROW {
            let count = sqlite3_column_count(stmt);
            for col in 0..count {
                let ty = sqlite3_column_type(stmt, col);
                match ty {
                    SQLITE_INTEGER => assert_eq!(ret[idx].0, sqlite3_column_int(stmt, col)),
                    SQLITE_TEXT => {
                        let s = CStr::from_ptr(sqlite3_column_text(stmt, col).cast())
                            .to_str()
                            .unwrap();
                        assert!(s == ret[idx].1);
                    }
                    SQLITE_FLOAT => assert_eq!(ret[idx].2, sqlite3_column_double(stmt, col)),
                    _ => unreachable!(),
                }
            }
            idx += 1;
        }
        console_log!("{ret:?}");
        sqlite3_finalize(stmt);
    }
}
