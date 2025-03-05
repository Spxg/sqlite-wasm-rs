use std::ffi::CStr;

use sqlite_wasm_rs::export::*;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

use super::memory_db;

#[wasm_bindgen_test]
#[allow(unused)]
fn test_trigger() {
    let db = memory_db();
    let sql = c"
CREATE TABLE employees (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    salary REAL NOT NULL
);

CREATE TABLE employees_audit (
    id INTEGER,
    name TEXT NOT NULL,
    salary REAL NOT NULL,
    change_date TEXT NOT NULL
);

CREATE TRIGGER before_employee_update
BEFORE UPDATE ON employees
FOR EACH ROW
BEGIN
    INSERT INTO employees_audit (id, name, salary, change_date)
    VALUES (OLD.id, OLD.name, OLD.salary, datetime('now'));
END;

INSERT INTO employees (id, name, salary) VALUES (1, 'Alice', 50000);
INSERT INTO employees (id, name, salary) VALUES (2, 'Bob', 60000);
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

    unsafe extern "C" fn callback(
        _: *mut ::std::os::raw::c_void,
        arg2: ::std::os::raw::c_int,
        arg3: *mut *mut ::std::os::raw::c_char,
        arg4: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int {
        assert_eq!(arg2, 4);
        let values = Vec::from_raw_parts(arg3, arg2 as usize, arg2 as usize);
        let names = Vec::from_raw_parts(arg4, arg2 as usize, arg2 as usize);
        let mut iter = values
            .iter()
            .map(|s| CStr::from_ptr(*s))
            .zip(names.iter().map(|s| CStr::from_ptr(*s)));

        let next = iter.next().unwrap();
        assert_eq!((c"1", c"id"), next);

        let next = iter.next().unwrap();
        assert_eq!((c"Alice", c"name"), next);

        let next = iter.next().unwrap();
        assert_eq!((c"50000.0", c"salary"), next);

        let next = iter.next().unwrap();
        console_log!("{next:?}");

        std::mem::forget(values);
        std::mem::forget(names);

        0
    }

    let sql = c"
SELECT * FROM employees_audit;
        ";
    let ret = unsafe {
        sqlite3_exec(
            db,
            sql.as_ptr().cast(),
            Some(callback),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };

    assert_eq!(SQLITE_OK, ret);
}
