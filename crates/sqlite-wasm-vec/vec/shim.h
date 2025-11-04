#include <stddef.h>

#define strncmp rust_sqlite_wasm_vec_strncmp
int rust_sqlite_wasm_shim_strncmp(const char *l, const char *r, size_t n);

#define atoi rust_sqlite_wasm_vec_atoi
int rust_sqlite_wasm_vec_atoi(const char *s);

#define strtol rust_sqlite_wasm_vec_strtol
long rust_sqlite_wasm_vec_strtol(const char *s, char **p, int base);

#define strtod rust_sqlite_wasm_vec_strtod
double rust_sqlite_wasm_vec_strtod(const char *s, char **p);

#define __errno_location rust_sqlite_wasm_vec_errno_location
int *rust_sqlite_wasm_vec_errno_location(void);

#define __assert_fail rust_sqlite_wasm_vec_assert_fail
[[noreturn]] void rust_sqlite_wasm_vec_assert_fail(const char *expr,
                                                   const char *file, int line,
                                                   const char *func);

#define bsearch rust_sqlite_wasm_vec_bsearch
void *rust_sqlite_wasm_vec_bsearch(const void *key, const void *base,
                                   size_t nel, size_t width,
                                   int (*cmp)(const void *, const void *));

#define qsort rust_sqlite_wasm_vec_qsort
void rust_sqlite_wasm_vec_qsort(void *base, size_t nel, size_t width,
           int (*cmp)(const void *, const void *));

#define __fpclassifyl rust_sqlite_wasm_vec_fpclassifyl
int rust_sqlite_wasm_vec_fpclassifyl(long double x);
