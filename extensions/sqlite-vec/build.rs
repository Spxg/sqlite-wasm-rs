fn main() {
    let mut cc = cc::Build::new();

    cc.warnings(false)
        .flag("-Wno-macro-redefined")
        .include("cc/shim/musl/arch/generic")
        .include("cc/shim/musl/include")
        .file("cc/sqlite-vec.c")
        .flag("-include")
        .flag("cc/shim/wasm-shim.h")
        .flag("-D__COSMOPOLITAN__")
        .flag("-DSQLITE_CORE")
        .compile("wsqlite_vec0");
}
