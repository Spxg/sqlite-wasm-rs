include!("cc/shim/cc.rs");

fn main() {
    let mut cc = cc::Build::new();

    cc.warnings(false)
        .flag("-Wno-macro-redefined")
        .include("cc/shim/musl/arch/generic")
        .include("cc/shim/musl/include")
        .file("cc/sqlite-vec.c")
        .flag("-D__COSMOPOLITAN__")
        .flag("-DSQLITE_CORE");

    for (from, to) in RENAME_SYMBOLS {
        cc.define(from, *to);
    }

    cc.compile("wsqlite_vec0");
}
