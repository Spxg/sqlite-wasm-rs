#[allow(unused)]
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

#[allow(unused)]
const RENAME_SYMBOLS: &[(&str, &str)] = &[
    // string
    ("strcmp", "rust_sqlite_wasm_strcmp"),
    ("strcpy", "rust_sqlite_wasm_strcpy"),
    ("strncpy", "rust_sqlite_wasm_strncpy"),
    ("strcat", "rust_sqlite_wasm_strcat"),
    ("strncat", "rust_sqlite_wasm_strncat"),
    ("strcspn", "rust_sqlite_wasm_strcspn"),
    ("strspn", "rust_sqlite_wasm_strspn"),
    ("strncmp", "rust_sqlite_wasm_strncmp"),
    ("strrchr", "rust_sqlite_wasm_strrchr"),
    ("strchr", "rust_sqlite_wasm_strchr"),
    ("memchr", "rust_sqlite_wasm_memchr"),
    ("strlen", "rust_sqlite_wasm_strlen"),
    ("__memrchr", "rust_sqlite_wasm_memrchr"),
    ("__stpcpy", "rust_sqlite_wasm_stpcpy"),
    ("__stpncpy", "rust_sqlite_wasm_strncpy"),
    ("__strchrnul", "rust_sqlite_wasm_strchrnul"),
    // math
    ("__fpclassifyl", "rust_sqlite_wasm_fpclassifyl"),
    ("acosh", "rust_sqlite_wasm_acosh"),
    ("asinh", "rust_sqlite_wasm_asinh"),
    ("atanh", "rust_sqlite_wasm_atanh"),
    ("trunc", "rust_sqlite_wasm_trunc"),
    ("sqrt", "rust_sqlite_wasm_sqrt"),
    ("fmodl", "rust_sqlite_wasm_fmodl"),
    ("scalbn", "rust_sqlite_wasm_scalbn"),
    ("scalbnl", "rust_sqlite_wasm_scalbnl"),
    // stdlib
    ("atoi", "rust_sqlite_wasm_atoi"),
    ("strtol", "rust_sqlite_wasm_strtol"),
    ("strtod", "rust_sqlite_wasm_strtod"),
    ("bsearch", "rust_sqlite_wasm_bsearch"),
    ("qsort", "rust_sqlite_wasm_qsort"),
    ("__qsort_r", "rust_sqlite_wasm_qsort_r"),
    // errno
    ("__errno_location", "rust_sqlite_wasm_errno_location"),
    // stdio
    ("sprintf", "rust_sqlite_wasm_sprintf"),
    // malloc
    ("malloc", "rust_sqlite_wasm_malloc"),
    ("realloc", "rust_sqlite_wasm_realloc"),
    ("free", "rust_sqlite_wasm_free"),
    ("calloc", "rust_sqlite_wasm_calloc"),
    // time
    ("localtime", "rust_sqlite_wasm_localtime"),
    // misc
    ("getentropy", "rust_sqlite_wasm_getentropy"),
    // exit
    ("abort", "rust_sqlite_wasm_abort"),
    ("__assert_fail", "rust_sqlite_wasm_assert_fail"),
];
