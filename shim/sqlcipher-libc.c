#include <stdio.h>

/* No files or exit handlers here, so cipher_log, cipher_profile and cipher_migrate fail and atexit is a no-op. */
FILE *const stdout = 0;
FILE *const stderr = 0;
FILE *fopen(const char *restrict path, const char *restrict mode) { (void)path; (void)mode; return 0; }
int fprintf(FILE *restrict f, const char *restrict fmt, ...) { (void)f; (void)fmt; return -1; }
int rename(const char *from, const char *to) { (void)from; (void)to; return -1; }
int atexit(void (*fn)(void)) { (void)fn; return 0; }
