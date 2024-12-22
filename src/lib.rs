#![warn(
    explicit_outlives_requirements,
    macro_use_extern_crate,
    meta_variable_misuse,
    missing_abi,
    noop_method_call,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    unused_extern_crates,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    variant_size_differences,
    clippy::clone_on_ref_ptr,
    clippy::cognitive_complexity,
    clippy::create_dir,
    clippy::dbg_macro,
    clippy::debug_assert_with_mut_call,
    clippy::empty_line_after_outer_attr,
    clippy::fallible_impl_from,
    clippy::filetype_is_file,
    clippy::float_cmp_const,
    clippy::get_unwrap,
    clippy::if_then_some_else_none,
    clippy::imprecise_flops,
    clippy::let_underscore_must_use,
    clippy::lossy_float_literal,
    clippy::multiple_inherent_impl,
    clippy::mutex_integer,
    clippy::nonstandard_macro_braces,
    clippy::panic_in_result_fn,
    clippy::path_buf_push_overwrite,
    clippy::pedantic,
    clippy::print_stderr,
    clippy::print_stdout,
    clippy::rc_buffer,
    clippy::rc_mutex,
    clippy::rest_pat_in_fully_bound_structs,
    clippy::string_lit_as_bytes,
    clippy::string_to_string,
    clippy::suboptimal_flops,
    clippy::suspicious_operation_groupings,
    clippy::todo,
    clippy::trivial_regex,
    clippy::unimplemented,
    clippy::unnecessary_self_imports,
    clippy::unneeded_field_pattern,
    clippy::use_debug,
    clippy::use_self,
    clippy::useless_let_if_seq,
    clippy::useless_transmute,
    clippy::verbose_file_reads
)]
#![allow(clippy::non_ascii_literal)]

pub mod ffi;

use ffi::{CApi, Wasm};
use js_sys::{Object, WebAssembly::Memory};
use serde::{Deserialize, Serialize};
use std::{error::Error, fmt::Display, result::Result};
use wasm_bindgen::JsValue;

const WASM: &[u8] = include_bytes!("jswasm/sqlite3.wasm");

#[derive(Serialize)]
pub struct InitOpts {
    #[serde(rename = "wasmBinary")]
    pub wasm_binary: &'static [u8],
    #[serde(with = "serde_wasm_bindgen::preserve", rename = "wasmMemory")]
    pub wasm_memory: Memory,
    #[serde(rename = "proxyUri")]
    pub proxy_uri: String,
}

#[derive(Serialize)]
pub struct MemoryOpts {
    /// The initial size of the WebAssembly Memory, in units of WebAssembly pages.
    pub initial: usize,
    /// The maximum size the WebAssembly Memory is allowed to grow to, in units of WebAssembly pages.
    pub maximum: usize,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Version {
    #[serde(rename = "libVersion")]
    pub lib_version: String,
    #[serde(rename = "libVersionNumber")]
    pub lib_version_number: u32,
    #[serde(rename = "sourceId")]
    pub source_id: String,
    #[serde(rename = "downloadVersion")]
    pub download_version: u32,
}

#[derive(Debug)]
pub enum SQLiteError {
    Memory(JsValue),
    Module(JsValue),
    Serde(serde_wasm_bindgen::Error),
}

impl Display for SQLiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Memory(msg) => f.debug_tuple("Memory").field(msg).finish(),
            Self::Module(msg) => f.debug_tuple("Module").field(msg).finish(),
            Self::Serde(msg) => f.debug_tuple("Serde").field(msg).finish(),
        }
    }
}

impl Error for SQLiteError {}

pub struct SQLiteOpts {
    pub memory: MemoryOpts,
}

pub struct SQLite {
    ffi: ffi::SQLite,
    version: Version,
}

impl SQLite {
    pub const DEFAULT_OPTIONS: SQLiteOpts = SQLiteOpts {
        memory: MemoryOpts {
            initial: 256,
            maximum: 32768,
        },
    };

    /// # Errors
    ///
    /// same as `SQLite::new()`
    ///
    pub async fn default() -> Result<Self, SQLiteError> {
        Self::new(Self::DEFAULT_OPTIONS).await
    }

    /// # Errors
    ///
    /// `SQLiteError::Memory`: the wrong range is configured
    ///
    /// `SQLiteError::Module`: error in initializing module
    ///
    /// `SQLiteError::Serde`: serialization and deserialization errors
    ///
    pub async fn new(opts: SQLiteOpts) -> Result<Self, SQLiteError> {
        let proxy_uri = wasm_bindgen::link_to!(module = "/src/jswasm/sqlite3-opfs-async-proxy.js");

        let wasm_memory = serde_wasm_bindgen::to_value(&opts.memory).map_err(SQLiteError::Serde)?;
        let wasm_memory = Memory::new(&Object::from(wasm_memory)).map_err(SQLiteError::Memory)?;

        let opts = InitOpts {
            wasm_binary: WASM,
            wasm_memory,
            proxy_uri,
        };

        let opts = serde_wasm_bindgen::to_value(&opts).map_err(SQLiteError::Serde)?;
        let module = ffi::SQLite::init(&Object::from(opts))
            .await
            .map_err(SQLiteError::Module)?;

        let sqlite = ffi::SQLite::new(module);

        let version =
            serde_wasm_bindgen::from_value(sqlite.version()).map_err(SQLiteError::Serde)?;

        let sqlite = Self {
            ffi: sqlite,
            version,
        };

        Ok(sqlite)
    }

    #[must_use]
    pub fn version(&self) -> &Version {
        &self.version
    }

    #[must_use]
    pub fn capi(&self) -> CApi {
        self.ffi.handle().capi()
    }

    #[must_use]
    pub fn wasm(&self) -> Wasm {
        self.ffi.handle().wasm()
    }
}
