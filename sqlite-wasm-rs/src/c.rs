//! This module provides some C-Like interfaces from sqlite-wasm.

use crate::libsqlite3::*;
use crate::SQLite;
use once_cell::sync::Lazy;
use sqlite_wasm_macro::multithread;
use std::mem::{size_of, ManuallyDrop};
use std::sync::{Mutex, MutexGuard};
use std::{
    collections::HashMap,
    ffi::{CStr, CString},
};
use std::{panic, slice, str};
use wasm_bindgen::{prelude::Closure, JsValue};

/// Wrap some multithreading calls
#[cfg(target_feature = "atomics")]
pub(crate) mod multithreading {
    use super::*;
    use std::sync::mpsc::Sender;

    include!(concat!(env!("OUT_DIR"), "/multithreading.rs"));

    pub struct Task {
        req: CApiReq,
        tx: Sender<CApiResp>,
    }

    impl Task {
        pub fn run(self) {
            self.tx
                .send(self.req.call())
                .expect("recv channel never disconnect");
        }
    }

    pub fn call(sqlite: &SQLite, req: CApiReq) -> CApiResp {
        let (tx, rx) = std::sync::mpsc::channel();
        sqlite
            .tx
            .send(Task { req, tx })
            .expect("recv channel never disconnect");
        rx.recv().expect("send channel never disconnect")
    }
}

#[cfg(target_feature = "atomics")]
use multithreading::{call, CApiReq, CApiResp};

/// Get a static reference to sqlite.
///
/// Need to call `init_sqlite()` before calling
fn sqlite() -> &'static SQLite {
    crate::sqlite()
        .expect("Call init_sqlite() to initialize sqlite3 before executing the C interface")
}

/// Use JsValue to express a null pointer or string.
/// Because the sqlite-wasm pointer is a number, use 0x0
macro_rules! cstr {
    ($ptr:ident) => {
        if $ptr.is_null() {
            JsValue::from(0x0)
        } else {
            JsValue::from(CStr::from_ptr($ptr).to_str().expect("expect utf8 text"))
        }
    };
}

/// Convert a `String` to a `CString`
fn cstring(s: String) -> CString {
    CString::new(s).expect("included an internal 0 byte")
}

/// Wraps an `OutputPtr` structure
///
/// Output-pointer arguments are commonplace in C.
/// On the contrary, they do not exist at all in JavaScript.
struct OutputPtr<'a, T> {
    sqlite3: &'a SQLite,
    wasm_ptr: *mut u8,
    rust_ptr: *mut T,
    is_cstr: bool,
}

impl<'a, T> OutputPtr<'a, T> {
    fn new(handle: &'a SQLite, rust_ptr: *mut T, is_cstr: bool) -> Self {
        Self {
            sqlite3: handle,
            wasm_ptr: if rust_ptr.is_null() {
                std::ptr::null_mut()
            } else {
                handle.wasm().alloc(size_of::<T>())
            },
            rust_ptr,
            is_cstr,
        }
    }
}

/// Peek and dealloc sqlite memory
impl<T> Drop for OutputPtr<'_, T> {
    fn drop(&mut self) {
        unsafe {
            if !self.wasm_ptr.is_null() {
                assert!(!self.rust_ptr.is_null());
                self.sqlite3.peek(self.wasm_ptr, &mut *self.rust_ptr);
                self.sqlite3.wasm().dealloc(self.wasm_ptr);
                if self.is_cstr {
                    cstr_output_ptr(self.sqlite3, self.rust_ptr.cast());
                }
            }
        }
    }
}

/// Convert output ptr of string type
unsafe fn cstr_output_ptr(handle: &SQLite, ptr: *mut *const ::std::os::raw::c_char) {
    if !ptr.is_null() && !(*ptr).is_null() {
        let capi = handle.capi();
        let wasm = handle.wasm();
        let wasm_ptr = *ptr;
        // convert to string
        let errmsg = wasm.cstr_to_js(wasm_ptr.cast());
        // free sqlite errmsg
        capi.sqlite3_free(wasm_ptr.cast_mut().cast());
        let raw = cstring(errmsg).into_raw();
        *ptr = raw;
        allocated().insert(Ptr(raw.cast()), AllocatedT::CString(raw));
    }
}

/// Some leaked memory during function calls
enum AllocatedT {
    // (ptr, len, cap)
    VecU8((*mut u8, usize, usize)),
    CString(*mut i8),
}

/// * This is a private structure
/// * SQLite has thread checking and currently only supports calling in one thread
unsafe impl Sync for AllocatedT {}
unsafe impl Send for AllocatedT {}

/// Free memory when drop
impl Drop for AllocatedT {
    fn drop(&mut self) {
        unsafe {
            match self {
                Self::VecU8((ptr, len, cap)) => {
                    drop(Vec::<u8>::from_raw_parts(*ptr, *len, *cap));
                }
                Self::CString(ptr) => {
                    drop(CString::from_raw(*ptr));
                }
            }
        }
    }
}

/// A simple wrapper that converts pointers to void* for storage
#[derive(PartialEq, Eq, Hash)]
struct Ptr(*mut ::std::os::raw::c_void);

/// just be key
unsafe impl Sync for Ptr {}
unsafe impl Send for Ptr {}

/// Maintain a list of allocated memory
/// and free the memory at the end of the life
fn allocated() -> MutexGuard<'static, HashMap<Ptr, AllocatedT>> {
    static ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    ALLOCATED.lock().expect("acquire allocated lock failed")
}

#[derive(Hash, PartialEq, Eq)]
struct StmtKey {
    r#type: &'static str,
    idx: ::std::os::raw::c_int,
}

impl StmtKey {
    fn new(r#type: &'static str, idx: ::std::os::raw::c_int) -> Self {
        Self { r#type, idx }
    }
}

/// Maintain a list of `sqlite3_stmt and col` allocated memory
/// and free the memory at the end of the life
fn stmt_with_key_allocated() -> MutexGuard<'static, HashMap<Ptr, HashMap<StmtKey, AllocatedT>>> {
    static STMT_COL_ALLOCATED: Lazy<Mutex<HashMap<Ptr, HashMap<StmtKey, AllocatedT>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    STMT_COL_ALLOCATED
        .lock()
        .expect("acquire stmt with key allocated lock failed")
}

/// Maintain a list of `aggregate_context` allocated memory
/// and free the memory at the end of the life
fn aggregate_allocated() -> MutexGuard<'static, HashMap<Ptr, AllocatedT>> {
    static AGGREGATE_ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    AGGREGATE_ALLOCATED
        .lock()
        .expect("acquire aggregate allocated lock failed")
}

/// Maintain a list of stmt's `sqlite3_value` allocated memory
/// and free the memory at the end of the life
fn stmt_sqlite3_values_allocated() -> MutexGuard<'static, HashMap<Ptr, Vec<Ptr>>> {
    static STMT_SQLITE3_VALUES_ALLOCATED: Lazy<Mutex<HashMap<Ptr, Vec<Ptr>>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    STMT_SQLITE3_VALUES_ALLOCATED
        .lock()
        .expect("acquire stmt sqlite3 values allocated lock failed")
}

/// Maintain a list of `sqlite3_value` allocated memory
/// and free the memory at the end of the life
fn sqlite3_values_allocated() -> MutexGuard<'static, HashMap<Ptr, AllocatedT>> {
    static SQLITE3_VALUES_ALLOCATED: Lazy<Mutex<HashMap<Ptr, AllocatedT>>> =
        Lazy::new(|| Mutex::new(HashMap::new()));
    SQLITE3_VALUES_ALLOCATED
        .lock()
        .expect("acquire sqlite3 values allocated lock failed")
}

/// Convert the dtor function pointer to i32
unsafe fn dtori32(
    arg: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> i32 {
    let dtor = std::mem::transmute::<
        ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
        isize,
    >(arg);
    if !matches!(dtor, -1 | 0) {
        // The dtor closure of sqilte-wasm does not provide a data pointer,
        // so it is currently not customizable.
        panic!("costom dtor not supported now");
    }
    dtor as i32
}

/// Make Vec<T> leak memory
///
/// `Vec::into_raw_parts` is unstable
fn vec_into_raw_parts<T>(v: Vec<T>) -> (*mut T, usize, usize) {
    let mut me = ManuallyDrop::new(v);
    (me.as_mut_ptr(), me.len(), me.capacity())
}

/// Wrap some `column*` related methods
enum ColumnCApi {
    Name,
    DatabaseName,
    OriginName,
    TableName,
    Decltype,
}

impl ColumnCApi {
    fn call(
        self,
        stmt: *mut sqlite3_stmt,
        colIdx: ::std::os::raw::c_int,
    ) -> *const ::std::os::raw::c_char {
        let sqlite3 = sqlite();
        let capi = sqlite3.capi();
        let (s, t) = match self {
            ColumnCApi::Name => (
                capi.sqlite3_column_name(stmt, colIdx),
                "sqlite3_column_name",
            ),
            ColumnCApi::DatabaseName => (
                capi.sqlite3_column_database_name(stmt, colIdx),
                "sqlite3_column_database_name",
            ),
            ColumnCApi::OriginName => (
                capi.sqlite3_column_origin_name(stmt, colIdx),
                "sqlite3_column_origin_name",
            ),
            ColumnCApi::TableName => (
                capi.sqlite3_column_table_name(stmt, colIdx),
                "sqlite3_column_table_name",
            ),
            ColumnCApi::Decltype => (
                capi.sqlite3_column_decltype(stmt, colIdx),
                "sqlite3_column_decltype",
            ),
        };

        // # Safety
        //
        // The returned string pointer is valid until either the prepared statement
        // is destroyed by sqlite3_finalize() or until the statement is automatically
        // reprepared by the first call to sqlite3_step() for a particular run or until
        // the next call to sqlite3_column_name() or sqlite3_column_name16() on the same column.
        let ret = cstring(s).into_raw();

        // We have established a mapping relationship between stmt and (col, text).
        // When sqlite3_finalize is called, all memory will be freed or
        // replaced by the same column.
        stmt_with_key_allocated()
            .entry(Ptr(stmt.cast()))
            .or_default()
            .insert(StmtKey::new(t, colIdx), AllocatedT::CString(ret));
        ret
    }
}

/// Release `text` and `blob` memory when Drop
struct WasmGuard<'a> {
    sqlite3: &'a SQLite,
    ptr: *mut u8,
    free: bool,
}

impl Drop for WasmGuard<'_> {
    fn drop(&mut self) {
        if !self.ptr.is_null() && self.free {
            self.sqlite3.wasm().dealloc(self.ptr);
        }
    }
}

/// Copy the contents of the rust blob to wasm
unsafe fn wasm_blob(
    sqlite3: &SQLite,
    blob: *const ::std::os::raw::c_void,
    len: ::std::os::raw::c_int,
    free: bool,
) -> WasmGuard {
    let wasm = sqlite3.wasm();

    let ptr = if blob.is_null() || len < 0 {
        std::ptr::null_mut()
    } else {
        let slice = slice::from_raw_parts(blob.cast(), len as usize);
        let wasm_ptr = wasm.alloc(len.max(1) as usize);
        sqlite3.poke_buf(slice, wasm_ptr);
        wasm_ptr
    };

    WasmGuard { sqlite3, ptr, free }
}

/// Copy the contents of the rust text to wasm
///
/// If the length is negative, it will be converted to cstr to get the length
unsafe fn wasm_text(
    sqlite3: &SQLite,
    text: *const ::std::os::raw::c_char,
    len: ::std::os::raw::c_int,
    free: bool,
) -> WasmGuard {
    let wasm = sqlite3.wasm();

    let ptr = if text.is_null() {
        std::ptr::null_mut()
    } else {
        let (wasm_ptr, len) = if len < 0 {
            // cstr length
            let cstr = CStr::from_ptr(text.cast_mut());
            // safety: nBytes is negative, so it is a cstr
            let len = cstr
                .to_str()
                .expect("text must be cstr because length is negative")
                .len();
            (wasm.alloc(len), len)
        } else {
            (wasm.alloc(len.max(1) as usize), len as usize)
        };
        sqlite3.poke_buf(slice::from_raw_parts(text.cast(), len), wasm_ptr);
        wasm_ptr
    };

    WasmGuard { sqlite3, ptr, free }
}

/// Copy the contents of the rust blob64 to wasm
unsafe fn wasm_blob64(
    sqlite3: &SQLite,
    blob: *const ::std::os::raw::c_void,
    len: sqlite3_uint64,
    free: bool,
) -> WasmGuard {
    if len > sqlite3_uint64::from(u32::MAX) {
        panic!("wasm32 does not support memory allocations larger than u32::MAX");
    }

    let wasm = sqlite3.wasm();

    let ptr = if blob.is_null() {
        std::ptr::null_mut()
    } else {
        let slice = slice::from_raw_parts(blob.cast(), len as usize);
        let wasm_ptr = wasm.alloc(len.max(1) as usize);
        sqlite3.poke_buf(slice, wasm_ptr);
        wasm_ptr
    };

    WasmGuard { sqlite3, ptr, free }
}

/// Copy the contents of the rust text64 to wasm
///
/// If the length is negative, it will be converted to cstr to get the length
unsafe fn wasm_text64(
    sqlite3: &SQLite,
    text: *const ::std::os::raw::c_char,
    len: sqlite3_uint64,
    free: bool,
) -> WasmGuard {
    wasm_blob64(sqlite3, text.cast(), len, free)
}

/// Open an `SQLite` database file as specified by the `filename` argument
/// and support opfs vfs on wasm platform.
///
/// See <https://www.sqlite.org/c3ref/open.html>
///
/// See <https://sqlite.org/wasm/doc/trunk/persistence.md>
#[multithread]
#[multithread]
pub unsafe fn sqlite3_open_v2(
    filename: *const ::std::os::raw::c_char,
    ppDb: *mut *mut sqlite3,
    flags: ::std::os::raw::c_int,
    vfs: *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    // using output-pointer arguments from JS
    let ptr = OutputPtr::new(sqlite3, ppDb, false);
    capi.sqlite3_open_v2(cstr!(filename), ptr.wasm_ptr.cast(), flags, cstr!(vfs))
}

/// A convenience wrapper around `sqlite3_prepare_v2()`, `sqlite3_step()`, and
/// `sqlite3_finalize()`, that allows an application to run multiple statements
/// of SQL without having to use a lot of C code.
///
/// See <https://www.sqlite.org/c3ref/exec.html>
#[multithread]
pub unsafe fn sqlite3_exec(
    db: *mut sqlite3,
    sql: *const ::std::os::raw::c_char,
    callback: ::std::option::Option<
        unsafe extern "C" fn(
            arg1: *mut ::std::os::raw::c_void,
            arg2: ::std::os::raw::c_int,
            arg3: *mut *mut ::std::os::raw::c_char,
            arg4: *mut *mut ::std::os::raw::c_char,
        ) -> ::std::os::raw::c_int,
    >,
    pCbArg: *mut ::std::os::raw::c_void,
    pzErrMsg: *mut *mut ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let callback = callback.map(|f| {
        Closure::new(
            move |values: Vec<String>, names: Vec<String>| -> ::std::os::raw::c_int {
                let mut values = values
                    .into_iter()
                    .map(|s| cstring(s).into_raw())
                    .collect::<Vec<_>>();
                let mut names = names
                    .into_iter()
                    .map(|s| cstring(s).into_raw())
                    .collect::<Vec<_>>();
                let ret = f(
                    pCbArg,
                    values.len() as ::std::os::raw::c_int,
                    values.as_mut_ptr(),
                    names.as_mut_ptr(),
                );

                // disposable data, free after use
                for value in values {
                    drop(CString::from_raw(value));
                }
                for name in names {
                    drop(CString::from_raw(name));
                }
                ret
            },
        )
    });

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    // using output-pointer arguments from JS
    let ptr = OutputPtr::new(sqlite3, pzErrMsg, true);
    capi.sqlite3_exec(
        db,
        cstr!(sql),
        callback.as_ref(),
        pCbArg,
        ptr.wasm_ptr.cast(),
    )
}

/// Destructor for the `sqlite3` object.
///
/// See <https://www.sqlite.org/c3ref/close.html>
#[multithread]
pub unsafe fn sqlite3_close(db: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_close_v2(db)
}

/// Destructor for the `sqlite3` object.
///
/// See <https://www.sqlite.org/c3ref/close.html>
#[multithread]
pub unsafe fn sqlite3_close_v2(db: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_close_v2(db)
}

/// Returns the number of rows modified, inserted or deleted by the most
/// recently completed `INSERT`, `UPDATE` or `DELETE` statement on the database
/// connection specified by the only parameter. Executing any other type of SQL
/// statement does not modify the value returned by these functions. `REturn`
/// value is undefined if the number of changes is bigger than 32 bits. Use
/// `sqlite3_changes64()` instead in these cases.
///
/// See <https://www.sqlite.org/c3ref/changes.html>
#[multithread]
pub unsafe fn sqlite3_changes(db: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_changes(db)
}

/// Causes the database connection `db` to disconnect from database `schema`
/// and then reopen `schema` as an in-memory database based on the
/// serialization contained in `data`. The serialized database `data` is
/// `dbSize` bytes in size. `bufferSize` is the size of the buffer `data`,
/// which might be larger than `dbSize`. If `bufferSize` is larger than
/// `dbSize`, and the `SQLITE_DESERIALIZE_READONLY` bit is not set in `flags`,
/// then `SQLite` is permitted to add content to the in-memory database as long
/// as the total size does not exceed `bufferSize` bytes.
///
/// **ACHTUNG:** There are severe caveats regarding memory allocations when
/// using this function in JavaScript. See
/// <https://sqlite.org/wasm/doc/trunk/api-c-style.md#sqlite3_deserialize> for
///
/// See <https://www.sqlite.org/c3ref/deserialize.html>
#[multithread]
pub unsafe fn sqlite3_deserialize(
    db: *mut sqlite3,
    schema: *const ::std::os::raw::c_char,
    data: *mut ::std::os::raw::c_uchar,
    dbSize: sqlite3_int64,
    bufferSize: sqlite3_int64,
    flags: ::std::os::raw::c_uint,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();
    let wasm = sqlite3.wasm();

    // I don't know how to handle this, so I'll leave it to sqlite3
    if data.is_null() || bufferSize < 0 {
        capi.sqlite3_deserialize(db, cstr!(schema), data, dbSize, bufferSize, flags)
    } else {
        let wasm_p_data = wasm.alloc(bufferSize.max(1) as usize);

        let slice = slice::from_raw_parts(data.cast_const(), bufferSize as usize);
        sqlite3.poke_buf(slice, wasm_p_data);

        // wasm ptr cannot be freed because it is a memory DB in SQLITE_DESERIALIZE_READONLY
        // and SQLITE_DESERIALIZE_FREEONCLOSE will automatically free it
        //
        // See https://www.sqlite.org/c3ref/c_deserialize_freeonclose.html

        capi.sqlite3_deserialize(
            db,
            cstr!(schema),
            wasm_p_data.cast(),
            dbSize,
            bufferSize,
            flags,
        )
    }
}

/// Returns a pointer to memory that is a serialization of the `schema`
/// database on database connection `db`. If `piSize` is not a NULL pointer,
/// then the size of the database in bytes is written into `*piSize`.
///
/// For an ordinary on-disk database file, the serialization is just a copy of
/// the disk file. For an in-memory database or a `"TEMP"` database, the
/// serialization is the same sequence of bytes which would be written to disk
/// if that database where backed up to disk.
///
/// See <https://www.sqlite.org/c3ref/serialize.html>
#[multithread]
pub unsafe fn sqlite3_serialize(
    db: *mut sqlite3,
    schema: *const ::std::os::raw::c_char,
    piSize: *mut sqlite3_int64,
    flags: ::std::os::raw::c_uint,
) -> *mut ::std::os::raw::c_uchar {
    unsafe fn serialized(ptr: *mut u8, len: usize, sqlite: &SQLite) -> *mut std::os::raw::c_uchar {
        let mut data = vec![0; len];
        sqlite.peek_buf(ptr, data.as_mut_slice());

        let (ret, len, cap) = vec_into_raw_parts(data);

        // Records allocated memory, which is freed when sqlite3_free is called
        allocated().insert(Ptr(ret.cast()), AllocatedT::VecU8((ret, len, cap)));

        ret
    }

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    // I don't know how to handle this, so I'll leave it to sqlite3
    if piSize.is_null() {
        capi.sqlite3_serialize(db, cstr!(schema), piSize, flags)
    } else {
        // using output-pointer arguments from JS
        let size = OutputPtr::new(sqlite3, piSize, false);
        let ptr = capi.sqlite3_serialize(db, cstr!(schema), size.wasm_ptr.cast(), flags);
        drop(size);

        let ret = serialized(ptr, *piSize as usize, sqlite3);

        // After the call, if the SQLITE_SERIALIZE_NOCOPY bit had been set,
        // the returned buffer content will remain accessible and unchanged until either the next write operation
        // on the connection or when the connection is closed, and applications must not modify the buffer.
        if flags != SQLITE_SERIALIZE_NOCOPY {
            capi.sqlite3_free(ptr.cast());
        }
        ret
    }
}

/// Calling `sqlite3_free()` with a pointer previously returned by
/// `sqlite3_malloc()` or `sqlite3_realloc()` releases that memory so that it
/// might be reused.
///
/// See <https://www.sqlite.org/c3ref/free.html>
#[multithread]
pub unsafe fn sqlite3_free(ptr: *mut ::std::os::raw::c_void) {
    // Because sqlite3 uses other wasm memory, in theory only the memory
    // copied to rust needs to be freed, such as sqlite3_serialize
    allocated().remove(&Ptr(ptr));
}

/// Add SQL function or aggregation or redefine the behavior of an existing SQL
/// function or aggregation.
///
/// See <https://www.sqlite.org/c3ref/create_function.html>
///
/// The `capi.sqlite3_create_function_v2` exposed by JS has been modified because
/// the original `helper` method is awkward to use in Rust
#[multithread]
pub unsafe fn sqlite3_create_function_v2(
    db: *mut sqlite3,
    functionName: *const ::std::os::raw::c_char,
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

    let xFunc = xFunc.map(|f| {
        Closure::new(
            move |arg1: *mut sqlite3_context,
                  arg2: ::std::os::raw::c_int,
                  arg3: *mut *mut sqlite3_value| {
                let mut values = vec![std::ptr::null_mut(); arg2 as usize];
                for (offset, value) in (0..).zip(values.iter_mut()) {
                    // peek pointer to get *mut sqlite3_value
                    sqlite().peek(arg3.offset(offset).cast(), &mut *value);
                }
                f(arg1, arg2, values.as_mut_ptr());
                // After xFunc is executed, the memory obtained by sqlite3_value,
                // such as text and blob, is freed.
                for value in values {
                    sqlite3_values_allocated().remove(&Ptr(value.cast()));
                }
            },
        )
    });

    let xStep = xStep.map(|f| {
        Closure::new(
            move |arg1: *mut sqlite3_context,
                  arg2: ::std::os::raw::c_int,
                  arg3: *mut *mut sqlite3_value| {
                let mut values = vec![std::ptr::null_mut(); arg2 as usize];
                for (offset, value) in (0..).zip(values.iter_mut()) {
                    sqlite().peek(arg3.offset(offset).cast(), &mut *value);
                }
                f(arg1, arg2, values.as_mut_ptr());
                // After xStep is executed, the memory obtained by sqlite3_value in this step,
                // such as text and blob, is freed.
                for value in values {
                    sqlite3_values_allocated().remove(&Ptr(value.cast()));
                }
            },
        )
    });

    let xFinal = xFinal.map(|f| {
        Closure::new(move |ctx: *mut sqlite3_context| {
            // If xStep has not allocated memory, this is null
            let aggreagate = sqlite().capi().sqlite3_aggregate_context(ctx, 0);
            f(ctx);

            // If it is not null, free the memory of aggregate_context (actually rust's memory),
            // see `sqlite3_aggregate_context` below for details
            if !aggreagate.is_null() {
                aggregate_allocated().remove(&Ptr(aggreagate.cast()));
            }
        })
    });

    // The sqlite-wasm callback does not provide a pApp parameter,
    // but the good news is that we can move it in.
    let xDestroy = xDestroy.map(|f| {
        Closure::new(move || {
            f(pApp);
        })
    });

    let ret = capi.sqlite3_create_function_v2_2(
        db,
        cstr!(functionName),
        nArg,
        eTextRep,
        pApp,
        xFunc.as_ref(),
        xStep.as_ref(),
        xFinal.as_ref(),
        xDestroy.as_ref(),
    );

    // Makes the closure leaky because the function is called multiple times
    if let Some(xFunc) = xFunc {
        Closure::forget(xFunc);
    }
    if let Some(xStep) = xStep {
        Closure::forget(xStep);
    }
    if let Some(xFinal) = xFinal {
        Closure::forget(xFinal);
    }
    if let Some(xDestroy) = xDestroy {
        Closure::forget(xDestroy);
    }

    ret
}

/// Set the return value of the application-defined function to be a text
/// string
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_text(
    ctx: *mut sqlite3_context,
    text: *const ::std::os::raw::c_char,
    textLen: ::std::os::raw::c_int,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_text(sqlite3, text, textLen, dtor == -1);
    capi.sqlite3_result_text(ctx, guard.ptr.cast(), textLen, dtor);
}

/// Sets the result from an application-defined function to be the `BLOB` whose
/// content is pointed to by the second parameter and which is `blobLen` bytes
/// long.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_blob(
    ctx: *mut sqlite3_context,
    blob: *const ::std::os::raw::c_void,
    blobLen: ::std::os::raw::c_int,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_blob(sqlite3, blob, blobLen, dtor == -1);
    capi.sqlite3_result_blob(ctx, guard.ptr.cast(), blobLen, dtor);
}

/// Sets the return value of the application-defined function to be the 32-bit
/// signed integer value given in the 2nd argument.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_int(ctx: *mut sqlite3_context, value: ::std::os::raw::c_int) {
    sqlite().capi().sqlite3_result_int(ctx, value);
}

/// Sets the return value of the application-defined function to be the 64-bit
/// signed integer value given in the 2nd argument.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_int64(ctx: *mut sqlite3_context, value: sqlite3_int64) {
    sqlite().capi().sqlite3_result_int64(ctx, value);
}

/// Sets the result from an application-defined function to be a floating point
/// value specified by its 2nd argument.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_double(ctx: *mut sqlite3_context, value: f64) {
    sqlite().capi().sqlite3_result_double(ctx, value);
}

/// Sets the return value of the application-defined function to be `NULL`.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_null(ctx: *mut sqlite3_context) {
    sqlite().capi().sqlite3_result_null(ctx);
}

/// Get a `sql_value*` result value from a column in the current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_value(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *mut sqlite3_value {
    let ret = sqlite().capi().sqlite3_column_value(stmt, colIdx);

    // We record the mapping relationship between stmt and sqlite_value,
    // and free the corresponding memeory (text and blob) when stmt ends.
    stmt_sqlite3_values_allocated()
        .entry(Ptr(stmt.cast()))
        .or_default()
        .push(Ptr(ret.cast()));
    ret
}

/// Returns the number of columns in the result set returned by the prepared
/// statement.
///
/// See <https://www.sqlite.org/c3ref/column_count.html>
#[multithread]
pub unsafe fn sqlite3_column_count(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_column_count(stmt)
}

/// Returns the name assigned to a particular column in the result set of a
/// `SELECT statement`.
///
/// See <https://www.sqlite.org/c3ref/column_name.html>
#[multithread]
pub unsafe fn sqlite3_column_name(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    ColumnCApi::Name.call(stmt, colIdx)
}

/// Bind a `NULL` value to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_null(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_null(stmt, idx);
    // JS capi thinks this will never fail
    SQLITE_OK
}

/// Bind a `BLOB` value to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_blob(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    blob: *const ::std::os::raw::c_void,
    n: ::std::os::raw::c_int,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_blob(sqlite3, blob, n, dtor == -1);
    capi.sqlite3_bind_blob(stmt, idx, guard.ptr.cast(), n, dtor)
}

/// Bind a `TEXT` value to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_text(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    text: *const ::std::os::raw::c_char,
    n: ::std::os::raw::c_int,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_text(sqlite3, text, n, dtor == -1);
    capi.sqlite3_bind_text(stmt, idx, guard.ptr.cast(), n, dtor)
}

/// Frees an `sqlite3_value` object previously obtained from
/// `sqlite3_value_dup()`.
///
/// See <https://www.sqlite.org/c3ref/value_dup.html>
#[multithread]
pub unsafe fn sqlite3_value_free(sqliteValue: *mut sqlite3_value) {
    // Free the dup sqlite3_value memory
    sqlite3_values_allocated().remove(&Ptr(sqliteValue.cast()));
    sqlite().capi().sqlite3_value_free(sqliteValue);
}

/// Get the size of a `BLOB` or `TEXT` value in bytes from a protected
/// `sqlite3_value` object.
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_bytes(sqliteValue: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_bytes(sqliteValue)
}

/// Extract a `TEXT` value from a protected `sqlite3_value` object.
///
/// **Achtung:** The pointer returned from this function can be invalidated by
/// subsequent calls to `sqlite3_value_bytes()` or `sqlite3_value_text()`!
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_text(
    sqliteValue: *mut sqlite3_value,
) -> *const ::std::os::raw::c_uchar {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    // Call sqlite3_value_text returns cstr, which is very confusing to me.
    // There is no such problem on the native platform.
    //
    // So here sqlite3_value_blob + sqlite3_value_bytes is used instead.
    let ptr = capi.sqlite3_value_blob(sqliteValue);
    let len = capi.sqlite3_value_bytes(sqliteValue);
    let mut data = vec![0; len as usize];
    sqlite3.peek_buf(ptr as _, data.as_mut_slice());
    let (ret, len, cap) = vec_into_raw_parts(data);

    // We record the memory allocated for sqlite3_value so that
    // it can be freed after stmt and context are finished.
    sqlite3_values_allocated().insert(Ptr(sqliteValue.cast()), AllocatedT::VecU8((ret, len, cap)));

    ret.cast_const()
}

/// Extract a `BLOB` value from a protected `sqlite3_value` object.
///
/// **Achtung:** The pointer returned from this function can be invalidated by
/// subsequent calls to `sqlite3_value_bytes` or `sqlite3_value_text()`!
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_blob(sqliteValue: *mut sqlite3_value) -> *const ::std::os::raw::c_void {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let ptr = capi.sqlite3_value_blob(sqliteValue);
    let len = capi.sqlite3_value_bytes(sqliteValue);
    let mut data = vec![0; len as usize];
    sqlite3.peek_buf(ptr as _, data.as_mut_slice());
    let (ret, len, cap) = vec_into_raw_parts(data);

    // We record the memory allocated for sqlite3_value so that
    // it can be freed after stmt and context are finished.
    sqlite3_values_allocated().insert(Ptr(sqliteValue.cast()), AllocatedT::VecU8((ret, len, cap)));
    ret.cast()
}

/// Extract a `INTEGER` value from a protected `sqlite3_value` object.
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_int(sqliteValue: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_int(sqliteValue)
}

/// Extract a 64-bit `INTEGER` value from a protected `sqlite3_value` object.
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_int64(sqliteValue: *mut sqlite3_value) -> sqlite3_int64 {
    sqlite().capi().sqlite3_value_int64(sqliteValue)
}

/// Extract a `REAL` value from a protected `sqlite3_value` object.
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_double(sqliteValue: *mut sqlite3_value) -> f64 {
    sqlite().capi().sqlite3_value_double(sqliteValue)
}

/// Get the default datatype of the value from a protected `sqlite3_value`
/// object.
///
/// See <https://www.sqlite.org/c3ref/value_blob.html>
#[multithread]
pub unsafe fn sqlite3_value_type(sqliteValue: *mut sqlite3_value) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_value_type(sqliteValue)
}

/// Makes a copy of the `sqlite3_value` object `sqliteValue` and returns a
/// pointer to that copy. The `sqlite3_value` returned is a protected
/// `sqlite3_value` object even if the input is not. If `sqliteValue is a
/// pointer value, then the result is a NULL value.
///
/// See <https://www.sqlite.org/c3ref/value_dup.html>
#[multithread]
pub unsafe fn sqlite3_value_dup(sqliteValue: *const sqlite3_value) -> *mut sqlite3_value {
    sqlite().capi().sqlite3_value_dup(sqliteValue)
}

/// Bind a double precision floating point number to a parameter in a prepared
/// statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_double(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    value: f64,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_double(stmt, idx, value)
}

/// Bind an integer number to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_int(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    value: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_int(stmt, idx, value)
}

/// Bind a 64 bit integer number to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_int64(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    value: sqlite3_int64,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_int64(stmt, idx, value)
}

/// Add a collation to a database connection.
///
/// See <https://www.sqlite.org/c3ref/create_collation.html>
#[multithread]
pub unsafe fn sqlite3_create_collation_v2(
    db: *mut sqlite3,
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

    let xCompare = xCompare.map(|f| {
        Closure::new(
            move |arg1: *mut ::std::os::raw::c_void,
                  arg2: ::std::os::raw::c_int,
                  arg3: *const ::std::os::raw::c_void,
                  arg4: ::std::os::raw::c_int,
                  arg5: *const ::std::os::raw::c_void| {
                let mut str1 = vec![0u8; arg2 as usize];
                sqlite().peek_buf(arg3 as _, str1.as_mut_slice());
                let str1 = str::from_utf8(&str1).expect("expect utf8 text");

                let mut str2 = vec![0u8; arg4 as usize];
                sqlite().peek_buf(arg5 as _, str2.as_mut_slice());
                let str2 = str::from_utf8(&str2).expect("expect utf8 text");

                f(arg1, arg2, str1.as_ptr().cast(), arg4, str2.as_ptr().cast())
            },
        )
    });

    let xDestroy = xDestroy.map(|f| Closure::new(move |arg1: *mut ::std::os::raw::c_void| f(arg1)));

    let ret = capi.sqlite3_create_collation_v2(
        db,
        // only string
        CStr::from_ptr(zName.cast_mut())
            .to_str()
            .expect("zName not utf8 text"),
        // sqlite-wasm only support SQLITE_UTF8
        eTextRep,
        pArg,
        xCompare.as_ref(),
        xDestroy.as_ref(),
    );

    // Makes the closure leaky because the collation is called multiple times
    if let Some(xCompare) = xCompare {
        Closure::forget(xCompare);
    }
    if let Some(xDestroy) = xDestroy {
        Closure::forget(xDestroy);
    }

    ret
}

/// If the most recent `sqlite3_*` API call associated with database connection
/// `db` failed, then the `sqlite3_extended_errcode(db)` interface returns the
/// extended result code for that API call, even when extended result codes are
/// disabled.
///
/// See <https://www.sqlite.org/c3ref/errcode.html>
#[multithread]
pub unsafe fn sqlite3_extended_errcode(db: *mut sqlite3) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_extended_errcode(db)
}

/// The `sqlite3_finalize()` function is called to delete a prepared statement.
/// If the most recent evaluation of the statement encountered no errors or if
/// the statement is never been evaluated, then `sqlite3_finalize()` returns
/// `SQLITE_OK`. If the most recent evaluation of statement `stmt` failed, then
/// `sqlite3_finalize(stmt)` returns the appropriate error code or extended
/// error code.
///
/// See <https://www.sqlite.org/c3ref/finalize.html>
#[multithread]
pub unsafe fn sqlite3_finalize(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    // Free all memory allocated by stmt
    stmt_with_key_allocated().remove(&Ptr(stmt.cast()));

    let mut locked = sqlite3_values_allocated();

    // Free all memory allocated by stmt sqlite3_value
    for sqlite3_value in stmt_sqlite3_values_allocated()
        .remove(&Ptr(stmt.cast()))
        .unwrap_or_default()
    {
        locked.remove(&sqlite3_value);
    }

    sqlite().capi().sqlite3_finalize(stmt)
}

/// After a prepared statement has been prepared using any of
/// `sqlite3_prepare_v2()`, `sqlite3_prepare_v3()`, `sqlite3_prepare16_v2()`,
/// or `sqlite3_prepare16_v3()` or one of the legacy interfaces
/// `sqlite3_prepare()` or `sqlite3_prepare16()`, this function must be called
/// one or more times to evaluate the statement.
///
/// See <https://www.sqlite.org/c3ref/step.html>
#[multithread]
pub unsafe fn sqlite3_step(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_step(stmt)
}

/// If the most recent `sqlite3_*` API call associated with database connection
/// `db` failed, then the `sqlite3_errmsg(db)` interface returns English-
/// language text that describes the error.
///
/// See <https://www.sqlite.org/c3ref/errcode.html>
#[multithread]
pub unsafe fn sqlite3_errmsg(db: *mut sqlite3) -> *const ::std::os::raw::c_char {
    // The application does not need to worry about freeing the result.
    // However, the error string might be overwritten or deallocated by
    // subsequent calls to other SQLite interface functions.
    // Memory to hold the error message string is managed internally and
    // must not be freed by the application.
    static ERRMSG: Lazy<Mutex<Option<AllocatedT>>> = Lazy::new(|| Mutex::new(None));
    let ret = sqlite().capi().sqlite3_errmsg(db);

    // # Safety
    //
    // The sqlite3_errmsg() and sqlite3_errmsg16() return English-language text
    // that describes the error, as either UTF-8 or UTF-16 respectively
    let raw = cstring(ret).into_raw();

    // Replace value and free previous allocated value
    ERRMSG
        .lock()
        .expect("acquire errmsg lock failed")
        .replace(AllocatedT::CString(raw));
    raw
}

/// Returns the database connection handle to which a prepared statement
/// belongs. The database connection returned by `sqlite3_db_handle` is the
/// same database connection that was the first argument to the
/// `sqlite3_prepare_v2()` call (or its variants) that was used to create the
/// statement in the first place.
///
/// See <https://www.sqlite.org/c3ref/db_handle.html>
#[multithread]
pub unsafe fn sqlite3_db_handle(stmt: *mut sqlite3_stmt) -> *mut sqlite3 {
    sqlite().capi().sqlite3_db_handle(stmt)
}

/// Called to reset a [prepared statement] object back to its initial state,
/// ready to be re-executed. Any SQL statement variables that had values bound
/// to them using the `sqlite3_bind_*()` API retain their values. Use
/// `sqlite3_clear_bindings()` to reset the bindings.
///
/// See <https://www.sqlite.org/c3ref/reset.html>
#[multithread]
pub unsafe fn sqlite3_reset(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_reset(stmt)
}

/// Compiles a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/prepare.html>
#[multithread]
pub unsafe fn sqlite3_prepare_v3(
    db: *mut sqlite3,
    sql: *const ::std::os::raw::c_char,
    nByte: ::std::os::raw::c_int,
    prepFlags: ::std::os::raw::c_uint,
    ppStmt: *mut *mut sqlite3_stmt,
    pzTail: *mut *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let guard = wasm_text(sqlite3, sql, nByte, true);
    let wasm_z_sql = guard.ptr.cast();
    // using output-pointer arguments from JS
    let pp_stmt = OutputPtr::new(sqlite3, ppStmt, false);
    let pz_tail = OutputPtr::new(sqlite3, pzTail, false);
    let ret = capi.sqlite3_prepare_v3(
        db,
        wasm_z_sql as _,
        nByte,
        prepFlags,
        pp_stmt.wasm_ptr.cast(),
        pz_tail.wasm_ptr.cast(),
    );
    drop(pz_tail);

    if !pzTail.is_null() && !(*pzTail).is_null() {
        // pzTail will point to the unused part of the statement.
        // Due to the difference between rust and sqlite ptr,
        // we can use the offset here to calculate the rust pointer.
        //
        // `c_char` size is always 1
        *pzTail =
            sql.add((*pzTail as usize - wasm_z_sql as usize) / size_of::<::std::os::raw::c_char>());
    }

    ret
}

/// Returns a copy of the pointer to the database connection (the 1st
/// parameter) of the `sqlite3_create_function()` routine that originally
/// registered the application defined function.
///
/// See <https://www.sqlite.org/c3ref/context_db_handle.html>
#[multithread]
pub unsafe fn sqlite3_context_db_handle(ctx: *mut sqlite3_context) -> *mut sqlite3 {
    sqlite().capi().sqlite3_context_db_handle(ctx)
}

/// Returns a copy of the pointer that was the `pUserData` parameter (the 5th
/// parameter) of the `sqlite3_create_function()` routine that originally
/// registered the application defined function.
///
/// See <https://www.sqlite.org/c3ref/user_data.html>
#[multithread]
pub unsafe fn sqlite3_user_data(ctx: *mut sqlite3_context) -> *mut ::std::os::raw::c_void {
    sqlite().capi().sqlite3_user_data(ctx)
}

/// Implementations of aggregate SQL functions use this routine to allocate
/// memory for storing their state.
///
/// See <https://www.sqlite.org/c3ref/aggregate_context.html>
#[multithread]
pub unsafe fn sqlite3_aggregate_context(
    ctx: *mut sqlite3_context,
    nBytes: ::std::os::raw::c_int,
) -> *mut ::std::os::raw::c_void {
    // This was originally to request memory from sqlite-wasm and modify the data,
    // but because the data is not in the same memory space, it is impossible to
    // synchronize the modification between Rust and sqlite, so only Rust's memory
    // is used here.
    //
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let ptr = capi.sqlite3_aggregate_context(ctx, nBytes);
    if ptr.is_null() {
        return std::ptr::null_mut::<::std::os::raw::c_void>();
    }

    // If it has been allocated before, it will be returned directly
    if let Some(AllocatedT::VecU8((ptr, _, _))) = aggregate_allocated().get(&Ptr(ptr.cast())) {
        return (*ptr).cast();
    }

    let mut data = vec![0; nBytes as usize];
    sqlite3.peek_buf(ptr.cast(), data.as_mut_slice());
    let (ret, len, cap) = vec_into_raw_parts(data);

    // Why use the ptr returned by sqlite3_aggregate_context as the key
    // instead of sqlite3_context?
    //
    // After testing, sqlite3_context will change in the final stage and
    // the correct pointer cannot be obtained
    aggregate_allocated().insert(Ptr(ptr.cast()), AllocatedT::VecU8((ret, len, cap)));

    ret.cast()
}

/// Cause the implemented SQL function to throw an exception.
///
/// `SQLite` uses the string pointed to by the 2nd parameter as the text of an
/// error message.
///
/// See <https://www.sqlite.org/c3ref/result_blob.html>
#[multithread]
pub unsafe fn sqlite3_result_error(
    ctx: *mut sqlite3_context,
    msg: *const ::std::os::raw::c_char,
    msgLen: ::std::os::raw::c_int,
) {
    sqlite()
        .capi()
        .sqlite3_result_error(ctx, cstr!(msg), msgLen);
}

/// Bind a `TEXT` value to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_text64(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    text: *const ::std::os::raw::c_char,
    n: sqlite3_uint64,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
    encoding: ::std::os::raw::c_uchar,
) -> ::std::os::raw::c_int {
    if encoding != SQLITE_UTF8 as u8 {
        panic!("sqlite3_bind_text64 only support utf8 encoding now");
    }

    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_text64(sqlite3, text, n, dtor == -1);

    capi.sqlite3_bind_text64(stmt, idx, guard.ptr.cast(), n, dtor, encoding)
}

/// Bind a `BLOB` value to a parameter in a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_blob64(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    blob: *const ::std::os::raw::c_void,
    n: sqlite3_uint64,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let dtor = dtori32(dtor);
    let guard = wasm_blob64(sqlite3, blob, n, dtor == -1);
    capi.sqlite3_bind_blob64(stmt, idx, guard.ptr.cast(), n, dtor)
}

/// These routines provide a means to determine the database, table, and
/// table column that is the origin of a particular result column in SELECT statement.
///
/// See <https://www.sqlite.org/c3ref/column_database_name.html>
#[multithread]
pub unsafe fn sqlite3_column_database_name(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    ColumnCApi::DatabaseName.call(stmt, colIdx)
}

/// These routines provide a means to determine the database, table, and
/// table column that is the origin of a particular result column in SELECT statement.
///
/// See <https://www.sqlite.org/c3ref/column_database_name.html>
#[multithread]
pub unsafe fn sqlite3_column_origin_name(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    ColumnCApi::OriginName.call(stmt, colIdx)
}

/// These routines provide a means to determine the database, table, and
/// table column that is the origin of a particular result column in SELECT statement.
///
/// See <https://www.sqlite.org/c3ref/column_database_name.html>
#[multithread]
pub unsafe fn sqlite3_column_table_name(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    ColumnCApi::TableName.call(stmt, colIdx)
}

/// Compiles a prepared statement.
///
/// See <https://www.sqlite.org/c3ref/prepare.html>
#[multithread]
pub unsafe fn sqlite3_prepare_v2(
    db: *mut sqlite3,
    sql: *const ::std::os::raw::c_char,
    nByte: ::std::os::raw::c_int,
    ppStmt: *mut *mut sqlite3_stmt,
    pzTail: *mut *const ::std::os::raw::c_char,
) -> ::std::os::raw::c_int {
    // sqlite3_prepare_v3() differs from sqlite3_prepare_v2() only in
    // having the extra prepFlags parameter, which is a bit array consisting
    // of zero or more of the SQLITE_PREPARE_* flags. The sqlite3_prepare_v2()
    // interface works exactly the same as sqlite3_prepare_v3() with a zero prepFlags parameter.
    sqlite3_prepare_v3(db, sql, nByte, 0, ppStmt, pzTail)
}

/// Open an `SQLite` database file as specified by the `filename` argument
///
/// See <https://www.sqlite.org/c3ref/open.html>
#[multithread]
pub unsafe fn sqlite3_open(
    filename: *const ::std::os::raw::c_char,
    ppDb: *mut *mut sqlite3,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let ptr = OutputPtr::new(sqlite3, ppDb, false);
    capi.sqlite3_open(cstr!(filename), ptr.wasm_ptr.cast())
}

/// Causes the `idx`-th parameter in prepared statement `stmt` to have an SQL
/// value of `NULL`, but to also be associated with the pointer `ptr` of type
/// `type`. `dtor` is either a `NULL pointer` or a pointer to a destructor
/// function for `ptr`. SQLite will invoke the destructor `dtor` with a single
/// argument of `ptr` when it is finished using `ptr`. The `type` parameter
/// should be a static string, preferably a string literal.
///
/// See <https://www.sqlite.org/c3ref/bind_blob.html>
#[multithread]
pub unsafe fn sqlite3_bind_pointer(
    stmt: *mut sqlite3_stmt,
    idx: ::std::os::raw::c_int,
    ptr: *mut ::std::os::raw::c_void,
    r#type: *const ::std::os::raw::c_char,
    dtor: ::std::option::Option<unsafe extern "C" fn(arg1: *mut ::std::os::raw::c_void)>,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_pointer(
        stmt,
        idx,
        ptr, // sqlite will not consume
        cstr!(r#type),
        dtori32(dtor),
    )
}

/// See <https://www.sqlite.org/c3ref/interrupt.html>
#[multithread]
pub unsafe fn sqlite3_interrupt(db: *mut sqlite3) {
    sqlite().capi().sqlite3_interrupt(db);
}

/// Used to make global configuration changes to `SQLite` in order to tune SQLite
/// to the specific needs of the application. The default configuration is
/// recommended for most applications and so this routine is usually not
/// necessary. It is provided to support rare applications with unusual needs.
///
/// See <https://www.sqlite.org/c3ref/config.html>
///
/// Currently only one parameter is supported. Add more if needed.
#[multithread]
pub unsafe fn sqlite3_config(
    op: ::std::os::raw::c_int,
    arg: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_config(op, arg)
}

/// Used to retrieve runtime status information about the performance of
/// `SQLite`, and optionally to reset various highwater marks.
///
/// See <https://www.sqlite.org/c3ref/status.html>
#[multithread]
pub unsafe fn sqlite3_status(
    op: ::std::os::raw::c_int,
    pCurrent: *mut ::std::os::raw::c_int,
    pHighwater: *mut ::std::os::raw::c_int,
    resetFlag: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let current = OutputPtr::new(sqlite3, pCurrent, false);
    let highwater = OutputPtr::new(sqlite3, pHighwater, false);

    capi.sqlite3_status(
        op,
        current.wasm_ptr.cast(),
        highwater.wasm_ptr.cast(),
        resetFlag,
    )
}

/// Used to retrieve runtime status information about the performance of
/// `SQLite`, and optionally to reset various highwater marks.
///
/// See <https://www.sqlite.org/c3ref/status.html>
#[multithread]
pub unsafe fn sqlite3_status64(
    op: ::std::os::raw::c_int,
    pCurrent: *mut sqlite3_int64,
    pHighwater: *mut sqlite3_int64,
    resetFlag: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let current = OutputPtr::new(sqlite3, pCurrent, false);
    let highwater = OutputPtr::new(sqlite3, pHighwater, false);

    capi.sqlite3_status64(
        op,
        current.wasm_ptr.cast(),
        highwater.wasm_ptr.cast(),
        resetFlag,
    )
}

/// Return the amount of memory currently checked out.
///
/// See <https://www.sqlite.org/c3ref/memory_highwater.html>
///
/// See <https://github.com/sqlite/sqlite/blob/4112a63b8fa8357133f2c8e089dcd9193fc2926b/src/malloc.c>
#[multithread]
#[multithread]
pub unsafe fn sqlite3_memory_used() -> sqlite3_int64 {
    let mut res: sqlite3_int64 = 0;
    let mut mx: sqlite3_int64 = 0;

    sqlite3_status64(
        SQLITE_STATUS_MEMORY_USED,
        &mut res as *mut _,
        &mut mx as *mut _,
        0,
    );

    res
}

/// Return the maximum amount of memory that has ever been
/// checked out since either the beginning of this process
/// or since the most recent reset.
///
/// See <https://www.sqlite.org/c3ref/memory_highwater.html>
///
/// See <https://github.com/sqlite/sqlite/blob/4112a63b8fa8357133f2c8e089dcd9193fc2926b/src/malloc.c>
#[multithread]
pub unsafe fn sqlite3_memory_highwater(resetFlag: ::std::os::raw::c_int) -> sqlite3_int64 {
    let mut res: sqlite3_int64 = 0;
    let mut mx: sqlite3_int64 = 0;

    sqlite3_status64(
        SQLITE_STATUS_MEMORY_USED,
        &mut res as *mut _,
        &mut mx as *mut _,
        resetFlag,
    );

    mx
}

/// Get the length in bytes of a `BLOB` or `TEXT` column in the current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_type(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_column_type(stmt, colIdx)
}

/// Registers a callback function to be invoked whenever a transaction is
/// committed. Any callback set by a previous call to `sqlite3_commit_hook()`
/// for the same database connection is overridden.
///
/// See <https://www.sqlite.org/c3ref/commit_hook.html>
#[multithread]
pub unsafe fn sqlite3_commit_hook(
    db: *mut sqlite3,
    hook: ::std::option::Option<
        unsafe extern "C" fn(cbArg: *mut ::std::os::raw::c_void) -> ::std::os::raw::c_int,
    >,
    cbArg: *mut ::std::os::raw::c_void,
) -> *mut ::std::os::raw::c_void {
    let hook = hook.map(|f| Closure::new(move |cbArg: *mut std::ffi::c_void| f(cbArg)));
    let ret = sqlite()
        .capi()
        .sqlite3_commit_hook(db, hook.as_ref(), cbArg);
    if let Some(hook) = hook {
        Closure::forget(hook);
    }
    ret
}

/// Causes the callback function `callback` to be invoked periodically during
/// long running calls to `sqlite3_step()` and `sqlite3_prepare()` and similar
/// for database connection `db`. An example use for this interface is to keep
/// a GUI updated during a large query.
///
/// See <https://www.sqlite.org/c3ref/progress_handler.html>
#[multithread]
pub unsafe fn sqlite3_progress_handler(
    db: *mut sqlite3,
    nOps: ::std::os::raw::c_int,
    callback: ::std::option::Option<
        unsafe extern "C" fn(cbArg: *mut ::std::os::raw::c_void) -> ::std::os::raw::c_int,
    >,
    cbArg: *mut ::std::os::raw::c_void,
) {
    let callback = callback.map(|f| Closure::new(move |cbArg: *mut std::ffi::c_void| f(cbArg)));
    sqlite()
        .capi()
        .sqlite3_progress_handler(db, nOps, callback.as_ref(), cbArg);
    if let Some(callback) = callback {
        Closure::forget(callback);
    }
}

/// The `sqlite3_rollback_hook()` interface registers a callback function to be
/// invoked whenever a transaction is rolled back. Any callback set by a
/// previous call to `sqlite3_rollback_hook()` for the same database connection
/// is overridden.
///
/// See <https://www.sqlite.org/c3ref/commit_hook.html>
#[multithread]
pub unsafe fn sqlite3_rollback_hook(
    db: *mut sqlite3,
    hook: ::std::option::Option<unsafe extern "C" fn(cbArg: *mut ::std::os::raw::c_void)>,
    cbArg: *mut ::std::os::raw::c_void,
) -> *mut ::std::os::raw::c_void {
    let hook = hook.map(|f| {
        Closure::new(move |cbArg: *mut std::ffi::c_void| {
            f(cbArg);
        })
    });
    let ret = sqlite()
        .capi()
        .sqlite3_rollback_hook(db, hook.as_ref(), cbArg);
    if let Some(hook) = hook {
        Closure::forget(hook);
    }
    ret
}

/// Registers a callback function with the database connection identified by
/// the first argument to be invoked whenever a row is updated, inserted or
/// deleted in a rowid table. Any callback set by a previous call to this
/// function for the same database connection is overridden.
///
/// See <https://www.sqlite.org/c3ref/update_hook.html>
#[multithread]
pub unsafe fn sqlite3_update_hook(
    db: *mut sqlite3,
    xUpdate: ::std::option::Option<
        unsafe extern "C" fn(
            userCtx: *mut ::std::os::raw::c_void,
            op: ::std::os::raw::c_int,
            dbName: *const ::std::os::raw::c_char,
            tableName: *const ::std::os::raw::c_char,
            newRowId: sqlite3_int64,
        ),
    >,
    userCtx: *mut ::std::os::raw::c_void,
) -> *mut ::std::os::raw::c_void {
    let xUpdate = xUpdate.map(|f| {
        Closure::new(
            move |userCtx: *mut std::ffi::c_void,
                  op: ::std::os::raw::c_int,
                  dbName: String,
                  tableName: String,
                  newRowId: sqlite3_int64| {
                let dbName = cstring(dbName);
                let tableName = cstring(tableName);
                f(userCtx, op, dbName.as_ptr(), tableName.as_ptr(), newRowId);
            },
        )
    });

    let ret = sqlite()
        .capi()
        .sqlite3_update_hook(db, xUpdate.as_ref(), userCtx);
    if let Some(xUpdate) = xUpdate {
        Closure::forget(xUpdate);
    }
    ret
}
/// Set A Busy Timeout.
///
/// Sets a `sqlite3_busy_handler` that sleeps for a specified amount of time
/// when a table is locked. The handler will sleep multiple times until at
/// least `ms` milliseconds of sleeping have accumulated. After at least `ms`
/// milliseconds of sleeping, the handler returns 0 which causes
/// `sqlite3_step()` to return `SQLITE_BUSY`.
///
/// See <https://www.sqlite.org/c3ref/busy_timeout.html>
#[multithread]
pub unsafe fn sqlite3_busy_timeout(
    db: *mut sqlite3,
    ms: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_busy_timeout(db, ms)
}

/// Usually returns the `rowid` of the most recent successful `INSERT` into a
/// rowid table or virtual table on database connection `db`. Inserts into
/// `WITHOUT ROWID` tables are not recorded. If no successful `INSERT`s into
/// rowid tables have ever occurred on the database connection `db`, then
/// `sqlite3_last_insert_rowid(db)` returns zero
///
/// See <https://www.sqlite.org/c3ref/last_insert_rowid.html>
#[multithread]
pub unsafe fn sqlite3_last_insert_rowid(db: *mut sqlite3) -> sqlite3_int64 {
    sqlite().capi().sqlite3_last_insert_rowid(db)
}

/// Used to find the number of SQL parameters in a prepared statement. SQL
/// parameters are tokens of the form `?`, `?NNN`, `:AAA`, `$AAA`, or `@AAA`
/// that serve as placeholders for values that are bound to the parameters at a
/// later time.
///
/// See <https://www.sqlite.org/c3ref/bind_parameter_count.html>
#[multithread]
pub unsafe fn sqlite3_bind_parameter_count(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_bind_parameter_count(stmt)
}

/// See <https://www.sqlite.org/c3ref/bind_parameter_name.html>
#[multithread]
pub unsafe fn sqlite3_bind_parameter_name(
    stmt: *mut sqlite3_stmt,
    nth: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    let ret = sqlite().capi().sqlite3_bind_parameter_name(stmt, nth);
    let raw = cstring(ret).into_raw();

    stmt_with_key_allocated()
        .entry(Ptr(stmt.cast()))
        .or_default()
        .insert(
            StmtKey::new("sqlite3_bind_parameter_name", nth),
            AllocatedT::CString(raw),
        );

    raw
}

/// Use this routine to reset all host parameters to NULL.
///
/// See <https://www.sqlite.org/c3ref/clear_bindings.html>
#[multithread]
pub unsafe fn sqlite3_clear_bindings(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_clear_bindings(stmt)
}

/// Returns the initial data type of the result column in the current result
/// row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_bytes(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_column_bytes(stmt, colIdx)
}

/// Get a BLOB result value from a column in the current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_blob(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_void {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let ptr = capi.sqlite3_column_blob(stmt, colIdx);
    let len = capi.sqlite3_column_bytes(stmt, colIdx) as usize;

    let mut data = vec![0; len];
    sqlite3.peek_buf(ptr as _, data.as_mut_slice());
    let (ret, len, cap) = vec_into_raw_parts(data);

    stmt_with_key_allocated()
        .entry(Ptr(stmt.cast()))
        .or_default()
        .insert(
            StmtKey::new("sqlite3_column_blob", colIdx),
            AllocatedT::VecU8((ret, len, cap)),
        );

    ret.cast()
}

/// See <https://www.sqlite.org/c3ref/column_decltype.html>
#[multithread]
pub unsafe fn sqlite3_column_decltype(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> *const ::std::os::raw::c_char {
    ColumnCApi::Decltype.call(stmt, colIdx)
}

/// Get a double precision floating point result value from a column in the
/// current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_double(stmt: *mut sqlite3_stmt, colIdx: ::std::os::raw::c_int) -> f64 {
    sqlite().capi().sqlite3_column_double(stmt, colIdx)
}

/// Get an integer result value from a column in the current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_int(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_column_int(stmt, colIdx)
}

/// Get a 64bit integer result value from a column in the current result row.
///
/// See <https://www.sqlite.org/c3ref/column_blob.html>
#[multithread]
pub unsafe fn sqlite3_column_int64(
    stmt: *mut sqlite3_stmt,
    colIdx: ::std::os::raw::c_int,
) -> sqlite3_int64 {
    sqlite().capi().sqlite3_column_int64(stmt, colIdx)
}

/// Returns a pointer to a copy of the UTF-8 SQL text used to create prepared
/// statement `stmt` if `stmt` was created by `sqlite3_prepare_v2()` or
/// `sqlite3_prepare_v3()`.
///
/// See <https://www.sqlite.org/c3ref/sql.html>
#[multithread]
pub unsafe fn sqlite3_sql(stmt: *mut sqlite3_stmt) -> *const ::std::os::raw::c_char {
    let ret = sqlite().capi().sqlite3_sql(stmt);
    let raw = cstring(ret).into_raw();
    stmt_with_key_allocated()
        .entry(Ptr(stmt.cast()))
        .or_default()
        .insert(StmtKey::new("sqlite3_sql", 0), AllocatedT::CString(raw));
    raw
}

/// Returns true (non-zero) if and only if the prepared statement `stmt` makes
/// no direct changes to the content of the database file.
///
/// See <https://www.sqlite.org/c3ref/stmt_readonly.html>
#[multithread]
pub unsafe fn sqlite3_stmt_readonly(stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_stmt_readonly(stmt)
}

/// Returns information about column `colName` of table `tblName` in database
/// `dbName` on database connection `db`. The `sqlite3_table_column_metadata()`
/// interface returns `SQLITE_OK` and fills in the non-NULL pointers in the
/// final five arguments with appropriate values if the specified column
/// exists. The `sqlite3_table_column_metadata()` interface returns
/// `SQLITE_ERROR` if the specified column does not exist.
///
/// See <https://www.sqlite.org/c3ref/table_column_metadata.html>
#[multithread]
pub unsafe fn sqlite3_table_column_metadata(
    db: *mut sqlite3,
    zDbName: *const ::std::os::raw::c_char,
    zTableName: *const ::std::os::raw::c_char,
    zColumnName: *const ::std::os::raw::c_char,
    pzDataType: *mut *const ::std::os::raw::c_char,
    pzCollSeq: *mut *const ::std::os::raw::c_char,
    pNotNull: *mut ::std::os::raw::c_int,
    pPrimaryKey: *mut ::std::os::raw::c_int,
    pAutoinc: *mut ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    let sqlite3 = sqlite();
    let capi = sqlite3.capi();

    let data_type = OutputPtr::new(sqlite3, pzDataType, true);
    let coll_seq = OutputPtr::new(sqlite3, pzCollSeq, true);
    let not_null = OutputPtr::new(sqlite3, pNotNull, false);
    let primary_key = OutputPtr::new(sqlite3, pPrimaryKey, false);
    let autoinc = OutputPtr::new(sqlite3, pAutoinc, false);

    capi.sqlite3_table_column_metadata(
        db,
        cstr!(zDbName),
        cstr!(zTableName),
        cstr!(zColumnName),
        data_type.wasm_ptr.cast(),
        coll_seq.wasm_ptr.cast(),
        not_null.wasm_ptr.cast(),
        primary_key.wasm_ptr.cast(),
        autoinc.wasm_ptr.cast(),
    )
}

/// Returns a pointer to the metadata associated by the
/// `sqlite3_set_auxdata(ctx, n , pAux, xDelete)` function with the `n`th
/// argument value to the application-defined function. `n` is zero for the
/// left-most function argument.
///
/// See <https://www.sqlite.org/c3ref/get_auxdata.html>
#[multithread]
pub unsafe fn sqlite3_get_auxdata(
    ctx: *mut sqlite3_context,
    n: ::std::os::raw::c_int,
) -> *mut ::std::os::raw::c_void {
    sqlite().capi().sqlite3_get_auxdata(ctx, n)
}

/// Enables or disables the `extended result codes` feature of SQLite. The
/// extended result codes are disabled by default for historical
/// compatibility.
///
/// See <https://www.sqlite.org/c3ref/extended_result_codes.html>
#[multithread]
pub unsafe fn sqlite3_extended_result_codes(
    db: *mut sqlite3,
    onoff: ::std::os::raw::c_int,
) -> ::std::os::raw::c_int {
    sqlite().capi().sqlite3_extended_result_codes(db, onoff)
}

/// Saves `pAux` as metadata for the `n`-th argument of the application-defined
/// function.
///
/// See <https://www.sqlite.org/c3ref/get_auxdata.html>
#[multithread]
pub unsafe fn sqlite3_set_auxdata(
    ctx: *mut sqlite3_context,
    n: ::std::os::raw::c_int,
    pAux: *mut ::std::os::raw::c_void,
    xDelete: ::std::option::Option<unsafe extern "C" fn(pAux: *mut ::std::os::raw::c_void)>,
) {
    let xDelete = xDelete.map(|f| Closure::new(move || f(pAux)));
    sqlite()
        .capi()
        .sqlite3_set_auxdata(ctx, n, pAux, xDelete.as_ref());
    if let Some(xDelete) = xDelete {
        xDelete.forget();
    }
}
