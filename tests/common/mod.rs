use sqlite_wasm_rs::*;
use std::ffi::{CStr, CString};

pub struct Db(pub *mut sqlite3);

impl Db {
    pub fn open(name: &str, vfs: &str, flags: i32) -> Result<Self, i32> {
        let name = CString::new(name).unwrap();
        let vfs = CString::new(vfs).unwrap();
        let mut db = std::ptr::null_mut();
        let code = unsafe { sqlite3_open_v2(name.as_ptr(), &mut db, flags, vfs.as_ptr()) };
        let db = Self(db);

        if code == SQLITE_OK {
            Ok(db)
        } else {
            Err(code)
        }
    }

    pub fn exec(&self, sql: &CStr) -> i32 {
        unsafe {
            sqlite3_exec(
                self.0,
                sql.as_ptr(),
                None,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    }

    pub fn prepare(&self) {
        assert_eq!(
            self.exec(
                c"
                CREATE TABLE employees(id INTEGER PRIMARY KEY, name TEXT, salary REAL);
                INSERT INTO employees VALUES(1, 'Alice', 50000), (2, 'Bob', 60000);
                UPDATE employees SET salary=55000 WHERE id=1;
            "
            ),
            SQLITE_OK
        );
    }

    pub fn check_rows(&self) {
        let mut stmt = std::ptr::null_mut();

        unsafe {
            assert_eq!(
                sqlite3_prepare_v2(
                    self.0,
                    c"SELECT id, name, salary FROM employees ORDER BY id;".as_ptr(),
                    -1,
                    &mut stmt,
                    std::ptr::null_mut(),
                ),
                SQLITE_OK
            );

            for (id, name, salary) in [(1, "Alice", 55000.0), (2, "Bob", 60000.0)] {
                assert_eq!(sqlite3_step(stmt), SQLITE_ROW);
                assert_eq!(sqlite3_column_type(stmt, 0), SQLITE_INTEGER);
                assert_eq!(sqlite3_column_int(stmt, 0), id);
                assert_eq!(sqlite3_column_type(stmt, 1), SQLITE_TEXT);
                assert_eq!(
                    CStr::from_ptr(sqlite3_column_text(stmt, 1).cast())
                        .to_str()
                        .unwrap(),
                    name
                );
                assert_eq!(sqlite3_column_type(stmt, 2), SQLITE_FLOAT);
                assert_eq!(sqlite3_column_double(stmt, 2), salary);
            }

            assert_eq!(sqlite3_step(stmt), SQLITE_DONE);
            assert_eq!(sqlite3_finalize(stmt), SQLITE_OK);
        }
    }
}

impl Drop for Db {
    fn drop(&mut self) {
        assert_eq!(unsafe { sqlite3_close(self.0) }, SQLITE_OK);
    }
}
