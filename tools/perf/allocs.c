/* Counts heap allocations inside the plugin's `process`, for allocs.py.
 * Preloaded into the playtest: the playtest looks up KX_ALLOC_ON before each
 * `process` call and KX_ALLOC_OFF after it (KONTRA_ALLOC_MARKS set), and
 * between the two every malloc-family call on that thread is counted. The
 * first KX_ALLOC_STACKS (default 400) print their return addresses. */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <execinfo.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static __thread int on, inside;
static unsigned long allocs, frees, shown, stacks = 400;
static void *(*real_malloc)(size_t);
static void *(*real_calloc)(size_t, size_t);
static void *(*real_realloc)(void *, size_t);
static int (*real_memalign)(void **, size_t, size_t);
static void *(*real_aligned)(size_t, size_t);
static void (*real_free)(void *);
static char *(*real_getenv)(const char *);

#define REAL(f, name) (f ? f : (f = dlsym(RTLD_NEXT, name)))

static void hit(void) {
    if (!on || inside) return;
    allocs++;
    if (shown < stacks) {
        void *b[16];
        inside = 1;
        shown++;
        int n = backtrace(b, 16);
        dprintf(2, "ALLOC #%lu\n", allocs);
        backtrace_symbols_fd(b, n, 2);
        inside = 0;
    }
}

void *malloc(size_t s) { hit(); return REAL(real_malloc, "malloc")(s); }
void *calloc(size_t a, size_t b) { hit(); return REAL(real_calloc, "calloc")(a, b); }
void *realloc(void *p, size_t s) { hit(); return REAL(real_realloc, "realloc")(p, s); }
int posix_memalign(void **p, size_t a, size_t s) {
    hit();
    return REAL(real_memalign, "posix_memalign")(p, a, s);
}
void *aligned_alloc(size_t a, size_t s) { hit(); return REAL(real_aligned, "aligned_alloc")(a, s); }
void free(void *p) {
    if (on && !inside && p) frees++;
    REAL(real_free, "free")(p);
}

char *getenv(const char *k) {
    if (!strcmp(k, "KX_ALLOC_ON")) { on = 1; return 0; }
    if (!strcmp(k, "KX_ALLOC_OFF")) { on = 0; return 0; }
    return REAL(real_getenv, "getenv")(k);
}

__attribute__((constructor)) static void start(void) {
    const char *s = REAL(real_getenv, "getenv")("KX_ALLOC_STACKS");
    if (s) stacks = strtoul(s, 0, 10);
}

__attribute__((destructor)) static void done(void) {
    dprintf(2, "ALLOCS %lu %lu\n", allocs, frees);
}
