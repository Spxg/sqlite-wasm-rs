//! This module provides some "raw" C-Like interfaces for sqlite-wasm.
//! It is called "raw" because the memory needs to be handled by yourself.
//! It is not recommended to use it directly, please use the `c` module.

use crate::{
    c::{sqlite3_stmt, sqlite3_value},
    libsqlite3::{sqlite3, sqlite3_context, sqlite3_int64},
};
use js_sys::{Error, Object, WebAssembly};
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
    pub async fn init(module: &Object) -> Result<JsValue, Error>;

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

    #[wasm_bindgen(method, js_name = "pokeBuf")]
    pub fn poke_buf(
        this: &SQLite,
        memory: &WebAssembly::Memory,
        src: *const u8,
        dst: *mut u8,
        len: u32,
    );

    #[wasm_bindgen(method, js_name = "peekBuf")]
    pub fn peek_buf(
        this: &SQLite,
        memory: &WebAssembly::Memory,
        src: *const u8,
        dst: *mut u8,
        len: u32,
    );

    #[wasm_bindgen(method, js_name = "installOpfsSAHPoolVfs", catch)]
    pub async fn install_opfs_sahpool(
        this: &SQLiteHandle,
        cfg: Option<&Object>,
    ) -> Result<OpfsSAHPoolUtil, Error>;
}

#[wasm_bindgen]
extern "C" {
    pub type OpfsSAHPoolUtil;

    /// Adds n entries to the current pool.
    #[wasm_bindgen(method, js_name = "addCapacity")]
    pub async fn add_capacity(this: &OpfsSAHPoolUtil, capacity: u32);

    /// Removes up to n entries from the pool, with the caveat that
    /// it can only remove currently-unused entries.
    #[wasm_bindgen(method, js_name = "reduceCapacity")]
    pub async fn reduce_capacity(this: &OpfsSAHPoolUtil, capacity: u32);

    /// Returns the number of files currently contained in the SAH pool.
    #[wasm_bindgen(method, js_name = "getCapacity")]
    pub fn get_capacity(this: &OpfsSAHPoolUtil) -> u32;

    /// Returns the number of files from the pool currently allocated to VFS slots.
    #[wasm_bindgen(method, js_name = "getFileCount")]
    pub fn get_file_count(this: &OpfsSAHPoolUtil) -> u32;

    /// Returns an array of the names of the files currently allocated to VFS slots.
    #[wasm_bindgen(method, js_name = "getFileNames")]
    pub fn get_file_names(this: &OpfsSAHPoolUtil) -> Vec<String>;

    /// Removes up to n entries from the pool, with the caveat that it can only
    /// remove currently-unused entries.
    #[wasm_bindgen(method, js_name = "reserveMinimumCapacity")]
    pub async fn reserve_minimum_capacity(this: &OpfsSAHPoolUtil, capacity: u32);

    /// Synchronously reads the contents of the given file into a Uint8Array and returns it.
    #[wasm_bindgen(method, js_name = "exportFile", catch)]
    pub fn export_file(this: &OpfsSAHPoolUtil, file: &str) -> Result<Vec<u8>, Error>;

    /// Imports the contents of an SQLite database, provided as a byte array or ArrayBuffer,
    /// under the given name, overwriting any existing content.
    #[wasm_bindgen(method, js_name = "importDb", catch)]
    pub fn import_db(this: &OpfsSAHPoolUtil, name: &str, bytes: Vec<u8>) -> Result<(), Error>;

    /// Clears all client-defined state of all SAHs and makes all of them available
    /// for re-use by the pool.
    #[wasm_bindgen(method, js_name = "wipeFiles")]
    pub async fn wipe_files(this: &OpfsSAHPoolUtil);

    /// If a virtual file exists with the given name, disassociates it
    /// from the pool and returns true, else returns false without side effects.
    #[wasm_bindgen(method, js_name = "unlink")]
    pub fn unlink(this: &OpfsSAHPoolUtil, filename: &str);

    /// Unregisters the VFS and removes its directory from OPFS (which means all client content is destroyed).
    #[wasm_bindgen(method, js_name = "removeVfs")]
    pub async fn remove_vfs(this: &OpfsSAHPoolUtil);
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

    /// Expects its argument to be a pointer into the WASM heap memory which
    /// refers to a NUL-terminated C-style string encoded as UTF-8.
    #[wasm_bindgen(method, js_name = "cstrToJs")]
    pub fn cstr_to_js(this: &Wasm, ptr: *const ::std::os::raw::c_char) -> String;
}
