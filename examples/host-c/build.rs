fn main() {
    println!("cargo::rerun-if-changed=host.c");
    println!("cargo::rerun-if-changed=../../sqlite-wasm-rs.h");
    cc::Build::new()
        .file("host.c")
        .include("../..")
        .std("c11")
        .warnings_into_errors(true)
        .compile("sqlite_host");
}
