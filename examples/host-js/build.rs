fn main() {
    println!("cargo::rerun-if-changed=imports.txt");
    println!(
        "cargo::rustc-link-arg=--allow-undefined-file={}/imports.txt",
        env!("CARGO_MANIFEST_DIR")
    );
}
