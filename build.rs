use std::path::{Path, PathBuf};

// SQLite compile flags tuned for WASM: no threads/dlopen, keep common extensions.
const FULL_FEATURED: [&str; 23] = [
    "-DSQLITE_OS_OTHER",
    "-DSQLITE_USE_URI",
    // All SQLite calls must remain single-threaded.
    "-DSQLITE_THREADSAFE=0",
    "-DSQLITE_TEMP_STORE=2",
    "-DSQLITE_DEFAULT_CACHE_SIZE=-16384",
    "-DSQLITE_DEFAULT_PAGE_SIZE=8192",
    "-DSQLITE_OMIT_DEPRECATED",
    // No dlopen on wasm32-unknown-unknown.
    "-DSQLITE_OMIT_LOAD_EXTENSION",
    // Shared cache is unused.
    "-DSQLITE_OMIT_SHARED_CACHE",
    "-DSQLITE_ENABLE_UNLOCK_NOTIFY",
    "-DSQLITE_ENABLE_API_ARMOR",
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

#[cfg(feature = "sqlite3mc")]
const SQLITE3_MC_FEATURED: [&str; 2] = ["-D__WASM__", "-DARGON2_NO_THREADS"];

const UPDATE_BINDGEN_ENV: &str = "SQLITE_WASM_RS_UPDATE_BINDGEN";
const SOURCE_DIR_ENV: &str = "SQLITE_WASM_RS_SOURCE_DIR";

fn main() {
    println!("cargo::rerun-if-env-changed={UPDATE_BINDGEN_ENV}");
    println!("cargo::rerun-if-env-changed={SOURCE_DIR_ENV}");
    println!("cargo::rerun-if-changed=shim");

    #[cfg(feature = "sqlite3mc")]
    let (default_dir, source_name, header_name) = (
        sqlite3mc_src::source_dir().to_path_buf(),
        sqlite3mc_src::SOURCE_FILE,
        sqlite3mc_src::HEADER_FILE,
    );
    #[cfg(not(feature = "sqlite3mc"))]
    let (default_dir, source_name, header_name) =
        (PathBuf::from("sqlite3"), "sqlite3.c", "sqlite3.h");

    let source_dir = match std::env::var_os(SOURCE_DIR_ENV) {
        Some(dir) => {
            assert!(!dir.is_empty(), "{SOURCE_DIR_ENV} must not be empty");
            PathBuf::from(dir)
        }
        None => default_dir,
    };
    let source = source_dir.join(source_name);
    let header = source_dir.join(header_name);
    for file in [&source, &header] {
        assert!(
            file.is_file(),
            "SQLite amalgamation file not found: {} (check {SOURCE_DIR_ENV} and the sqlite3mc feature)",
            file.display()
        );
    }
    // Watch the directory too, including any headers used by a custom amalgamation.
    println!("cargo::rerun-if-changed={}", source_dir.display());

    compile(&source);

    #[cfg(feature = "bindgen")]
    {
        let update_bindgen = std::env::var(UPDATE_BINDGEN_ENV).is_ok();
        let output = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR env not set"))
            .join("bindgen.rs");
        bindgen(&header, &output);

        if update_bindgen {
            #[cfg(not(feature = "sqlite3mc"))]
            const SQLITE3_BINDGEN: &str = "src/bindings/sqlite3_bindgen.rs";
            #[cfg(feature = "sqlite3mc")]
            const SQLITE3_BINDGEN: &str = "src/bindings/sqlite3mc_bindgen.rs";
            std::fs::copy(&output, SQLITE3_BINDGEN).unwrap();
        }
    }
}

#[cfg(feature = "bindgen")]
fn bindgen(header: &Path, output: &Path) {
    use bindgen::callbacks::{IntKind, ParseCallbacks};

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
        // Keep generated bindings compatible with the MSRV on newer toolchains too.
        .rust_target(
            env!("CARGO_PKG_RUST_VERSION")
                .parse()
                .expect("invalid package rust-version"),
        )
        .rust_edition(bindgen::RustEdition::Edition2021)
        .default_macro_constant_type(bindgen::MacroTypeVariation::Signed)
        .disable_nested_struct_naming()
        .generate_cstr(true)
        // SQLite's C/HTML comments are not Rust documentation tests.
        .generate_comments(false)
        .trust_clang_mangling(false)
        .header(
            header
                .to_str()
                .expect("SQLite header path must be valid UTF-8"),
        )
        .parse_callbacks(Box::new(SqliteTypeChooser));

    bindings = bindings
        .blocklist_function("sqlite3_auto_extension")
        .raw_line(
            r#"extern "C" {
    pub fn sqlite3_auto_extension(
        xEntryPoint: ::core::option::Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                pzErrMsg: *mut *mut ::core::ffi::c_char,
                _: *const sqlite3_api_routines,
            ) -> ::core::ffi::c_int,
        >,
    ) -> ::core::ffi::c_int;
}"#,
        )
        .blocklist_function("sqlite3_cancel_auto_extension")
        .raw_line(
            r#"extern "C" {
    pub fn sqlite3_cancel_auto_extension(
        xEntryPoint: ::core::option::Option<
            unsafe extern "C" fn(
                db: *mut sqlite3,
                pzErrMsg: *mut *mut ::core::ffi::c_char,
                _: *const sqlite3_api_routines,
            ) -> ::core::ffi::c_int,
        >,
    ) -> ::core::ffi::c_int;
}"#,
        )
        // Block functions related to dynamic library loading, which is not available.
        .blocklist_function("sqlite3_load_extension")
        .raw_line(
            r#"pub unsafe fn sqlite3_load_extension(
    _db: *mut sqlite3,
    _zFile: *const ::core::ffi::c_char,
    _zProc: *const ::core::ffi::c_char,
    _pzErrMsg: *mut *mut ::core::ffi::c_char,
) -> ::core::ffi::c_int {
    // SQLITE_ERROR
    1
}"#,
        )
        .blocklist_function("sqlite3_enable_load_extension")
        .raw_line(
            r#"pub unsafe fn sqlite3_enable_load_extension(
    _db: *mut sqlite3,
    _onoff: ::core::ffi::c_int,
) -> ::core::ffi::c_int {
    // SQLITE_ERROR
    1
}"#,
        )
        // Match SQLITE_OMIT_DEPRECATED.
        .blocklist_function("sqlite3_profile")
        .blocklist_function("sqlite3_trace")
        // Exclude UTF-16 entrypoints to keep the WASM surface minimal.
        .blocklist_function(".*16.*")
        .blocklist_function("sqlite3_close_v2")
        .blocklist_function("sqlite3_create_collation")
        .blocklist_function("sqlite3_create_function")
        .blocklist_function("sqlite3_create_module")
        .blocklist_function("sqlite3_prepare");

    bindings = bindings.clang_args(FULL_FEATURED);

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
        // Workaround for bindgen issue #1941, ensuring symbols are public.
        // https://github.com/rust-lang/rust-bindgen/issues/1941
        .clang_arg("-fvisibility=default");

    let bindings = bindings
        .layout_tests(false)
        .use_core()
        .formatter(bindgen::Formatter::Prettyplease)
        .generate()
        .unwrap();

    bindings.write_to_file(output).unwrap();
}

fn compile(source: &Path) {
    const C_SOURCE: [&str; 36] = [
        // string
        "string/memchr.c",
        "string/memrchr.c",
        "string/stpcpy.c",
        "string/stpncpy.c",
        "string/strcat.c",
        "string/strchr.c",
        "string/strchrnul.c",
        "string/strcmp.c",
        "string/strcpy.c",
        "string/strcspn.c",
        "string/strlen.c",
        "string/strncat.c",
        "string/strncmp.c",
        "string/strncpy.c",
        "string/strrchr.c",
        "string/strspn.c",
        // stdlib
        "stdlib/atoi.c",
        "stdlib/bsearch.c",
        "stdlib/qsort.c",
        "stdlib/qsort_nr.c",
        "stdlib/strtod.c",
        "stdlib/strtol.c",
        // math
        "math/__fpclassifyl.c",
        "math/acosh.c",
        "math/asinh.c",
        "math/atanh.c",
        "math/fmodl.c",
        "math/scalbn.c",
        "math/scalbnl.c",
        "math/sqrt.c",
        "math/trunc.c",
        // errno
        "errno/__errno_location.c",
        // stdio
        "stdio/__toread.c",
        "stdio/__uflow.c",
        // internal
        "internal/floatscan.c",
        "internal/shgetc.c",
    ];

    let mut cc = cc::Build::new();
    cc.warnings(false)
        .flag("-Wno-macro-redefined")
        .include("shim")
        .include("shim/musl/arch/generic")
        .include("shim/musl/include")
        .file("shim/printf/printf.c")
        .file(source)
        .files(C_SOURCE.map(|s| format!("shim/musl/{s}")))
        .flag("-DPRINTF_ALIAS_STANDARD_FUNCTION_NAMES_HARD")
        .flag("-include")
        .flag("shim/wasm-shim.h");

    for flag in FULL_FEATURED {
        cc.flag(flag);
    }

    #[cfg(feature = "sqlite3mc")]
    for flag in SQLITE3_MC_FEATURED {
        cc.flag(flag);
    }

    cc.compile("wsqlite3");
}
