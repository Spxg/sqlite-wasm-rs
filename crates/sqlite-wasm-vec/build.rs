fn main() {
    let mut cc = cc::Build::new();
    cc.warnings(false).target("wasm32-unknown-emscripten");

    cc.file("vec/sqlite-vec.c")
        .flag("-include")
        .flag("vec/shim.h")
        .define("SQLITE_CORE", None)
        .compile("sqlite_vec0");
}
