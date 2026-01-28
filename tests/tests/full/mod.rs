#[cfg(feature = "sqlite3mc")]
mod sqlite3mc;
#[cfg(feature = "uuid4")]
mod uuid4_verify;
#[cfg(feature = "uuid7")]
mod uuid7_verify;
mod vfs;

use sqlite_wasm_rs::*;
use std::ffi::CStr;
use wasm_bindgen_test::console_log;

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
        sqlite3_finalize(stmt);
    }
}

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

/// Helper to execute a SQL statement ignoring result rows.
/// Panics if execution fails.
///
/// # Arguments
/// * `db` - Pointer to the open SQLite database connection.
/// * `sql` - The SQL statement to execute.
pub fn exec(db: *mut sqlite3, sql: &str) {
    let sql_c = std::ffi::CString::new(sql).unwrap();
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql_c.as_ptr().cast(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK, "exec failed for: {}", sql);
}

/// Helper to prepare a SQL statement.
/// Returns the raw statement pointer.
/// Panics if preparation fails.
///
/// # Arguments
/// * `db` - Pointer to the open SQLite database connection.
/// * `sql` - The SQL statement to prepare.
pub fn prepare(db: *mut sqlite3, sql: &str) -> *mut sqlite3_stmt {
    let sql_c = std::ffi::CString::new(sql).unwrap();
    let mut stmt = std::ptr::null_mut();
    let ret = unsafe {
        sqlite3_prepare_v2(
            db,
            sql_c.as_ptr().cast(),
            -1,
            &mut stmt,
            std::ptr::null_mut(),
        )
    };
    assert_eq!(ret, SQLITE_OK, "prepare failed for: {}", sql);
    stmt
}

/// Helper to read a text column from the current row of a statement.
/// Returns an empty string if NULL.
///
/// # Arguments
/// * `stmt` - Pointer to the prepared statement.
/// * `col` - Zero-based index of the column to read.
pub fn text_from_col(stmt: *mut sqlite3_stmt, col: i32) -> String {
    unsafe {
        let ptr = sqlite3_column_text(stmt, col);
        if ptr.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(ptr.cast())
                .to_string_lossy()
                .into_owned()
        }
    }
}
