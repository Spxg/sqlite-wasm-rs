fn main() {
    println!("cargo::rerun-if-changed=src/library");
    let path = std::env::current_dir().unwrap().join("library");
    let lib_path = path.to_str().unwrap();

    println!("cargo:rustc-link-search=native={lib_path}");
    if cfg!(feature = "custom-libc") {
        println!("cargo:rustc-link-lib=static=sqlite3");
    } else {
        println!("cargo:rustc-link-lib=static=sqlite3linked");
    }
}
