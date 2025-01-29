use super::{cstr, memory_db};
use sqlite_wasm_rs::export::*;
use std::ffi::CStr;
use wasm_bindgen_test::{console_log, wasm_bindgen_test};

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_column_metadata() {
    init_sqlite().await.unwrap();
    let mut db = memory_db();

    let sql = "
        CREATE TABLE test(id INTEGER PRIMARY KEY, name TEXT);
        INSERT INTO test VALUES(1, 'Alice');
        INSERT INTO test VALUES(2, 'Bob');
        INSERT INTO test VALUES(3, 'Charlie');
        SELECT * FROM test;
        DELETE FROM test WHERE id = 2;
        SELECT * FROM test;
        DROP TABLE test;
    ";

    let sql = cstr(sql.trim());
    let mut remaining_sql = sql.as_ptr();

    unsafe {
        while !remaining_sql.is_null() {
            let remain = CStr::from_ptr(remaining_sql);
            if remain.is_empty() {
                break;
            }
            let mut stmt: *mut sqlite3_stmt = std::ptr::null_mut();
            let mut pz_tail = std::ptr::null();

            let ret =
                sqlite3_prepare_v3(db, remaining_sql, -1, 0, &mut stmt as _, &mut pz_tail as _);
            assert_eq!(ret, SQLITE_OK);

            let mut rc = sqlite3_step(stmt);

            while rc == SQLITE_ROW {
                for col in 0..sqlite3_column_count(stmt) {
                    let table_name = CStr::from_ptr(sqlite3_column_table_name(stmt, col));
                    let origin_name = CStr::from_ptr(sqlite3_column_origin_name(stmt, col));
                    let database_name = CStr::from_ptr(sqlite3_column_database_name(stmt, col));
                    assert_eq!(table_name.to_str().unwrap(), "test");
                    if col == 0 {
                        assert_eq!(origin_name.to_str().unwrap(), "id");
                    } else {
                        assert_eq!(origin_name.to_str().unwrap(), "name");
                    }
                    assert_eq!(database_name.to_str().unwrap(), "main");
                    console_log!("{table_name:?} {origin_name:?} {database_name:?}");
                }
                rc = sqlite3_step(stmt);
            }

            if rc == SQLITE_DONE {
                sqlite3_finalize(stmt);
                remaining_sql = pz_tail;
            }
        }
    }
    unsafe {
        sqlite3_close(db);
    }
}

#[wasm_bindgen_test]
#[allow(unused)]
async fn test_table_column_metadata() {
    init_sqlite().await.unwrap();
    let db = memory_db();

    unsafe {
        sqlite3_exec(
            db,
            cstr("CREATE TABLE test_table (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL, age INT);").as_ptr(),
            None,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );

        let mut data_type: *const ::std::os::raw::c_char = std::ptr::null_mut();
        let mut coll_req: *const ::std::os::raw::c_char = std::ptr::null_mut();
        let mut not_null = 0;
        let mut primary_key = 0;
        let mut auto_inc = 0;

        sqlite3_table_column_metadata(
            db,
            std::ptr::null(),
            cstr("test_table").as_ptr(),
            cstr("id").as_ptr(),
            &mut data_type as *mut _,
            &mut coll_req as *mut _,
            &mut not_null as _,
            &mut primary_key as _,
            &mut auto_inc as _,
        );

        let data_type = CStr::from_ptr(data_type);
        let coll_req = CStr::from_ptr(coll_req);
        console_log!("data_type: {data_type:?}, coll_req: {coll_req:?}, not_null: {not_null}, primary: {primary_key}, auto_inc: {auto_inc}");
        assert_eq!("INTEGER", data_type.to_str().unwrap());
        assert_eq!("BINARY", coll_req.to_str().unwrap());
        assert_eq!(0, not_null);
        assert_eq!(1, primary_key);
        assert_eq!(1, auto_inc);
    }
}
