#include <stddef.h>
#include <stdint.h>
#include <time.h>

#define malloc rust_sqlite_wasm_shim_malloc
#define realloc rust_sqlite_wasm_shim_realloc
#define free rust_sqlite_wasm_shim_free
#define calloc rust_sqlite_wasm_shim_calloc
void *rust_sqlite_wasm_shim_malloc(size_t size);
void *rust_sqlite_wasm_shim_realloc(void *ptr, size_t size);
void rust_sqlite_wasm_shim_free(void *ptr);
void *rust_sqlite_wasm_shim_calloc(size_t num, size_t size);

#define strcmp rust_sqlite_wasm_shim_strcmp
#define strcpy rust_sqlite_wasm_shim_strcpy
#define strncpy rust_sqlite_wasm_shim_strncpy
#define strcat rust_sqlite_wasm_shim_strcat
#define strncat rust_sqlite_wasm_shim_strncat
#define strcspn rust_sqlite_wasm_shim_strcspn
#define strspn rust_sqlite_wasm_shim_strspn
#define strncmp rust_sqlite_wasm_shim_strncmp
#define strrchr rust_sqlite_wasm_shim_strrchr
#define strchr rust_sqlite_wasm_shim_strchr
#define memchr rust_sqlite_wasm_shim_memchr
int rust_sqlite_wasm_shim_strcmp(const char *l, const char *r);
char *rust_sqlite_wasm_shim_strcpy(char *dest, const char *src);
char *rust_sqlite_wasm_shim_strncpy(char *d, const char *s, size_t n);
char *rust_sqlite_wasm_shim_strcat(char *dest, const char *src);
char *rust_sqlite_wasm_shim_strncat(char *d, const char *s, size_t n);
size_t rust_sqlite_wasm_shim_strcspn(const char *s, const char *c);
size_t rust_sqlite_wasm_shim_strspn(const char *s, const char *c);
int rust_sqlite_wasm_shim_strncmp(const char *l, const char *r, size_t n);
char *rust_sqlite_wasm_shim_strrchr(const char *s, int c);
char *rust_sqlite_wasm_shim_strchr(const char *s, int c);
void *rust_sqlite_wasm_shim_memchr(const void *src, int c, size_t n);

#define acosh rust_sqlite_wasm_shim_acosh
#define asinh rust_sqlite_wasm_shim_asinh
#define atanh rust_sqlite_wasm_shim_atanh
#define trunc rust_sqlite_wasm_shim_trunc
#define sqrt rust_sqlite_wasm_shim_sqrt
double rust_sqlite_wasm_shim_acosh(double x);
double rust_sqlite_wasm_shim_asinh(double x);
double rust_sqlite_wasm_shim_atanh(double x);
double rust_sqlite_wasm_shim_trunc(double x);
double rust_sqlite_wasm_shim_sqrt(double x);

#define localtime rust_sqlite_wasm_shim_localtime
struct tm *rust_sqlite_wasm_shim_localtime(const time_t *t);

#define abort rust_sqlite_wasm_shim_abort
[[noreturn]] void rust_sqlite_wasm_shim_abort();

#define getentropy rust_sqlite_wasm_shim_getentropy
int rust_sqlite_wasm_shim_getentropy(void *buffer, size_t len);

#define __errno_location rust_sqlite_wasm_shim_errno_location
int *rust_sqlite_wasm_shim_errno_location(void);

#define sprintf sprintf_
int sprintf_(char* buffer, const char* format, ...);

