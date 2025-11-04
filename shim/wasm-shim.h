#include <stddef.h>
#include <stdint.h>
#include <time.h>

#define malloc rust_sqlite_wasm_rs_malloc
#define realloc rust_sqlite_wasm_rs_realloc
#define free rust_sqlite_wasm_rs_free
#define calloc rust_sqlite_wasm_rs_calloc
void *rust_sqlite_wasm_rs_malloc(size_t size);
void *rust_sqlite_wasm_rs_realloc(void *ptr, size_t size);
void rust_sqlite_wasm_rs_free(void *ptr);
void *rust_sqlite_wasm_rs_calloc(size_t num, size_t size);

#define strcmp rust_sqlite_wasm_rs_strcmp
#define strcpy rust_sqlite_wasm_rs_strcpy
#define strncpy rust_sqlite_wasm_rs_strncpy
#define strcat rust_sqlite_wasm_rs_strcat
#define strncat rust_sqlite_wasm_rs_strncat
#define strcspn rust_sqlite_wasm_rs_strcspn
#define strspn rust_sqlite_wasm_rs_strspn
#define strncmp rust_sqlite_wasm_rs_strncmp
#define strrchr rust_sqlite_wasm_rs_strrchr
#define strchr rust_sqlite_wasm_rs_strchr
#define memchr rust_sqlite_wasm_rs_memchr
int rust_sqlite_wasm_rs_strcmp(const char *l, const char *r);
char *rust_sqlite_wasm_rs_strcpy(char *dest, const char *src);
char *rust_sqlite_wasm_rs_strncpy(char *d, const char *s, size_t n);
char *rust_sqlite_wasm_rs_strcat(char *dest, const char *src);
char *rust_sqlite_wasm_rs_strncat(char *d, const char *s, size_t n);
size_t rust_sqlite_wasm_rs_strcspn(const char *s, const char *c);
size_t rust_sqlite_wasm_rs_strspn(const char *s, const char *c);
int rust_sqlite_wasm_rs_strncmp(const char *l, const char *r, size_t n);
char *rust_sqlite_wasm_rs_strrchr(const char *s, int c);
char *rust_sqlite_wasm_rs_strchr(const char *s, int c);
void *rust_sqlite_wasm_rs_memchr(const void *src, int c, size_t n);

#define acosh rust_sqlite_wasm_rs_acosh
#define asinh rust_sqlite_wasm_rs_asinh
#define atanh rust_sqlite_wasm_rs_atanh
#define trunc rust_sqlite_wasm_rs_trunc
#define sqrt rust_sqlite_wasm_rs_sqrt
double rust_sqlite_wasm_rs_acosh(double x);
double rust_sqlite_wasm_rs_asinh(double x);
double rust_sqlite_wasm_rs_atanh(double x);
double rust_sqlite_wasm_rs_trunc(double x);
double rust_sqlite_wasm_rs_sqrt(double x);

#define localtime rust_sqlite_wasm_rs_localtime
struct tm *rust_sqlite_wasm_rs_localtime(const time_t *t);

#define abort rust_sqlite_wasm_rs_abort
[[noreturn]] void rust_sqlite_wasm_rs_abort();

#define getentropy rust_sqlite_wasm_rs_getentropy
int rust_sqlite_wasm_rs_getentropy(void *buffer, size_t len);

#define __errno_location rust_sqlite_wasm_rs_errno_location
int *rust_sqlite_wasm_rs_errno_location(void);

#define sprintf sprintf_
int sprintf_(char *buffer, const char *format, ...);
