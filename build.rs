use std::path::{Path, PathBuf};

// SQLite compile flags tuned for WASM: no threads/dlopen, keep common extensions.
const FULL_FEATURED: &[&str] = &[
    "-DSQLITE_OS_OTHER",
    "-DSQLITE_USE_URI",
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

#[cfg(all(feature = "sqlite3mc", feature = "sqlcipher"))]
compile_error!("features `sqlite3mc` and `sqlcipher` are mutually exclusive");

struct Backend {
    name: &'static str,
    source_dir: PathBuf,
    source_name: &'static str,
    header_name: &'static str,
    flags: &'static [&'static str],
    compile: Option<fn(cc::Build, &Path)>,
}

impl Backend {
    /// Use the same SQLite configuration for C compilation and binding generation.
    fn sqlite_flags(&self) -> impl Iterator<Item = &'static str> + '_ {
        FULL_FEATURED
            .iter()
            .copied()
            .chain(self.flags.iter().copied())
    }
}

/// Select all backend-specific settings in one place.
fn backend() -> Backend {
    #[cfg(feature = "sqlcipher")]
    {
        Backend {
            name: "sqlcipher",
            source_dir: sqlcipher_src::source_dir().to_path_buf(),
            source_name: sqlcipher_src::SOURCE_FILE,
            header_name: sqlcipher_src::HEADER_FILE,
            flags: &[
                "-DSQLITE_HAS_CODEC=1",
                "-DSQLCIPHER_CRYPTO_LIBTOMCRYPT=1",
                // Install the host entropy hook before the provider initializes.
                "-DSQLITE_EXTRA_INIT=sqlcipher_wasm_extra_init",
                "-DSQLITE_EXTRA_SHUTDOWN=sqlcipher_extra_shutdown",
                // The shim has no gettimeofday, log devices or memory locking.
                "-DSQLCIPHER_OMIT_LOG=1",
                "-DSQLCIPHER_OMIT_LOG_DEVICE=1",
                "-DSQLCIPHER_OMIT_DEFAULT_LOGGING=1",
                "-DOMIT_MEMLOCK=1",
                // SQLCipher requires THREADSAFE=1 or 2, but calls stay single-threaded.
                "-DSQLITE_THREADSAFE=1",
                "-DSQLITE_MUTEX_NOOP=1",
            ],
            compile: Some(compile_sqlcipher),
        }
    }
    #[cfg(all(feature = "sqlite3mc", not(feature = "sqlcipher")))]
    {
        Backend {
            name: "sqlite3mc",
            source_dir: sqlite3mc_src::source_dir().to_path_buf(),
            source_name: sqlite3mc_src::SOURCE_FILE,
            header_name: sqlite3mc_src::HEADER_FILE,
            flags: &["-DSQLITE_THREADSAFE=0", "-D__WASM__", "-DARGON2_NO_THREADS"],
            compile: None,
        }
    }
    #[cfg(not(any(feature = "sqlite3mc", feature = "sqlcipher")))]
    {
        Backend {
            name: "sqlite3",
            source_dir: PathBuf::from("sqlite3"),
            source_name: "sqlite3.c",
            header_name: "sqlite3.h",
            flags: &["-DSQLITE_THREADSAFE=0"],
            compile: None,
        }
    }
}

const UPDATE_BINDGEN_ENV: &str = "SQLITE_WASM_RS_UPDATE_BINDGEN";
const SOURCE_DIR_ENV: &str = "SQLITE_WASM_RS_SOURCE_DIR";

fn main() {
    println!("cargo::rerun-if-env-changed={UPDATE_BINDGEN_ENV}");
    println!("cargo::rerun-if-env-changed={SOURCE_DIR_ENV}");
    println!("cargo::rerun-if-changed=shim");

    let backend = backend();
    let source_dir = match std::env::var_os(SOURCE_DIR_ENV) {
        Some(dir) => {
            assert!(!dir.is_empty(), "{SOURCE_DIR_ENV} must not be empty");
            PathBuf::from(dir)
        }
        None => backend.source_dir.clone(),
    };
    let source = source_dir.join(backend.source_name);
    let header = source_dir.join(backend.header_name);
    for file in [&source, &header] {
        assert!(
            file.is_file(),
            "{} amalgamation file not found: {} (check {SOURCE_DIR_ENV} and the selected backend feature)",
            backend.name,
            file.display()
        );
    }
    // Watch the directory too, including any headers used by a custom amalgamation.
    println!("cargo::rerun-if-changed={}", source_dir.display());

    compile(&backend, &source, &source_dir);

    #[cfg(feature = "bindgen")]
    {
        let update_bindgen = std::env::var(UPDATE_BINDGEN_ENV).is_ok();
        let output = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR env not set"))
            .join("bindgen.rs");
        bindgen(&backend, &header, &output);

        if update_bindgen {
            let destination = format!("src/bindings/{}_bindgen.rs", backend.name);
            std::fs::copy(&output, destination).unwrap();
        }
    }
}

#[cfg(feature = "bindgen")]
fn bindgen(backend: &Backend, header: &Path, output: &Path) {
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

    bindings = bindings.clang_args(backend.sqlite_flags());

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

/// Compiler settings every C unit shares, with the shim standing in for libc.
fn shim_build() -> cc::Build {
    let mut cc = cc::Build::new();
    cc.warnings(false)
        .flag("-Wno-macro-redefined")
        .include("shim")
        .include("shim/musl/arch/generic")
        .include("shim/musl/include")
        .flag("-DPRINTF_ALIAS_STANDARD_FUNCTION_NAMES_HARD")
        .flag("-include")
        .flag("shim/wasm-shim.h");
    cc
}

fn compile(backend: &Backend, source: &Path, source_dir: &Path) {
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

    let mut cc = shim_build();
    cc.file("shim/printf/printf.c")
        .file(source)
        .files(C_SOURCE.map(|s| format!("shim/musl/{s}")))
        .include(source_dir)
        .flags(backend.sqlite_flags());

    match backend.compile {
        Some(compile) => compile(cc, source_dir),
        None => cc.compile("wsqlite3"),
    }
}

/// SQLCipher and its shim share LibTomCrypt's configuration with every crypto unit.
#[cfg(feature = "sqlcipher")]
fn compile_sqlcipher(mut sqlite: cc::Build, source_dir: &Path) {
    let tomcrypt = source_dir.join(sqlcipher_src::LIBTOMCRYPT_INCLUDE_DIR);
    sqlite
        .include(&tomcrypt)
        .flag("-include")
        .flag("shim/sqlcipher/libtomcrypt.h")
        .file("shim/sqlcipher/shim.c")
        .compile("wsqlite3");

    shim_build()
        .include(&tomcrypt)
        .flag("-include")
        .flag("shim/sqlcipher/libtomcrypt.h")
        // Expose internal declarations when compiling LibTomCrypt itself.
        .define("LTC_SOURCE", None)
        .files(
            sqlcipher_src::LIBTOMCRYPT_SOURCES
                .iter()
                .map(|s| source_dir.join(s)),
        )
        .compile("wsqlcipher_ltc");
}
