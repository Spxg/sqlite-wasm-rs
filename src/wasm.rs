//! This module provides some "raw" C-Like interfaces for sqlite-wasm.
//! It is called "raw" because the memory needs to be handled by yourself.
//! It is not recommended to use it directly, please use the `c` module.

use crate::{
    c::{sqlite3_stmt, sqlite3_value},
    libsqlite3::{sqlite3, sqlite3_context, sqlite3_int64},
};
use js_sys::{Object, Uint8Array};
use std::mem::size_of;
use wasm_bindgen::{
    prelude::{wasm_bindgen, Closure},
    JsValue,
};

// Workaround, make file copy to snippets
//
// https://github.com/rustwasm/wasm-bindgen/issues/4233
#[wasm_bindgen(module = "/src/jswasm/sqlite3-opfs-async-proxy.js")]
extern "C" {
    type Workaround;
}

#[wasm_bindgen(module = "/src/jswasm/sqlite3.js")]
extern "C" {
    pub type SQLite;

    pub type SQLiteHandle;

    #[wasm_bindgen(static_method_of = SQLite, catch)]
    pub async fn init(module: &Object) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(constructor)]
    pub fn new(module: JsValue) -> SQLite;

    #[wasm_bindgen(method, getter, js_name = "sqlite3")]
    pub fn handle(this: &SQLite) -> SQLiteHandle;

    #[wasm_bindgen(method, getter)]
    pub fn capi(this: &SQLiteHandle) -> CApi;

    #[wasm_bindgen(method, getter)]
    pub fn wasm(this: &SQLiteHandle) -> Wasm;

    #[wasm_bindgen(method)]
    pub fn version(this: &SQLite) -> JsValue;
}

/// https://github.com/sqlite/sqlite-wasm/blob/main/index.d.ts
#[wasm_bindgen]
extern "C" {
    pub type CApi;

    #[wasm_bindgen(method)]
    pub fn sqlite3_open_v2(
        capi: &CApi,
        filename: JsValue,
        ppDb: *mut *mut sqlite3,
        flags: ::std::os::raw::c_int,
        vfs: JsValue,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_close_v2(capi: &CApi, db: *mut sqlite3) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_exec(
        capi: &CApi,
        ab: *mut sqlite3,
        sql: JsValue,
        callback: ::std::option::Option<
            &Closure<dyn FnMut(Vec<String>, Vec<String>) -> ::std::os::raw::c_int>,
        >,
        pCbArg: *mut ::std::os::raw::c_void,
        pzErrMsg: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_changes(capi: &CApi, db: *mut sqlite3) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_errmsg(capi: &CApi, db: *mut sqlite3) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_serialize(
        capi: &CApi,
        db: *mut sqlite3,
        schema: JsValue,
        piSize: *mut sqlite3_int64,
        flags: ::std::os::raw::c_uint,
    ) -> *mut ::std::os::raw::c_uchar;

    #[wasm_bindgen(method)]
    pub fn sqlite3_free(capi: &CApi, ptr: *mut ::std::os::raw::c_void);

    #[wasm_bindgen(method)]
    pub fn sqlite3_create_function_v2(
        capi: &CApi,
        db: *mut sqlite3,
        functionName: JsValue,
        nArg: ::std::os::raw::c_int,
        eTextRep: ::std::os::raw::c_int,
        pApp: *mut ::std::os::raw::c_void,
        xFunc: ::std::option::Option<
            &Closure<
                dyn FnMut(*mut sqlite3_context, ::std::os::raw::c_int, *mut *mut sqlite3_value),
            >,
        >,
        xStep: ::std::option::Option<
            &Closure<
                dyn FnMut(*mut sqlite3_context, ::std::os::raw::c_int, *mut *mut sqlite3_value),
            >,
        >,
        xFinal: ::std::option::Option<&Closure<dyn FnMut(*mut sqlite3_context)>>,
        xDestroy: ::std::option::Option<&Closure<dyn FnMut()>>,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_text(
        capi: &CApi,
        ctx: *mut sqlite3_context,
        text: JsValue,
        textLen: ::std::os::raw::c_int,
        dtor: ::std::os::raw::c_int,
    );

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_blob(
        capi: &CApi,
        ctx: *mut sqlite3_context,
        blob: *const ::std::os::raw::c_void,
        blobLen: ::std::os::raw::c_int,
        dtor: ::std::os::raw::c_int,
    );

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_int(capi: &CApi, ctx: *mut sqlite3_context, value: ::std::os::raw::c_int);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_int64(capi: &CApi, ctx: *mut sqlite3_context, value: sqlite3_int64);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_double(capi: &CApi, ctx: *mut sqlite3_context, value: f64);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_null(capi: &CApi, ctx: *mut sqlite3_context);

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_value(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        colIdx: ::std::os::raw::c_int,
    ) -> *mut sqlite3_value;

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_count(capi: &CApi, stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_name(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        colIdx: ::std::os::raw::c_int,
    ) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_null(capi: &CApi, stmt: *mut sqlite3_stmt, idx: ::std::os::raw::c_int);

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_blob(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        idx: ::std::os::raw::c_int,
        blob: *const ::std::os::raw::c_void,
        n: ::std::os::raw::c_int,
        dtor: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_text(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        idx: ::std::os::raw::c_int,
        text: *const ::std::os::raw::c_char,
        n: ::std::os::raw::c_int,
        dtor: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_free(capi: &CApi, sqliteValue: *mut sqlite3_value);

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_text(capi: &CApi, sqliteValue: *mut sqlite3_value) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_bytes(
        capi: &CApi,
        sqliteValue: *mut sqlite3_value,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_blob(
        capi: &CApi,
        sqliteValue: *mut sqlite3_value,
    ) -> *const ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_int(capi: &CApi, sqliteValue: *mut sqlite3_value)
        -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_int64(capi: &CApi, sqliteValue: *mut sqlite3_value) -> sqlite3_int64;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_double(capi: &CApi, sqliteValue: *mut sqlite3_value) -> f64;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_type(
        capi: &CApi,
        sqliteValue: *mut sqlite3_value,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_dup(capi: &CApi, sqliteValue: *const sqlite3_value) -> *mut sqlite3_value;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_double(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        idx: ::std::os::raw::c_int,
        value: f64,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_int(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        idx: ::std::os::raw::c_int,
        value: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_int64(
        capi: &CApi,
        stmt: *mut sqlite3_stmt,
        idx: ::std::os::raw::c_int,
        value: sqlite3_int64,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_create_collation_v2(
        capi: &CApi,
        db: *mut sqlite3,
        zName: &str,
        eTextRep: ::std::os::raw::c_int,
        pArg: *mut ::std::os::raw::c_void,
        xCompare: ::std::option::Option<
            &Closure<
                dyn FnMut(
                    *mut ::std::os::raw::c_void,
                    ::std::os::raw::c_int,
                    *const ::std::os::raw::c_void,
                    ::std::os::raw::c_int,
                    *const ::std::os::raw::c_void,
                ) -> ::std::os::raw::c_int,
            >,
        >,
        xDestroy: ::std::option::Option<&Closure<dyn FnMut(*mut ::std::os::raw::c_void)>>,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_extended_errcode(capi: &CApi, db: *mut sqlite3) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_finalize(capi: &CApi, stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_step(capi: &CApi, stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_db_handle(capi: &CApi, stmt: *mut sqlite3_stmt) -> *mut sqlite3;

    #[wasm_bindgen(method)]
    pub fn sqlite3_reset(capi: &CApi, stmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_prepare_v3(
        capi: &CApi,
        db: *mut sqlite3,
        sql: *const ::std::os::raw::c_char,
        nByte: ::std::os::raw::c_int,
        prepFlags: ::std::os::raw::c_uint,
        ppStmt: *mut *mut sqlite3_stmt,
        pzTail: *mut *const ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_deserialize(
        capi: &CApi,
        db: *mut sqlite3,
        schema: JsValue,
        data: *mut ::std::os::raw::c_uchar,
        dbSize: sqlite3_int64,
        bufferSize: sqlite3_int64,
        flags: ::std::os::raw::c_uint,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_context_db_handle(capi: &CApi, ctx: *mut sqlite3_context) -> *mut sqlite3;

    #[wasm_bindgen(method)]
    pub fn sqlite3_user_data(capi: &CApi, ctx: *mut sqlite3_context)
        -> *mut ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_aggregate_context(
        capi: &CApi,
        ctx: *mut sqlite3_context,
        nBytes: ::std::os::raw::c_int,
    ) -> *mut ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_error(
        capi: &CApi,
        ctx: *mut sqlite3_context,
        msg: JsValue,
        msgLen: ::std::os::raw::c_int,
    );
}

/// Just like in C, WASM offers a memory "heap," and transfering values
/// between JS and WASM often requires manipulation of that memory, including
/// low-level allocation and deallocation of it.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = SQLiteHandle)]
    pub type Wasm;

    /// Allocates n bytes of memory from the WASM heap and returns
    /// the address of the first byte in the block
    #[wasm_bindgen(method)]
    pub fn alloc(this: &Wasm, bytes: usize) -> *mut u8;

    /// Frees memory returned by alloc()
    #[wasm_bindgen(method)]
    pub fn dealloc(this: &Wasm, ptr: *mut u8);

    /// View into the wasm memory reprsented as unsigned 8-bit integers
    ///
    /// It is important never to hold on to objects returned from methods
    /// like heap8u() long-term, as they may be invalidated if the heap grows.
    /// It is acceptable to hold the reference for a brief series of calls, so
    /// long as those calls are guaranteed not to allocate memory on the WASM heap,
    /// but it should never be cached for later use.
    #[wasm_bindgen(method)]
    pub fn heap8u(this: &Wasm) -> Uint8Array;

    /// Expects its argument to be a pointer into the WASM heap memory which
    /// refers to a NUL-terminated C-style string encoded as UTF-8.
    #[wasm_bindgen(method, js_name = "cstrToJs")]
    pub fn cstr_to_js(this: &Wasm, ptr: *const ::std::os::raw::c_char) -> String;
}

impl Wasm {
    /// Write buffer to wasm pointer
    pub fn poke(&self, from: &[u8], dst: *mut u8) {
        let heap = self.heap8u();
        let offset = dst as usize / size_of::<u8>();

        // Never use `&[u8]` to convert to `Uint8Array`
        // because the `Uint8Array` will be detached when the memory grows.
        for (idx, &val) in from.into_iter().enumerate() {
            heap.set_index((offset + idx) as u32, val);
        }
    }

    /// Read T size buffer from wasm pointer
    pub unsafe fn peek<T>(&self, from: *mut u8, dst: &mut T) {
        let heap = self.heap8u();
        let from = from as u32;
        let end = from + size_of::<T>() as u32;
        let view = heap.subarray(from, end);

        // Never use `raw_copy_to_ptr` etc. functions,
        // because the `Uint8Array` will be detached when the memory grows.
        let dst = std::ptr::from_ref(dst) as *mut u8;
        view.for_each(&mut |val, idx, _| {
            std::ptr::write(dst.add(idx as usize), val);
        });
    }

    /// Read specified size buffer from wasm pointer
    pub unsafe fn peek_buf(&self, from: *mut u8, len: usize, dst: &mut [u8]) {
        let heap = self.heap8u();
        let from = from as u32;
        let end = from + len as u32;
        let view = heap.subarray(from, end);

        // Never use `raw_copy_to_ptr` etc. functions,
        // because the `Uint8Array` will be detached when the memory grows.
        let dst = dst.as_mut_ptr();
        view.for_each(&mut |val, idx, _| {
            std::ptr::write(dst.add(idx as usize), val);
        });
    }
}
