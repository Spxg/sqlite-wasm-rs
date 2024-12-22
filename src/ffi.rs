use js_sys::{Object, Uint8Array};
use std::ptr::NonNull;
use wasm_bindgen::{
    JsValue,
    prelude::{Closure, wasm_bindgen},
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

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct Sqlite3DbHandle {
    _unused: [u8; 0],
}

/// Define: https://github.com/sqlite/sqlite-wasm/blob/main/index.d.ts
#[wasm_bindgen]
extern "C" {
    pub type CApi;

    #[wasm_bindgen(method, getter)]
    pub const fn SQLITE_OK(this: &CApi) -> i32;
    #[wasm_bindgen(method, getter)]
    pub const fn SQLITE_ERROR(this: &CApi) -> i32;

    #[wasm_bindgen(method, getter)]
    pub const fn SQLITE_OPEN_READONLY(this: &CApi) -> i32;
    #[wasm_bindgen(method, getter)]
    pub const fn SQLITE_OPEN_READWRITE(this: &CApi) -> i32;
    #[wasm_bindgen(method, getter)]
    pub const fn SQLITE_OPEN_CREATE(this: &CApi) -> i32;

    #[wasm_bindgen(method)]
    pub fn sqlite3_open(capi: &CApi, filename: &str, db: *mut *mut Sqlite3DbHandle) -> i32;

    #[wasm_bindgen(method)]
    pub fn sqlite3_open_v2(
        capi: &CApi,
        filename: &str,
        db: *mut *mut Sqlite3DbHandle,
        flags: i32,
        vfs: &str,
    ) -> i32;

    #[wasm_bindgen(method)]
    pub fn sqlite3_exec(
        capi: &CApi,
        db: *mut Sqlite3DbHandle,
        sql: &str,
        callback: Option<&Closure<dyn FnMut(Vec<JsValue>, Vec<String>) -> i32>>,
        arg2: *mut u8,
        errmsg: *mut *mut u8,
    ) -> i32;

    #[wasm_bindgen(method)]
    pub fn sqlite3_errmsg(capi: &CApi, sqlite3: *mut Sqlite3DbHandle) -> String;
}

/// Copy from https://github.com/xmtp/sqlite-web-rs/blob/main/src/ffi/wasm.rs.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(extends = SQLiteHandle)]
    pub type Wasm;

    #[wasm_bindgen(method, js_name = "peekPtr")]
    pub fn peek_ptr(this: &Wasm, stmt: &JsValue) -> JsValue;
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
    pub fn dealloc(this: &Wasm, ptr: NonNull<u8>);

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
    pub fn copy_to_wasm(&self, from: &[u8], dst: *mut u8) {
        let heap = self.heap8u();
        let bytes: Uint8Array = from.into();
        let offset = dst as usize / size_of::<u8>();
        // Safety: wasm32, ptr is 32bit
        #[allow(clippy::cast_possible_truncation)]
        heap.set(&bytes, offset as u32);
    }

    pub fn copy_to_rust<T>(&self, from: *mut u8, dst: &mut T) {
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
        // Safety: dst is &mut T
        #[allow(unsafe_code)]
        unsafe {
            view.raw_copy_to_ptr(std::ptr::from_ref(dst) as *mut _);
        }
    }
}
