/* string */
#define strcmp rust_sqlite_wasm_strcmp
#define strcpy rust_sqlite_wasm_strcpy
#define strncpy rust_sqlite_wasm_strncpy
#define strcat rust_sqlite_wasm_strcat
#define strncat rust_sqlite_wasm_strncat
#define strcspn rust_sqlite_wasm_strcspn
#define strspn rust_sqlite_wasm_strspn
#define strncmp rust_sqlite_wasm_strncmp
#define strrchr rust_sqlite_wasm_strrchr
#define strchr rust_sqlite_wasm_strchr
#define memchr rust_sqlite_wasm_memchr
#define strlen rust_sqlite_wasm_strlen
#define __memrchr rust_sqlite_wasm_memrchr
#define __stpcpy rust_sqlite_wasm_stpcpy
#define __stpncpy rust_sqlite_wasm_stpncpy
#define __strchrnul rust_sqlite_wasm_strchrnul

/* math */
#define __fpclassifyl rust_sqlite_wasm_fpclassifyl
#define acosh rust_sqlite_wasm_acosh
#define asinh rust_sqlite_wasm_asinh
#define atanh rust_sqlite_wasm_atanh
#define trunc rust_sqlite_wasm_trunc
#define sqrt rust_sqlite_wasm_sqrt
#define fmodl rust_sqlite_wasm_fmodl
#define scalbn rust_sqlite_wasm_scalbn
#define scalbnl rust_sqlite_wasm_scalbnl

/* stdlib */
#define atoi rust_sqlite_wasm_atoi
#define strtol rust_sqlite_wasm_strtol
#define strtod rust_sqlite_wasm_strtod
#define bsearch rust_sqlite_wasm_bsearch
#define qsort rust_sqlite_wasm_qsort
#define __qsort_r rust_sqlite_wasm_qsort_r

/* errno */
#define __errno_location rust_sqlite_wasm_errno_location

/* malloc */
#define malloc rust_sqlite_wasm_malloc
#define realloc rust_sqlite_wasm_realloc
#define free rust_sqlite_wasm_free
#define calloc rust_sqlite_wasm_calloc

/* time */
#define localtime rust_sqlite_wasm_localtime

/* misc */
#define getentropy rust_sqlite_wasm_getentropy

/* exit */
#define abort rust_sqlite_wasm_abort
#define __assert_fail rust_sqlite_wasm_assert_fail

/* stdio */
#define stdout rust_sqlite_wasm_stdout
#define stderr rust_sqlite_wasm_stderr

#define fopen rust_sqlite_wasm_fopen
#define fprintf rust_sqlite_wasm_fprintf
#define sprintf rust_sqlite_wasm_sprintf
#define rename rust_sqlite_wasm_rename
#define atexit rust_sqlite_wasm_atexit

#include <stdio.h>

#undef stdout
#undef stderr

#define stdout rust_sqlite_wasm_stdout
#define stderr rust_sqlite_wasm_stderr
