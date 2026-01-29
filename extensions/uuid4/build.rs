fn main() {
    println!("cargo:rerun-if-changed=c/uuid4.c");
    println!("cargo:rerun-if-changed=../../shim/wasm-shim.h");

    let mut cc = cc::Build::new();

    cc.warnings(false)
        .file("c/uuid4.c")
        .include("../../include") // fallback or general include if any
        .include("../../shim/musl/include")
        .include("../../shim/musl/arch/generic")
        .include("../../sqlite3") // where sqlite3ext.h is
        .flag("-include")
        .flag("../../shim/wasm-shim.h")
        .flag("-DSQLITE_CORE")
        .flag("-DSQLITE_WASM")
        .flag("-DNDEBUG")
        .compile("sqlite_uuid4");
}
