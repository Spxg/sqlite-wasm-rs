#![allow(deprecated)]

#[cfg(any(feature = "bundled", feature = "buildtime-bindgen"))]
const COMMON: [&str; 7] = [
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

#[cfg(any(feature = "bundled", feature = "buildtime-bindgen"))]
const FULL_FEATURED: [&str; 12] = [
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

#[cfg(all(
    any(feature = "bundled", feature = "buildtime-bindgen"),
    feature = "sqlite3mc"
))]
const SQLITE3_MC_FEATURED: [&str; 1] = ["-D__WASM__"];

#[cfg(all(not(feature = "bundled"), feature = "precompiled"))]
fn main() {
    #[cfg(feature = "buildtime-bindgen")]
    bindgen(&std::env::var("OUT_DIR").expect("OUT_DIR env not set"));

    let path = std::env::current_dir().unwrap().join("sqlite3");
    let lib_path = path.to_str().unwrap();
    println!("cargo::rerun-if-changed=sqlite3");
    static_linking(lib_path);
}

#[cfg(all(not(feature = "precompiled"), feature = "bundled"))]
fn main() {
    const UPDATE_LIB_ENV: &str = "SQLITE_WASM_RS_UPDATE_PREBUILD";

    println!("cargo::rerun-if-env-changed={UPDATE_LIB_ENV}");
    println!("cargo::rerun-if-changed=shim");

    let update_precompiled = std::env::var(UPDATE_LIB_ENV).is_ok();
    let output = std::env::var("OUT_DIR").expect("OUT_DIR env not set");

    #[cfg(feature = "sqlite3mc")]
    println!("cargo::rerun-if-changed=sqlite3mc");

    #[cfg(not(feature = "sqlite3mc"))]
    println!("cargo::rerun-if-changed=sqlite3");

    compile(&output, update_precompiled);

    #[cfg(feature = "buildtime-bindgen")]
    bindgen(&output);

    if update_precompiled {
        #[cfg(not(feature = "sqlite3mc"))]
        {
            std::fs::copy(
                format!("{output}/libsqlite3linked.a"),
                "sqlite3/libsqlite3linked.a",
            )
            .unwrap();
            std::fs::copy(format!("{output}/libsqlite3.a"), "sqlite3/libsqlite3.a").unwrap();
        }
        #[cfg(feature = "buildtime-bindgen")]
        {
            #[cfg(not(feature = "sqlite3mc"))]
            const SQLITE3_BINDGEN: &str = "src/libsqlite3/sqlite3_bindgen.rs";
            #[cfg(feature = "sqlite3mc")]
            const SQLITE3_BINDGEN: &str = "src/libsqlite3/sqlite3mc_bindgen.rs";
            std::fs::copy(format!("{output}/bindgen.rs"), SQLITE3_BINDGEN).unwrap();
        }
    }

    static_linking(&output);
}

#[cfg(all(not(feature = "bundled"), not(feature = "precompiled")))]
fn main() {
    panic!(
        "
must set `bundled` or `precompiled` feature
"
    );
}

#[cfg(all(feature = "bundled", feature = "precompiled"))]
fn main() {
    panic!(
        "
`bundled` feature and `precompiled` feature can't use together
"
    );
}

#[cfg(any(feature = "bundled", feature = "precompiled"))]
fn static_linking(lib_path: &str) {
    println!("cargo:rustc-link-search=native={lib_path}");
    if cfg!(feature = "custom-libc") {
        println!("cargo:rustc-link-lib=static=sqlite3");
    } else {
        println!("cargo:rustc-link-lib=static=sqlite3linked");
    }
}

#[cfg(all(feature = "buildtime-bindgen"))]
fn bindgen(output: &str) {
    #[cfg(not(feature = "sqlite3mc"))]
    const SQLITE3_HEADER: &str = "sqlite3/sqlite3.h";
    #[cfg(feature = "sqlite3mc")]
    const SQLITE3_HEADER: &str = "sqlite3mc/sqlite3mc_amalgamation.h";

    use bindgen::{
        callbacks::{IntKind, ParseCallbacks},
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

    let mut bindings = bindgen::builder()
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .disable_nested_struct_naming()
        .generate_cstr(true)
        .trust_clang_mangling(false)
        .header(SQLITE3_HEADER)
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
        .blocklist_function(".*16.*")
        .blocklist_function("sqlite3_close_v2")
        .blocklist_function("sqlite3_create_collation")
        .blocklist_function("sqlite3_create_function")
        .blocklist_function("sqlite3_create_module")
        .blocklist_function("sqlite3_prepare");

    bindings = bindings.clang_args(COMMON).clang_args(FULL_FEATURED);

    #[cfg(feature = "sqlite3mc")]
    {
        bindings = bindings.clang_args(SQLITE3_MC_FEATURED);
    }

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
        .write_to_file(format!("{output}/bindgen.rs"))
        .unwrap();
}

#[cfg(feature = "bundled")]
fn compile(output: &str, build_all: bool) {
    use std::process::Command;

    #[cfg(target_os = "windows")]
    const CC: &str = "emcc.bat";
    #[cfg(target_os = "windows")]
    const AR: &str = "emar.bat";

    #[cfg(not(target_os = "windows"))]
    const CC: &str = "emcc";
    #[cfg(not(target_os = "windows"))]
    const AR: &str = "emar";

    #[cfg(not(feature = "sqlite3mc"))]
    const SQLITE3_SOURCE: &str = "sqlite3/sqlite3.c";
    #[cfg(feature = "sqlite3mc")]
    const SQLITE3_SOURCE: &str = "sqlite3mc/sqlite3mc_amalgamation.c";

    if Command::new(CC).arg("-v").status().is_err() {
        panic!("
It looks like you don't have the emscripten toolchain: https://emscripten.org/docs/getting_started/downloads.html,
or use the precompiled binaries via the `default-features = false` and `precompiled` feature flag.
");
    }

    if !cfg!(feature = "custom-libc") || build_all {
        let mut cmd = Command::new(CC);
        cmd.args(COMMON).args(FULL_FEATURED);
        #[cfg(feature = "sqlite3mc")]
        cmd.args(SQLITE3_MC_FEATURED);
        cmd.arg(SQLITE3_SOURCE)
            .arg("shim/wasm-shim.c")
            .arg("-o")
            .arg(format!("{output}/sqlite3.o"))
            .arg("-Ishim")
            .arg("-r")
            .arg("-Oz")
            .arg("-lc")
            .status()
            .expect("Failed to build sqlite3");

        let mut cmd = Command::new(AR);
        cmd.arg("rcs")
            .arg(format!("{output}/libsqlite3linked.a"))
            .arg(format!("{output}/sqlite3.o"))
            .status()
            .expect("Failed to archive sqlite3linked.o");
    }

    if cfg!(feature = "custom-libc") || build_all {
        let mut cmd = Command::new(CC);
        cmd.args(COMMON).args(FULL_FEATURED);
        #[cfg(feature = "sqlite3mc")]
        cmd.args(SQLITE3_MC_FEATURED);
        cmd.arg(SQLITE3_SOURCE)
            .arg("shim/wasm-shim.c")
            .arg("-o")
            .arg(format!("{output}/sqlite3.o"))
            .arg("-Ishim")
            .arg("-r")
            .arg("-Oz")
            .status()
            .expect("Failed to build sqlite3");

        let mut cmd = Command::new(AR);
        cmd.arg("rcs")
            .arg(format!("{output}/libsqlite3.a"))
            .arg(format!("{output}/sqlite3.o"))
            .status()
            .expect("Failed to archive sqlite3.o");
    }

    let _ = std::fs::remove_file(format!("{output}/sqlite3.o"));
}
