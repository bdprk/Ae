// Allocator accounting used by scripts/verify_generated_c.py on platforms
// without AddressSanitizer (MinGW-W64 ships no libasan). GNU ld's --wrap
// redirects every malloc/free/realloc in the generated program and the
// runtime through these counters, so a forgotten release shows up as a
// non-zero block count at exit.
#include <stdio.h>
#include <stdlib.h>

static long long ae_leak_blocks = 0;
static long long ae_leak_bytes = 0;

void *__real_malloc(size_t);
void __real_free(void *);
void *__real_realloc(void *, size_t);

static void ae_leak_report(void) {
    // Stderr keeps the program's stdout byte-comparable in verification runs.
    fprintf(stderr, "[leak] blocks=%lld bytes=%lld\n", ae_leak_blocks, ae_leak_bytes);
}

__attribute__((constructor)) static void ae_leak_init(void) { atexit(ae_leak_report); }

void *__wrap_malloc(size_t size) {
    size_t *header = (size_t *)__real_malloc(size + 16);
    if (!header) return NULL;
    *header = size;
    ae_leak_blocks += 1;
    ae_leak_bytes += (long long)size;
    return (char *)header + 16;
}

void __wrap_free(void *block) {
    if (!block) return;
    size_t *header = (size_t *)((char *)block - 16);
    ae_leak_blocks -= 1;
    ae_leak_bytes -= (long long)*header;
    __real_free(header);
}

void *__wrap_realloc(void *block, size_t size) {
    if (!block) return __wrap_malloc(size);
    size_t *header = (size_t *)((char *)block - 16);
    size_t previous = *header;
    size_t *grown = (size_t *)__real_realloc(header, size + 16);
    if (!grown) return NULL;
    *grown = size;
    ae_leak_bytes += (long long)size - (long long)previous;
    return (char *)grown + 16;
}
