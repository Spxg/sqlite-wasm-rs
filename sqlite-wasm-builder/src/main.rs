#![allow(deprecated)]

use std::process::Command;

use bindgen::{
    callbacks::{IntKind, ParseCallbacks},
    Formatter,
    RustEdition::Edition2021,
    RustTarget,
};

#[derive(Debug)]
struct SqliteTypeChooser;

impl ParseCallbacks for SqliteTypeChooser {
    fn int_macro(&self, name: &str, _value: i64) -> Option<IntKind> {
        if name == "SQLITE_SERIALIZE_NOCOPY"
            || name.starts_with("SQLITE_DESERIALIZE_")
            || name.starts_with("SQLITE_PREPARE_")
            || name.starts_with("SQLITE_TRACE_")
        {
            Some(IntKind::UInt)
        } else {
            None
        }
    }
}

fn main() {
    let common = [
        // wasm is single-threaded
        "-DSQLITE_THREADSAFE=0",
        "-DSQLITE_TEMP_STORE=2",
        "-DSQLITE_OS_OTHER",
        "-DSQLITE_ENABLE_MATH_FUNCTIONS",
        "-DSQLITE_USE_URI=1",
        "-DSQLITE_OMIT_DEPRECATED",
        // there is no dlopen on this platform.
        "-DSQLITE_OMIT_LOAD_EXTENSION",
        // -DSQLITE_THREADSAFE=0
        "-DSQLITE_OMIT_SHARED_CACHE",
    ];

    let full_featured = [
        "-DSQLITE_ENABLE_BYTECODE_VTAB",
        "-DSQLITE_ENABLE_DBPAGE_VTAB",
        "-DSQLITE_ENABLE_DBSTAT_VTAB",
        "-DSQLITE_ENABLE_FTS5",
        "-DSQLITE_ENABLE_MATH_FUNCTIONS",
        "-DSQLITE_ENABLE_OFFSET_SQL_FUNC",
        "-DSQLITE_ENABLE_PREUPDATE_HOOK",
        "-DSQLITE_ENABLE_RTREE",
        "-DSQLITE_ENABLE_SESSION",
        "-DSQLITE_ENABLE_STMTVTAB",
        "-DSQLITE_ENABLE_UNKNOWN_SQL_FUNCTION",
        "-DSQLITE_ENABLE_COLUMN_METADATA",
    ];

    let mut bindings = bindgen::builder()
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .disable_nested_struct_naming()
        .generate_cstr(true)
        .trust_clang_mangling(false)
        .header("source/sqlite3.h")
        .parse_callbacks(Box::new(SqliteTypeChooser));

    bindings = bindings
        .blocklist_function("sqlite3_auto_extension")
        .raw_line(
            r#"extern "C" {
    pub fn sqlite3_auto_extension(
        xEntryPoint: ::std::option::Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                pzErrMsg: *mut *mut ::std::os::raw::c_char,
                _: *const sqlite3_api_routines,
            ) -> ::std::os::raw::c_int,
        >,
    ) -> ::std::os::raw::c_int;
}"#,
        )
        .blocklist_function("sqlite3_cancel_auto_extension")
        .raw_line(
            r#"extern "C" {
    pub fn sqlite3_cancel_auto_extension(
        xEntryPoint: ::std::option::Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                pzErrMsg: *mut *mut ::std::os::raw::c_char,
                _: *const sqlite3_api_routines,
            ) -> ::std::os::raw::c_int,
        >,
    ) -> ::std::os::raw::c_int;
}"#,
        )
        // there is no dlopen on this platform.
        .blocklist_function("sqlite3_load_extension")
        .blocklist_function("sqlite3_enable_load_extension")
        // DSQLITE_OMIT_DEPRECATED
        .blocklist_function("sqlite3_profile")
        .blocklist_function("sqlite3_trace")
        // DSQLITE_THREADSAFE=0
        .blocklist_function("sqlite3_unlock_notify")
        .blocklist_function(".*16.*")
        .blocklist_function("sqlite3_close_v2")
        .blocklist_function("sqlite3_create_collation")
        .blocklist_function("sqlite3_create_function")
        .blocklist_function("sqlite3_create_module")
        .blocklist_function("sqlite3_prepare");

    bindings = bindings.clang_args(full_featured);

    bindings = bindings
        .blocklist_function("sqlite3_vmprintf")
        .blocklist_function("sqlite3_vsnprintf")
        .blocklist_function("sqlite3_str_vappendf")
        .blocklist_type("va_list")
        .blocklist_item("__.*");

    bindings = bindings
        .rust_edition(Edition2021)
        .rust_target(RustTarget::Stable_1_77);

    let bindings = bindings
        .layout_tests(false)
        .formatter(Formatter::None)
        .generate()
        .unwrap();

    bindings
        .write_to_file("../sqlite-wasm-rs/src/shim/libsqlite3/bindings.rs")
        .unwrap();

    let mut cmd = Command::new("emcc");

    cmd.args(common)
        .args(full_featured)
        .arg("source/sqlite3.c")
        .arg("source/wasm-shim.c")
        .arg("-o")
        .arg("sqlite3.o")
        .arg("-I")
        .arg("source")
        .arg("-r")
        .arg("-Oz")
        .arg("-lc");
    cmd.status().unwrap();

    let mut cmd = Command::new("emar");
    cmd.arg("rcs")
        .arg("../sqlite-wasm-rs/library/libsqlite3linked.a")
        .arg("sqlite3.o");
    cmd.status().unwrap();

    let mut cmd = Command::new("emcc");
    cmd.args(common)
        .args(full_featured)
        .arg("source/sqlite3.c")
        .arg("-o")
        .arg("sqlite3.o")
        .arg("-r")
        .arg("-Oz");
    cmd.status().unwrap();

    let mut cmd = Command::new("emar");
    cmd.arg("rcs")
        .arg("../sqlite-wasm-rs/library/libsqlite3.a")
        .arg("sqlite3.o");
    cmd.status().unwrap();

    let _ = std::fs::remove_file("sqlite3.o");
}
