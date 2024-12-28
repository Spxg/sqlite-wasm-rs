use crate::{
    c::{sqlite3_stmt, sqlite3_value},
    libsqlite3_sys::{sqlite3, sqlite3_context, sqlite3_int64},
};
use js_sys::{Object, Uint8Array};
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

/// Define: https://github.com/sqlite/sqlite-wasm/blob/main/index.d.ts
#[wasm_bindgen]
extern "C" {
    pub type CApi;

    #[wasm_bindgen(method)]
    pub fn sqlite3_open(capi: &CApi, filename: JsValue, ppDb: *mut *mut sqlite3) -> i32;

    #[wasm_bindgen(method)]
    pub fn sqlite3_open_v2(
        capi: &CApi,
        filename: JsValue,
        ppDb: *mut *mut sqlite3,
        flags: ::std::os::raw::c_int,
        zVfs: JsValue,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_close_v2(capi: &CApi, ppDb: *mut sqlite3) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_exec(
        capi: &CApi,
        arg1: *mut sqlite3,
        sql: JsValue,
        callback: ::std::option::Option<&Closure<dyn FnMut(Vec<String>, Vec<String>) -> i32>>,
        arg2: *mut ::std::os::raw::c_void,
        errmsg: *mut *mut ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_changes(capi: &CApi, arg1: *mut sqlite3) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_errmsg(capi: &CApi, arg1: *mut sqlite3) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_serialize(
        capi: &CApi,
        db: *mut sqlite3,
        zSchema: JsValue,
        piSize: *mut sqlite3_int64,
        mFlags: ::std::os::raw::c_uint,
    ) -> *mut u8;

    #[wasm_bindgen(method)]
    pub fn sqlite3_free(capi: &CApi, arg1: *mut ::std::os::raw::c_void);

    #[wasm_bindgen(method)]
    pub fn sqlite3_create_function_v2(
        capi: &CApi,
        db: *mut sqlite3,
        zFunctionName: JsValue,
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
        arg1: *mut sqlite3_context,
        arg2: JsValue,
        arg3: ::std::os::raw::c_int,
        arg4: ::std::os::raw::c_int,
    );

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_blob(
        capi: &CApi,
        arg1: *mut sqlite3_context,
        arg2: *const ::std::os::raw::c_void,
        arg3: ::std::os::raw::c_int,
        arg4: ::std::os::raw::c_int,
    );

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_int(capi: &CApi, arg1: *mut sqlite3_context, arg2: ::std::os::raw::c_int);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_int64(capi: &CApi, arg1: *mut sqlite3_context, arg2: sqlite3_int64);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_double(capi: &CApi, arg1: *mut sqlite3_context, arg2: f64);

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_null(capi: &CApi, arg1: *mut sqlite3_context);

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_value(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        iCol: ::std::os::raw::c_int,
    ) -> *mut sqlite3_value;

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_count(capi: &CApi, pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_column_name(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        N: ::std::os::raw::c_int,
    ) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_null(capi: &CApi, arg1: *mut sqlite3_stmt, arg2: ::std::os::raw::c_int);

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_blob(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        arg2: ::std::os::raw::c_int,
        arg3: *const ::std::os::raw::c_void,
        n: ::std::os::raw::c_int,
        arg4: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_text(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        arg2: ::std::os::raw::c_int,
        arg3: *const ::std::os::raw::c_char,
        arg4: ::std::os::raw::c_int,
        arg5: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_free(capi: &CApi, ag1: *mut sqlite3_value);

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_text(capi: &CApi, arg1: *mut sqlite3_value) -> String;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_bytes(capi: &CApi, arg1: *mut sqlite3_value) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_blob(
        capi: &CApi,
        arg1: *mut sqlite3_value,
    ) -> *const ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_int(capi: &CApi, arg1: *mut sqlite3_value) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_int64(capi: &CApi, arg1: *mut sqlite3_value) -> sqlite3_int64;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_double(capi: &CApi, arg1: *mut sqlite3_value) -> f64;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_type(capi: &CApi, arg1: *mut sqlite3_value) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_value_dup(capi: &CApi, arg1: *const sqlite3_value) -> *mut sqlite3_value;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_double(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        arg2: ::std::os::raw::c_int,
        arg3: f64,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_int(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        arg2: ::std::os::raw::c_int,
        arg3: ::std::os::raw::c_int,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_bind_int64(
        capi: &CApi,
        arg1: *mut sqlite3_stmt,
        arg2: ::std::os::raw::c_int,
        arg3: sqlite3_int64,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_create_collation_v2(
        capi: &CApi,
        arg1: *mut sqlite3,
        zName: JsValue,
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
    pub fn sqlite3_finalize(capi: &CApi, pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_step(capi: &CApi, arg1: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_db_handle(capi: &CApi, arg1: *mut sqlite3_stmt) -> *mut sqlite3;

    #[wasm_bindgen(method)]
    pub fn sqlite3_reset(capi: &CApi, pStmt: *mut sqlite3_stmt) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_prepare_v3(
        capi: &CApi,
        db: *mut sqlite3,
        zSql: *const ::std::os::raw::c_char,
        nByte: ::std::os::raw::c_int,
        prepFlags: ::std::os::raw::c_uint,
        ppStmt: *mut *mut sqlite3_stmt,
        pzTail: *mut *const ::std::os::raw::c_char,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_deserialize(
        capi: &CApi,
        db: *mut sqlite3,
        zSchema: JsValue,
        pData: *mut ::std::os::raw::c_uchar,
        szDb: sqlite3_int64,
        szBuf: sqlite3_int64,
        mFlags: ::std::os::raw::c_uint,
    ) -> ::std::os::raw::c_int;

    #[wasm_bindgen(method)]
    pub fn sqlite3_context_db_handle(capi: &CApi, arg1: *mut sqlite3_context) -> *mut sqlite3;

    #[wasm_bindgen(method)]
    pub fn sqlite3_user_data(
        capi: &CApi,
        arg1: *mut sqlite3_context,
    ) -> *mut ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_aggregate_context(
        capi: &CApi,
        arg1: *mut sqlite3_context,
        nBytes: ::std::os::raw::c_int,
    ) -> *mut ::std::os::raw::c_void;

    #[wasm_bindgen(method)]
    pub fn sqlite3_result_error(
        capi: &CApi,
        arg1: *mut sqlite3_context,
        arg2: JsValue,
        arg3: ::std::os::raw::c_int,
    );
}

/// Copy from https://github.com/xmtp/sqlite-web-rs/blob/main/src/ffi/wasm.rs.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = SQLiteHandle)]
    pub type Wasm;

    #[wasm_bindgen(method, js_name = "peekPtr")]
    pub fn peek_ptr(this: &Wasm, stmt: *mut u8) -> *mut u8;
    /// The "pstack" (pseudo-stack) API is a special-purpose allocator
    /// intended solely for use with allocating small amounts of memory such
    /// as that needed for output pointers.
    /// It is more efficient than the scoped allocation API,
    /// and covers many of the use cases for that API, but it
    /// has a tiny static memory limit (with an unspecified total size no less than 4kb).
    #[wasm_bindgen(method, getter)]
    pub fn pstack(this: &Wasm) -> PStack;

    #[wasm_bindgen(method)]
    pub fn alloc(this: &Wasm, bytes: u32) -> *mut u8;

    #[wasm_bindgen(method, getter, js_name = "alloc")]
    pub fn alloc_inner(this: &Wasm) -> Alloc;

    /// Uses alloc() to allocate enough memory for the byte-length of the given JS string,
    /// plus 1 (for a NUL terminator), copies the given JS string to that memory using jstrcpy(),
    /// NUL-terminates it, and returns the pointer to that C-string.
    /// Ownership of the pointer is transfered to the caller, who must eventually pass the pointer to dealloc() to free it.
    // TODO: Avoid using this since it allocates in JS and other webassembly. Instead use technique
    // used in Statement::prepare
    #[wasm_bindgen(method, js_name = "allocCString")]
    pub fn alloc_cstring(this: &Wasm, string: String) -> *mut u8;

    /// Allocates one or more pointers as a single chunk of memory and zeroes them out.
    /// The first argument is the number of pointers to allocate.
    /// The second specifies whether they should use a "safe" pointer size (8 bytes)
    /// or whether they may use the default pointer size (typically 4 but also possibly 8).
    /// How the result is returned depends on its first argument: if passed 1, it returns the allocated memory address.
    /// If passed more than one then an array of pointer addresses is returned
    #[wasm_bindgen(method, js_name = "allocPtr")]
    pub fn alloc_ptr(this: &Wasm, how_many: u32, safe_ptr_size: bool) -> *mut u8;

    #[wasm_bindgen(method)]
    pub fn dealloc(this: &Wasm, ptr: *mut u8);

    /// View into the wasm memory reprsented as unsigned 8-bit integers
    #[wasm_bindgen(method)]
    pub fn heap8u(this: &Wasm) -> Uint8Array;
}

/// Copy from https://github.com/xmtp/sqlite-web-rs/blob/main/src/ffi/wasm.rs.
#[wasm_bindgen]
extern "C" {
    pub type PStack;

    /// allocate some memory on the PStack
    #[wasm_bindgen(method)]
    pub fn alloc(this: &PStack, bytes: u32) -> JsValue;

    /// Resolves the current pstack position pointer.
    /// should only be used in argument for `restore`
    #[wasm_bindgen(method, getter)]
    pub fn pointer(this: &PStack) -> JsValue;

    /// resolves to total number of bytes available in pstack, including any
    /// space currently allocated. compile-time constant
    #[wasm_bindgen(method, getter)]
    pub fn quota(this: &PStack) -> u32;

    // Property resolves to the amount of space remaining in the pstack
    #[wasm_bindgen(method, getter)]
    pub fn remaining(this: &PStack) -> u32;

    /// sets current pstack
    #[wasm_bindgen(method)]
    pub fn restore(this: &PStack, ptr: &JsValue);

}

/// Copy from https://github.com/xmtp/sqlite-web-rs/blob/main/src/ffi/wasm.rs.
#[wasm_bindgen]
extern "C" {
    pub type Alloc;

    /// Non-throwing version of `Wasm::Alloc`
    /// returns NULL pointer if cannot allocate
    #[wasm_bindgen(method, js_name = "impl")]
    pub fn alloc_impl(this: &Alloc, bytes: u32) -> *mut u8;
}

impl Wasm {
    pub fn poke(&self, from: &[u8], dst: *mut u8) {
        let heap = self.heap8u();
        let bytes: Uint8Array = from.into();
        let offset = dst as usize / size_of::<u8>();
        // Safety: wasm32, ptr is 32bit
        #[allow(clippy::cast_possible_truncation)]
        heap.set(&bytes, offset as u32);
    }

    pub unsafe fn peek<T>(&self, from: *mut u8, dst: &mut T) {
        let heap = self.heap8u();
        let view = Uint8Array::new_with_byte_offset_and_length(
            &heap.buffer(),
            from as u32,
            // Safety: wasm32, ptr is 32bit
            #[allow(clippy::cast_possible_truncation)]
            {
                size_of::<T>() as u32
            },
        );
        view.raw_copy_to_ptr(std::ptr::from_ref(dst) as *mut _);
    }

    pub unsafe fn peek_buf(&self, src: *mut u8, len: u32, buf: &mut [u8]) {
        let heap = self.heap8u();
        let view = Uint8Array::new_with_byte_offset_and_length(&heap.buffer(), src as u32, len);
        view.raw_copy_to_ptr(buf.as_mut_ptr())
    }
}
