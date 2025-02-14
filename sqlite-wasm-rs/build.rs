#![allow(deprecated)]

use std::fs;

use bindgen::{
    callbacks::{IntKind, ParseCallbacks},
    RustEdition::Edition2021,
    RustTarget,
};
use xshell::{cmd, Shell};

const UPDATE_LIB_ENV: &str = "SQLITE_WASM_RS_UPDATE_PREBUILD";

fn main() {
    if !cfg!(feature = "shim") {
        return;
    }

    let path = std::env::current_dir().unwrap().join("library");
    let lib_path = path.to_str().unwrap();

    if cfg!(feature = "bundled") {
        println!("cargo::rerun-if-env-changed={UPDATE_LIB_ENV}");
        println!("cargo::rerun-if-changed=source");

        let update_precompiled = std::env::var(UPDATE_LIB_ENV).is_ok();
        let output = std::env::var("OUT_DIR").expect("OUT_DIR env not set");

        if cfg!(feature = "buildtime-bindgen") {
            bindgen(&output);
        }

        compile(&output, update_precompiled);

        if update_precompiled {
            fs::copy(
                format!("{output}/libsqlite3linked.a"),
                "library/libsqlite3linked.a",
            )
            .unwrap();
            fs::copy(format!("{output}/libsqlite3.a"), "library/libsqlite3.a").unwrap();

            if cfg!(feature = "buildtime-bindgen") {
                fs::copy(
                    format!("{output}/bindings.rs"),
                    "src/shim/libsqlite3/bindings.rs",
                )
                .unwrap();
            }
        }
        static_linking(&output);
    } else {
        println!("cargo::rerun-if-changed=library");

        static_linking(lib_path);
    }
}

fn static_linking(lib_path: &str) {
    println!("cargo:rustc-link-search=native={lib_path}");
    if cfg!(feature = "custom-libc") {
        println!("cargo:rustc-link-lib=static=sqlite3");
    } else {
        println!("cargo:rustc-link-lib=static=sqlite3linked");
    }
}

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

static COMMON: [&str; 7] = [
    // wasm is single-threaded
    "-DSQLITE_THREADSAFE=0",
    "-DSQLITE_TEMP_STORE=2",
    "-DSQLITE_OS_OTHER",
    "-DSQLITE_ENABLE_MATH_FUNCTIONS",
    "-DSQLITE_USE_URI=1",
    "-DSQLITE_OMIT_DEPRECATED",
    // there is no dlopen on this platform.
    "-DSQLITE_OMIT_LOAD_EXTENSION",
];

static FULL_FEATURED: [&str; 12] = [
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

fn bindgen(output: &str) {
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

    bindings = bindings.clang_args(FULL_FEATURED);

    bindings = bindings
        .blocklist_function("sqlite3_vmprintf")
        .blocklist_function("sqlite3_vsnprintf")
        .blocklist_function("sqlite3_str_vappendf")
        .blocklist_type("va_list")
        .blocklist_item("__.*");

    bindings = bindings
        .rust_edition(Edition2021)
        .rust_target(RustTarget::Stable_1_77)
        // Unfortunately, we need to specify the target
        // because `wasm32-unknown-unknown` cannot codegen anything.
        .clang_arg("--target=x86_64-unknown-linux-gnu");

    let bindings = bindings
        .layout_tests(false)
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .unwrap();

    bindings
        .write_to_file(format!("{output}/bindings.rs"))
        .unwrap();
}

fn compile(output: &str, build_all: bool) {
    let sh = Shell::new().unwrap();

    if cmd!(sh, "emcc -v").read().is_err() {
        panic!("
It looks like you don't have the emscripten toolchain: https://emscripten.org/docs/getting_started/downloads.html,
or use the precompiled binaries via the `default-features = false` and `precompiled` feature flag.
");
    }

    if !cfg!(feature = "custom-libc") || build_all {
        cmd!(sh, "emcc {COMMON...} {FULL_FEATURED...} source/sqlite3.c source/wasm-shim.c -o {output}/sqlite3.o -I source -r -Oz -lc").read().unwrap();

        cmd!(
            sh,
            "emar rcs {output}/libsqlite3linked.a {output}/sqlite3.o"
        )
        .read()
        .unwrap();
    }

    if cfg!(feature = "custom-libc") || build_all {
        cmd!(
            sh,
            "emcc {COMMON...} {FULL_FEATURED...} source/sqlite3.c -o {output}/sqlite3.o -r -Oz"
        )
        .read()
        .unwrap();

        cmd!(sh, "emar rcs {output}/libsqlite3.a {output}/sqlite3.o")
            .read()
            .unwrap();
    }

    let _ = fs::remove_file(format!("{output}/sqlite3.o"));
}
