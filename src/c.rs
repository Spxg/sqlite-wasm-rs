pub use crate::libsqlite3_sys::{
    code_to_str, sqlite3, sqlite3_context, sqlite3_destructor_type, sqlite3_int64, sqlite3_stmt,
    sqlite3_value, SQLITE_BLOB, SQLITE_CONSTRAINT_CHECK, SQLITE_CONSTRAINT_FOREIGNKEY,
    SQLITE_CONSTRAINT_NOTNULL, SQLITE_CONSTRAINT_PRIMARYKEY, SQLITE_CONSTRAINT_UNIQUE,
    SQLITE_DESERIALIZE_READONLY, SQLITE_DETERMINISTIC, SQLITE_DONE, SQLITE_FLOAT, SQLITE_INTEGER,
    SQLITE_NULL, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, SQLITE_OPEN_URI,
    SQLITE_PREPARE_PERSISTENT, SQLITE_ROW, SQLITE_STATIC, SQLITE_TEXT, SQLITE_TRANSIENT,
    SQLITE_UTF8,
};
use crate::{
    wasm::{CApi, Wasm},
    SQLite, SQLiteError,
};
use core::panic;
use once_cell::sync::OnceCell;
use parking_lot::Mutex;
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
    ptr::NonNull,
};
use wasm_bindgen::{prelude::Closure, JsValue};

macro_rules! cstr {
    ($ptr:ident) => {
        CStr::from_ptr($ptr).to_str().expect("input not UTF-8")
    };
}

enum AllocType {
    // (len, cap)
    VecU8((usize, usize)),
}

#[derive(PartialEq, Eq, Hash)]
struct Ptr(*mut ::std::os::raw::c_void);

/// just be key
unsafe impl Sync for Ptr {}

/// just be key
unsafe impl Send for Ptr {}

static ALLOCATED: OnceCell<Mutex<HashMap<Ptr, AllocType>>> = OnceCell::new();

fn allocated() -> &'static Mutex<HashMap<Ptr, AllocType>> {
    ALLOCATED.get_or_init(|| Mutex::new(HashMap::new()))
}

fn sqlite() -> &'static SQLite {
    crate::sqlite().expect("must call init_sqlite_*() fisrt")
}

pub unsafe fn sqlite3_open_v2(
    filename: *const ::std::os::raw::c_char,
    ppDb: *mut *mut sqlite3,
    flags: ::std::os::raw::c_int,
    zVfs: *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let wasm_pp_db = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_open_v2(cstr!(filename), wasm_pp_db as *mut _, flags, cstr!(zVfs));

    wasm.peek(wasm_pp_db as _, &mut *ppDb);
    wasm.dealloc(wasm_pp_db);

    ret
}

pub unsafe fn sqlite3_exec(
    arg1: *mut sqlite3,
    sql: *const ::std::os::raw::c_char,
    callback: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut ::std::os::raw::c_void,
            arg2: ::std::os::raw::c_int,
            arg3: *mut *mut ::std::os::raw::c_char,
            arg4: *mut *mut ::std::os::raw::c_char,
        ) -> ::std::os::raw::c_int,
    >,
    arg2: *mut ::std::os::raw::c_void,
    errmsg: *mut *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let callback = callback.map(|f| {
        Closure::new(move |values: Vec<String>, names: Vec<String>| -> i32 {
            let mut values = values
                .into_iter()
                .map(|s| CString::new(s).unwrap().into_raw())
                .collect::<Vec<_>>();
            let mut names = names
                .into_iter()
                .map(|s| CString::new(s).unwrap().into_raw())
                .collect::<Vec<_>>();
            let ret = f(
                arg2,
                values.len() as ::std::os::raw::c_int,
                values.as_mut_ptr(),
                names.as_mut_ptr(),
            );
            for value in values {
                CString::from_raw(value);
            }
            for name in names {
                CString::from_raw(name);
            }
            ret
        })
    });

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let wasm_errmsg = wasm.alloc_ptr(1, true);

    let ret = capi.sqlite3_exec(
        arg1,
        cstr!(sql),
        callback.as_ref(),
        std::ptr::null_mut(),
        wasm_errmsg as *mut _,
    );

    wasm.peek(wasm_errmsg as _, &mut *errmsg);
    wasm.dealloc(wasm_errmsg);

    ret
}

pub unsafe fn sqlite3_close(arg1: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_close_v2(arg1)
}

pub unsafe fn sqlite3_changes(arg1: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_changes(arg1)
}

pub unsafe fn sqlite3_serialize(
    db: *mut sqlite3,
    zSchema: *const ::std::os::raw::c_char,
    piSize: *mut sqlite3_int64,
    mFlags: ::std::os::raw::c_uint,
) -> *mut ::std::os::raw::c_uchar {
    unsafe fn serialized(
        ptr: *mut u8,
        len: usize,
        capi: &CApi,
        wasm: &Wasm,
    ) -> *mut std::os::raw::c_uchar {
        let mut data = vec![0; len];
        wasm.peek_buf(ptr, len as u32, data.as_mut_slice());
        capi.sqlite3_free(ptr as *mut _);

        // into_raw_parts is unstable
        let (ret, len, cap) = (data.as_mut_ptr(), data.len(), data.capacity());
        std::mem::forget(data);

        allocated()
            .lock()
            .insert(Ptr(ret as _), AllocType::VecU8((len, cap)));

        ret
    }

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    if piSize.is_null() {
        capi.sqlite3_serialize(db, cstr!(zSchema), std::ptr::null_mut(), mFlags)
    } else {
        let size = wasm.alloc(std::mem::size_of::<sqlite3_int64>() as u32);
        let ptr = capi.sqlite3_serialize(db, cstr!(zSchema), size as *mut _, mFlags);

        let mut len = 0i64;
        wasm.peek(size, &mut len);

        wasm.dealloc(size);
        serialized(ptr, len as usize, &capi, &wasm)
    }
}

pub unsafe fn sqlite3_free(arg1: *mut ::std::os::raw::c_void) {
    match allocated()
        .lock()
        .remove(&Ptr(arg1))
        .expect("free not sqlite3 ptr")
    {
        AllocType::VecU8((len, cap)) => {
            drop(Vec::<u8>::from_raw_parts(arg1 as _, len, cap));
        }
    }
}

pub unsafe fn sqlite3_create_function_v2(
    db: *mut sqlite3,
    zFunctionName: *const ::std::os::raw::c_char,
    nArg: ::std::os::raw::c_int,
    eTextRep: ::std::os::raw::c_int,
    pApp: *mut ::std::os::raw::c_void,
    xFunc: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_context,
            arg2: ::std::os::raw::c_int,
            arg3: *mut *mut sqlite3_value,
        ),
    >,
    xStep: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut sqlite3_context,
            arg2: ::std::os::raw::c_int,
            arg3: *mut *mut sqlite3_value,
        ),
    >,
    xFinal: ::std::option::Option<unsafe extern "C" fn(arg1: *mut sqlite3_context)>,
    xDestroy: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();
    capi.sqlite3_create_function_v2(
        db,
        cstr!(zFunctionName),
        nArg,
        eTextRep,
        pApp,
        None,
        None,
        None,
        None,
    )
}

pub unsafe fn sqlite3_result_text(
    arg1: *mut sqlite3_context,
    arg2: *const ::std::os::raw::c_char,
    arg3: ::std::os::raw::c_int,
    arg4: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
}
pub unsafe fn sqlite3_result_blob(
    arg1: *mut sqlite3_context,
    arg2: *const ::std::os::raw::c_void,
    arg3: ::std::os::raw::c_int,
    arg4: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
}

pub unsafe fn sqlite3_result_int(arg1: *mut sqlite3_context, arg2: ::std::os::raw::c_int) {}

pub unsafe fn sqlite3_result_int64(arg1: *mut sqlite3_context, arg2: sqlite3_int64) {}

pub unsafe fn sqlite3_result_double(arg1: *mut sqlite3_context, arg2: f64) {}

pub unsafe fn sqlite3_result_null(arg1: *mut sqlite3_context) {}
