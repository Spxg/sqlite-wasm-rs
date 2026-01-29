fn main() {
    let mut cc = cc::Build::new();

    cc.warnings(false)
        .file("c/uuid7.c")
        .include("../../shim/musl/include")
        .include("../../shim/musl/arch/generic")
        .include("../../include")
        .include("../../sqlite3")
        .flag("-include")
        .flag("../../shim/wasm-shim.h")
        .flag("-DSQLITE_CORE")
        .flag("-DSQLITE_WASM")
        .flag("-DNDEBUG")
        .compile("sqlite_uuid7");
}
