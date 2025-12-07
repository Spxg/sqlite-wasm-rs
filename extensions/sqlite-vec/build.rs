fn main() {
    let mut cc = cc::Build::new();
    cc.warnings(false).target("wasm32-unknown-emscripten");

    cc.file("cc/sqlite-vec.c")
        .flag("-include")
        .flag("cc/wasm-shim.h")
        .define("SQLITE_CORE", None)
        .compile("wsqlite_vec0");
}
