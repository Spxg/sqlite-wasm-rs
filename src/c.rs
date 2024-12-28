pub use crate::libsqlite3_sys::{
    code_to_str, sqlite3, sqlite3_context, sqlite3_destructor_type, sqlite3_int64, sqlite3_stmt,
    sqlite3_value, SQLITE_BLOB, SQLITE_CONSTRAINT_CHECK, SQLITE_CONSTRAINT_FOREIGNKEY,
    SQLITE_CONSTRAINT_NOTNULL, SQLITE_CONSTRAINT_PRIMARYKEY, SQLITE_CONSTRAINT_UNIQUE,
    SQLITE_DESERIALIZE_READONLY, SQLITE_DETERMINISTIC, SQLITE_DONE, SQLITE_FLOAT, SQLITE_INTEGER,
    SQLITE_NULL, SQLITE_OK, SQLITE_OPEN_CREATE, SQLITE_OPEN_READWRITE, SQLITE_OPEN_URI,
    SQLITE_PREPARE_PERSISTENT, SQLITE_ROW, SQLITE_STATIC, SQLITE_TEXT, SQLITE_TRANSIENT,
    SQLITE_UTF8,
};
pub use crate::{init_sqlite, init_sqlite_with, MemoryOpts, SQLiteError, SQLiteOpts};
use crate::{
    wasm::{CApi, Wasm},
    SQLite,
};
use core::panic;
use once_cell::sync::Lazy;
use std::sync::Mutex;
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
};
use wasm_bindgen::{prelude::Closure, JsValue};

macro_rules! cstr {
    ($ptr:ident) => {
        if $ptr.is_null() {
            JsValue::from(0x0)
        } else {
            JsValue::from(CStr::from_ptr($ptr).to_str().expect("input not UTF-8"))
        }
    };
}

enum AllocatedT {
    // (ptr, len, cap)
    VecU8((*mut u8, usize, usize)),
    CStr(*mut i8),
}

impl Drop for AllocatedT {
    fn drop(&mut self) {
        unsafe {
            match self {
                AllocatedT::VecU8((ptr, len, cap)) => {
                    drop(Vec::<u8>::from_raw_parts(*ptr, *len, *cap));
                }
                AllocatedT::CStr(ptr) => {
                    drop(CString::from_raw(*ptr));
                }
            }
        }
    }
}

#[derive(PartialEq, Eq, Hash)]
struct Ptr(*mut ::std::os::raw::c_void);

/// just be key
unsafe impl Sync for Ptr {}

/// just be key
unsafe impl Send for Ptr {}

/// just be value
unsafe impl Sync for AllocatedT {}

/// just be value
unsafe impl Send for AllocatedT {}

static ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> = Lazy::new(|| Mutex::new(HashMap::new()));

static STMT_ALLOCATED: Lazy<Mutex<HashMap<Ptr, HashMap<i32, AllocatedT>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static AGGREGATE_ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static STMT_SQLITE3_VALUES_ALLOCATED: Lazy<Mutex<HashMap<Ptr, Vec<Ptr>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

static SQLITE3_VALUES_ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn sqlite() -> &'static SQLite {
    crate::sqlite().expect("must call init_sqlite_*() fisrt")
}

fn dtor(
    arg: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> i32 {
    if arg == SQLITE_TRANSIENT() {
        -1
    } else if arg == SQLITE_STATIC() {
        0
    } else {
        panic!("used dtor not supported now");
    }
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
                drop(CString::from_raw(value));
            }
            for name in names {
                drop(CString::from_raw(name));
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
        arg2,
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

pub unsafe fn sqlite3_deserialize(
    db: *mut sqlite3,
    zSchema: *const ::std::os::raw::c_char,
    pData: *mut ::std::os::raw::c_uchar,
    szDb: sqlite3_int64,
    szBuf: sqlite3_int64,
    mFlags: ::std::os::raw::c_uint,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    if pData.is_null() {
        capi.sqlite3_deserialize(db, cstr!(zSchema), pData, szDb, szBuf, mFlags)
    } else {
        let wasm_p_data = if szBuf == 0 {
            wasm.alloc_ptr(1, true)
        } else {
            wasm.alloc(szBuf as u32)
        };
        let slice = std::slice::from_raw_parts(pData as _, szBuf as usize);
        wasm.poke(slice, wasm_p_data);

        let ret =
            capi.sqlite3_deserialize(db, cstr!(zSchema), wasm_p_data as _, szDb, szBuf, mFlags);

        // duplicated memeory

        ret
    }
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

        ALLOCATED
            .lock()
            .unwrap()
            .insert(Ptr(ret as _), AllocatedT::VecU8((ret, len, cap)));

        ret
    }

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    if piSize.is_null() {
        capi.sqlite3_serialize(db, cstr!(zSchema), piSize, mFlags)
    } else {
        let size = wasm.alloc(std::mem::size_of::<sqlite3_int64>() as u32);
        let ptr = capi.sqlite3_serialize(db, cstr!(zSchema), size as *mut _, mFlags);

        wasm.peek(size, &mut *piSize);

        wasm.dealloc(size);
        serialized(ptr, *piSize as usize, &capi, &wasm)
    }
}

pub unsafe fn sqlite3_free(arg1: *mut ::std::os::raw::c_void) {
    // FIX ME: memory leak if Rust and Wasm have same allocated ptr
    if ALLOCATED.lock().unwrap().remove(&Ptr(arg1)).is_none() {
        sqlite().capi().sqlite3_free(arg1);
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
    let xFunc = xFunc
        .map(|f| {
            Closure::new(
                move |arg1: *mut sqlite3_context,
                      arg2: ::std::os::raw::c_int,
                      arg3: *mut *mut sqlite3_value| {
                    let mut values = vec![std::ptr::null_mut(); arg2 as usize];
                    for (offset, value) in (0..).zip(values.iter_mut()) {
                        *value = wasm.peek_ptr(arg3.offset(offset) as _) as _;
                    }
                    f(arg1, arg2, values.as_mut_ptr());
                },
            )
        })
        .map(|x| x);

    let wasm = sqlite3.wasm();
    let xStep = xStep
        .map(|f| {
            Closure::new(
                move |arg1: *mut sqlite3_context,
                      arg2: ::std::os::raw::c_int,
                      arg3: *mut *mut sqlite3_value| {
                    let mut values = vec![std::ptr::null_mut(); arg2 as usize];
                    for (offset, value) in (0..).zip(values.iter_mut()) {
                        *value = wasm.peek_ptr(arg3.offset(offset) as _) as _;
                    }
                    f(arg1, arg2, values.as_mut_ptr());
                },
            )
        })
        .map(|x| x);

    let xFinal = xFinal.map(|f| {
        Closure::new(move |ctx: *mut sqlite3_context| {
            let aggreagate = sqlite3_aggregate_context(ctx, 0);
            f(ctx);
            if !aggreagate.is_null() {
                AGGREGATE_ALLOCATED
                    .lock()
                    .unwrap()
                    .remove(&Ptr(aggreagate as _));
            }
        })
    });

    let xDestroy = xDestroy.map(|f| {
        Closure::new(move || {
            f(pApp);
        })
    });

    let ret = capi.sqlite3_create_function_v2(
        db,
        cstr!(zFunctionName),
        nArg,
        eTextRep,
        pApp,
        xFunc.as_ref(),
        xStep.as_ref(),
        xFinal.as_ref(),
        xDestroy.as_ref(),
    );

    xFunc.map(|x| x.forget());
    xStep.map(|x| x.forget());
    xFinal.map(|x| x.forget());
    xDestroy.map(|x| x.forget());

    ret
}

pub unsafe fn sqlite3_result_text(
    arg1: *mut sqlite3_context,
    arg2: *const ::std::os::raw::c_char,
    arg3: ::std::os::raw::c_int,
    arg4: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtor(arg4);

    if arg2.is_null() {
        capi.sqlite3_result_text(arg1, JsValue::from(0x0), arg3, dtor);
    } else {
        let slice = std::slice::from_raw_parts(arg2 as *const u8, arg3 as usize);
        let s = core::str::from_utf8(slice).expect("result is not utf8");
        capi.sqlite3_result_text(arg1, JsValue::from(s), arg3, dtor);
    }
}

pub unsafe fn sqlite3_result_blob(
    arg1: *mut sqlite3_context,
    arg2: *const ::std::os::raw::c_void,
    arg3: ::std::os::raw::c_int,
    arg4: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let dtor = dtor(arg4);

    if arg2.is_null() {
        capi.sqlite3_result_blob(arg1, arg2, arg3, dtor);
    } else {
        let slice = std::slice::from_raw_parts(arg2 as *const u8, arg3 as usize);
        let wasm_ptr = wasm.alloc(arg3 as u32);
        wasm.poke(slice, wasm_ptr);
        capi.sqlite3_result_blob(arg1, wasm_ptr as *const _, arg3, dtor);

        // FIX ME: memory leak if set SQLITE_STATIC
        if dtor == -1 {
            wasm.dealloc(wasm_ptr);
        }
    }
}

pub unsafe fn sqlite3_result_int(arg1: *mut sqlite3_context, arg2: ::std::os::raw::c_int) {
    sqlite().capi().sqlite3_result_int(arg1, arg2);
}

pub unsafe fn sqlite3_result_int64(arg1: *mut sqlite3_context, arg2: sqlite3_int64) {
    sqlite().capi().sqlite3_result_int64(arg1, arg2);
}

pub unsafe fn sqlite3_result_double(arg1: *mut sqlite3_context, arg2: f64) {
    sqlite().capi().sqlite3_result_double(arg1, arg2);
}

pub unsafe fn sqlite3_result_null(arg1: *mut sqlite3_context) {
    sqlite().capi().sqlite3_result_null(arg1);
}

pub unsafe fn sqlite3_column_value(
    arg1: *mut sqlite3_stmt,
    iCol: ::std::os::raw::c_int,
) -> *mut sqlite3_value {
    let ret = sqlite().capi().sqlite3_column_value(arg1, iCol);
    STMT_SQLITE3_VALUES_ALLOCATED
        .lock()
        .unwrap()
        .entry(Ptr(arg1 as _))
        .or_default()
        .push(Ptr(ret as _));
    ret
}

pub unsafe fn sqlite3_column_count(pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_column_count(pStmt)
}

pub unsafe fn sqlite3_column_name(
    arg1: *mut sqlite3_stmt,
    N: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    let s = sqlite().capi().sqlite3_column_name(arg1, N);
    // The returned string pointer is valid until either the prepared statement
    // is destroyed by sqlite3_finalize() or until the statement is automatically
    // reprepared by the first call to sqlite3_step() for a particular run or until
    // the next call to sqlite3_column_name() or sqlite3_column_name16() on the same column.

    let cstr = CString::new(s).unwrap();
    let ret = cstr.into_raw();
    STMT_ALLOCATED
        .lock()
        .unwrap()
        .entry(Ptr(arg1 as _))
        .or_default()
        .insert(N, AllocatedT::CStr(ret));
    ret
}

pub unsafe fn sqlite3_bind_null(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_null(arg1, arg2);
    SQLITE_OK
}

pub unsafe fn sqlite3_bind_blob(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
    arg3: *const ::std::os::raw::c_void,
    n: ::std::os::raw::c_int,
    arg4: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let dtor = dtor(arg4);

    if arg3.is_null() {
        capi.sqlite3_bind_blob(arg1, arg2, arg3, n, dtor)
    } else {
        let slice = std::slice::from_raw_parts(arg3 as *const u8, n as usize);
        let wasm_ptr = if n == 0 {
            wasm.alloc_ptr(1, true)
        } else {
            wasm.alloc(n as u32)
        };
        wasm.poke(slice, wasm_ptr);

        let ret = capi.sqlite3_bind_blob(arg1, arg2, wasm_ptr as *const _, n, dtor);

        // FIX ME: memory leak if set SQLITE_STATIC
        if dtor == -1 {
            wasm.dealloc(wasm_ptr);
        }

        ret
    }
}

pub unsafe fn sqlite3_bind_text(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
    arg3: *const ::std::os::raw::c_char,
    arg4: ::std::os::raw::c_int,
    arg5: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let dtor = dtor(arg5);

    if arg3.is_null() {
        capi.sqlite3_bind_text(arg1, arg2, arg3, arg4, dtor)
    } else {
        let slice = std::slice::from_raw_parts(arg3 as *const u8, arg4 as usize);
        let wasm_ptr = if arg4 == 0 {
            wasm.alloc_ptr(1, true)
        } else {
            wasm.alloc(arg4 as u32)
        };
        wasm.poke(slice, wasm_ptr);

        let ret = capi.sqlite3_bind_text(arg1, arg2, wasm_ptr as *const _, arg4, dtor);

        // FIX ME: memory leak if set SQLITE_STATIC
        if dtor == -1 {
            wasm.dealloc(wasm_ptr);
        }

        ret
    }
}

// only duplicate sqlite3_value will call
pub unsafe fn sqlite3_value_free(arg1: *mut sqlite3_value) {
    SQLITE3_VALUES_ALLOCATED
        .lock()
        .unwrap()
        .remove(&Ptr(arg1 as _));
    sqlite().capi().sqlite3_value_free(arg1);
}

pub unsafe fn sqlite3_value_bytes(arg1: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_bytes(arg1)
}

pub unsafe fn sqlite3_value_text(arg1: *mut sqlite3_value) -> *const ::std::os::raw::c_uchar {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    // sqlite3_value_text returns cstr, which is very confusing to me.
    // There is no such problem on the native platform.
    // So here sqlite3_value_blob is used instead.
    let ptr = capi.sqlite3_value_blob(arg1);
    let len = capi.sqlite3_value_bytes(arg1);
    let mut ret = vec![0; len as usize];
    wasm.peek_buf(ptr as _, len as u32, ret.as_mut_slice());
    let mut data = String::from_utf8_unchecked(ret);

    let (ret, len, cap) = (data.as_mut_ptr(), data.len(), data.capacity());
    std::mem::forget(data);
    SQLITE3_VALUES_ALLOCATED
        .lock()
        .unwrap()
        .insert(Ptr(arg1 as _), AllocatedT::VecU8((ret, len, cap)));

    ret as _
}

pub unsafe fn sqlite3_value_blob(arg1: *mut sqlite3_value) -> *const ::std::os::raw::c_void {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let ptr = capi.sqlite3_value_blob(arg1);
    let len = capi.sqlite3_value_bytes(arg1);
    let mut data = vec![0; len as usize];
    wasm.peek_buf(ptr as _, len as u32, data.as_mut_slice());

    let (ret, len, cap) = (data.as_mut_ptr(), data.len(), data.capacity());
    std::mem::forget(data);
    SQLITE3_VALUES_ALLOCATED
        .lock()
        .unwrap()
        .insert(Ptr(arg1 as _), AllocatedT::VecU8((ret, len, cap)));
    ret as _
}

pub unsafe fn sqlite3_value_int(arg1: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_int(arg1)
}

pub unsafe fn sqlite3_value_int64(arg1: *mut sqlite3_value) -> sqlite3_int64 {
    sqlite().capi().sqlite3_value_int64(arg1)
}

pub unsafe fn sqlite3_value_double(arg1: *mut sqlite3_value) -> f64 {
    sqlite().capi().sqlite3_value_double(arg1)
}

pub unsafe fn sqlite3_value_type(arg1: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_type(arg1)
}

pub unsafe fn sqlite3_value_dup(arg1: *const sqlite3_value) -> *mut sqlite3_value {
    sqlite().capi().sqlite3_value_dup(arg1)
}

pub unsafe fn sqlite3_bind_double(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
    arg3: f64,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_double(arg1, arg2, arg3)
}

pub unsafe fn sqlite3_bind_int(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
    arg3: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_int(arg1, arg2, arg3)
}

pub unsafe fn sqlite3_bind_int64(
    arg1: *mut sqlite3_stmt,
    arg2: ::std::os::raw::c_int,
    arg3: sqlite3_int64,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_int64(arg1, arg2, arg3)
}

pub unsafe fn sqlite3_create_collation_v2(
    arg1: *mut sqlite3,
    zName: *const ::std::os::raw::c_char,
    eTextRep: ::std::os::raw::c_int,
    pArg: *mut ::std::os::raw::c_void,
    xCompare: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut ::std::os::raw::c_void,
            arg2: ::std::os::raw::c_int,
            arg3: *const ::std::os::raw::c_void,
            arg4: ::std::os::raw::c_int,
            arg5: *const ::std::os::raw::c_void,
        ) -> ::std::os::raw::c_int,
    >,
    xDestroy: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let xCompare = xCompare.map(|f| {
        Closure::new(
            move |arg1: *mut ::std::os::raw::c_void,
                  arg2: ::std::os::raw::c_int,
                  arg3: *const ::std::os::raw::c_void,
                  arg4: ::std::os::raw::c_int,
                  arg5: *const ::std::os::raw::c_void| {
                let mut str1 = vec![0u8; arg2 as usize];
                wasm.peek_buf(arg3 as _, arg2 as u32, str1.as_mut_slice());
                let str1 = CString::new(String::from_utf8(str1).unwrap()).unwrap();
                let ptr1 = str1.into_raw();

                let mut str2 = vec![0u8; arg4 as usize];
                wasm.peek_buf(arg5 as _, arg4 as u32, str2.as_mut_slice());
                let str2 = CString::new(String::from_utf8(str2).unwrap()).unwrap();
                let ptr2 = str2.into_raw();

                let ret = f(arg1, arg2, ptr1 as _, arg4, ptr2 as _);

                drop(CString::from_raw(ptr1));
                drop(CString::from_raw(ptr2));

                ret
            },
        )
    });

    let xDestroy = xDestroy.map(|f| Closure::new(move |arg1: *mut ::std::os::raw::c_void| f(arg1)));

    let ret = capi.sqlite3_create_collation_v2(
        arg1,
        cstr!(zName),
        eTextRep,
        pArg,
        xCompare.as_ref(),
        xDestroy.as_ref(),
    );

    xCompare.map(|x| x.forget());
    xDestroy.map(|x| x.forget());

    ret
}

pub unsafe fn sqlite3_extended_errcode(db: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_extended_errcode(db)
}

pub unsafe fn sqlite3_finalize(pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    STMT_ALLOCATED.lock().unwrap().remove(&Ptr(pStmt as _));

    let mut locked = SQLITE3_VALUES_ALLOCATED.lock().unwrap();
    for sqlite3_value in STMT_SQLITE3_VALUES_ALLOCATED
        .lock()
        .unwrap()
        .remove(&Ptr(pStmt as _))
        .unwrap_or_else(|| vec![])
    {
        locked.remove(&sqlite3_value);
    }

    sqlite().capi().sqlite3_finalize(pStmt)
}

pub unsafe fn sqlite3_step(arg1: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_step(arg1)
}

pub unsafe fn sqlite3_errmsg(arg1: *mut sqlite3) -> *const ::std::os::raw::c_char {
    static ERR_MSG: Lazy<Mutex<Option<AllocatedT>>> = Lazy::new(|| Mutex::new(None));
    let ret = sqlite().capi().sqlite3_errmsg(arg1);
    let cstr = CString::new(ret).unwrap();
    let raw = cstr.into_raw();
    ERR_MSG.lock().unwrap().replace(AllocatedT::CStr(raw));
    raw
}

pub unsafe fn sqlite3_db_handle(arg1: *mut sqlite3_stmt) -> *mut sqlite3 {
    sqlite().capi().sqlite3_db_handle(arg1)
}

pub unsafe fn sqlite3_reset(pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_reset(pStmt)
}

pub unsafe fn sqlite3_prepare_v3(
    db: *mut sqlite3,
    zSql: *const ::std::os::raw::c_char,
    nByte: ::std::os::raw::c_int,
    prepFlags: ::std::os::raw::c_uint,
    ppStmt: *mut *mut sqlite3_stmt,
    pzTail: *mut *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let wasm_z_sql = if zSql.is_null() {
        std::ptr::null_mut::<u8>()
    } else {
        let (wasm_z_sql, len) = match nByte.cmp(&0) {
            std::cmp::Ordering::Less => {
                let len = CString::from_raw(zSql as *mut _).to_str().unwrap().len();
                (wasm.alloc(len as u32), len)
            }
            std::cmp::Ordering::Equal => (wasm.alloc_ptr(1, true), 0),
            std::cmp::Ordering::Greater => (wasm.alloc(nByte as u32), nByte as usize),
        };
        wasm.poke(std::slice::from_raw_parts(zSql as _, len), wasm_z_sql);
        wasm_z_sql
    };

    let wasm_pp_stmt = wasm.alloc_ptr(1, true);
    let wasm_pz_tail = wasm.alloc_ptr(1, true);
    let ret = capi.sqlite3_prepare_v3(
        db,
        wasm_z_sql as _,
        nByte,
        prepFlags,
        wasm_pp_stmt as _,
        wasm_pz_tail as _,
    );
    wasm.peek(wasm_pp_stmt, &mut *ppStmt);
    wasm.peek(wasm_pz_tail, &mut *pzTail);

    wasm.dealloc(wasm_z_sql);
    wasm.dealloc(wasm_pp_stmt);
    wasm.dealloc(wasm_pz_tail);

    ret
}

pub unsafe fn sqlite3_context_db_handle(arg1: *mut sqlite3_context) -> *mut sqlite3 {
    sqlite().capi().sqlite3_context_db_handle(arg1)
}

pub unsafe fn sqlite3_user_data(arg1: *mut sqlite3_context) -> *mut ::std::os::raw::c_void {
    sqlite().capi().sqlite3_user_data(arg1)
}

pub unsafe fn sqlite3_aggregate_context(
    arg1: *mut sqlite3_context,
    nBytes: ::std::os::raw::c_int,
) -> *mut ::std::os::raw::c_void {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    let ptr = capi.sqlite3_aggregate_context(arg1, nBytes);
    if ptr.is_null() {
        return std::ptr::null_mut::<::std::os::raw::c_void>();
    }

    if let Some(AllocatedT::VecU8((ptr, _, _))) =
        AGGREGATE_ALLOCATED.lock().unwrap().get(&Ptr(ptr as _))
    {
        return *ptr as _;
    }

    let mut data = vec![0; nBytes as usize];
    wasm.peek_buf(ptr as _, nBytes as _, data.as_mut_slice());
    let (ret, len, cap) = (data.as_mut_ptr(), data.len(), data.capacity());
    std::mem::forget(data);

    AGGREGATE_ALLOCATED
        .lock()
        .unwrap()
        .insert(Ptr(ptr as _), AllocatedT::VecU8((ret, len, cap)));

    ret as _
}

pub unsafe fn sqlite3_result_error(
    arg1: *mut sqlite3_context,
    arg2: *const ::std::os::raw::c_char,
    arg3: ::std::os::raw::c_int,
) {
    sqlite()
        .capi()
        .sqlite3_result_error(arg1, cstr!(arg2), arg3);
}
