#include <stddef.h>

#define atoi rust_sqlite_wasm_vec_atoi
int rust_sqlite_wasm_vec_atoi(const char *s);
#define strtol rust_sqlite_wasm_vec_strtol
long rust_sqlite_wasm_vec_strtol(const char *s, char **p, int base);
#define strtod rust_sqlite_wasm_vec_strtod
double rust_sqlite_wasm_vec_strtod(const char *s, char **p);
#define bsearch rust_sqlite_wasm_vec_bsearch
void *rust_sqlite_wasm_vec_bsearch(const void *key, const void *base,
                                   size_t nel, size_t width,
                                   int (*cmp)(const void *, const void *));
#define qsort rust_sqlite_wasm_vec_qsort
void rust_sqlite_wasm_vec_qsort(void *base, size_t nel, size_t width,
           int (*cmp)(const void *, const void *));




