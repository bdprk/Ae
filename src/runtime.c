#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
_Noreturn static void ae_fail(const char *message, size_t line, size_t column) { fprintf(stderr, "ae:%zu:%zu: %s\n", line, column, message); exit(1); }
static int64_t ae_add(int64_t a, int64_t b, size_t line, size_t column) {
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) ae_fail("integer overflow", line, column);
    return a + b;
}
static int64_t ae_sub(int64_t a, int64_t b, size_t line, size_t column) {
    if ((b < 0 && a > INT64_MAX + b) || (b > 0 && a < INT64_MIN + b)) ae_fail("integer overflow", line, column);
    return a - b;
}
static int64_t ae_mul(int64_t a, int64_t b, size_t line, size_t column) {
    if (a > 0 ? (b > 0 ? a > INT64_MAX / b : b < INT64_MIN / a)
              : (a < 0 ? (b > 0 ? a < INT64_MIN / b : b < 0 && a < INT64_MAX / b) : 0))
        ae_fail("integer overflow", line, column);
    return a * b;
}
static int64_t ae_div(int64_t a, int64_t b, size_t line, size_t column) {
    if (b == 0) ae_fail("division by zero", line, column);
    if (a == INT64_MIN && b == -1) ae_fail("integer overflow", line, column);
    return a / b;
}
static int64_t ae_mod(int64_t a, int64_t b, size_t line, size_t column) {
    if (b == 0) ae_fail("division by zero", line, column);
    if (a == INT64_MIN && b == -1) ae_fail("integer overflow", line, column);
    return a % b;
}
static int64_t ae_neg(int64_t a, size_t line, size_t column) {
    if (a == INT64_MIN) ae_fail("integer overflow", line, column);
    return -a;
}

static size_t ae_index(int64_t index, size_t len, size_t line, size_t column) {
    if (index < 0 || (uint64_t)index >= len) ae_fail("array index out of bounds", line, column);
    return (size_t)index;
}

// Owned strings. ae_str_cat consumes both operands; ae_str_release is
// idempotent so overlapping release paths can never double-free.
static char *ae_str_from(const char *text, size_t line, size_t column) {
    size_t n = strlen(text) + 1;
    char *copy = malloc(n);
    if (!copy) ae_fail("out of memory", line, column);
    memcpy(copy, text, n);
    return copy;
}
static char *ae_str_cat(char *a, char *b, size_t line, size_t column) {
    size_t na = strlen(a), nb = strlen(b);
    char *out = malloc(na + nb + 1);
    if (!out) { free(a); free(b); ae_fail("out of memory", line, column); }
    memcpy(out, a, na);
    memcpy(out + na, b, nb + 1);
    free(a); free(b);
    return out;
}
static void ae_str_release(char **slot) { free(*slot); *slot = NULL; }
static bool ae_str_eq(const char *a, const char *b) { return strcmp(a, b) == 0; }
static int64_t ae_str_len(const char *s, size_t line, size_t column) {
    size_t n = strlen(s);
    if (n > (size_t)INT64_MAX) ae_fail("string too long", line, column);
    return (int64_t)n;
}

// Owned growable i64 vector. Moves copy the struct and forfeit the source
// slot (zeroed without freeing); releases are idempotent, so overlapping
// release paths can never double-free.
typedef struct { int64_t *data; int64_t len; int64_t cap; } ae_vec;
static ae_vec ae_vec_new(size_t line, size_t column) {
    ae_vec v;
    v.len = 0;
    v.cap = 4;
    v.data = malloc((size_t)v.cap * sizeof(int64_t));
    if (!v.data) { v.cap = 0; ae_fail("out of memory", line, column); }
    return v;
}
static void ae_vec_push(ae_vec *v, int64_t x, size_t line, size_t column) {
    if (v->len == v->cap) {
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap * 2;
        int64_t *nd = realloc(v->data, (size_t)ncap * sizeof(int64_t));
        if (!nd) ae_fail("out of memory", line, column);
        v->data = nd;
        v->cap = ncap;
    }
    v->data[v->len] = x;
    v->len = v->len + 1;
}
static void ae_vec_pop(ae_vec *v, size_t line, size_t column) {
    if (v->len == 0) ae_fail("pop from empty vector", line, column);
    v->len = v->len - 1;
}
static int64_t ae_vec_len(ae_vec v, size_t line, size_t column) {
    (void)line; (void)column;
    return v.len;
}
static int64_t ae_vec_index(int64_t index, int64_t len, size_t line, size_t column) {
    if (index < 0 || index >= len) ae_fail("array index out of bounds", line, column);
    return index;
}
static int64_t ae_vec_get(ae_vec v, int64_t index, size_t line, size_t column) {
    ae_vec_index(index, v.len, line, column);
    return v.data[index];
}
static void ae_vec_release(ae_vec *v) {
    free(v->data);
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}
// Zero a slot whose contents were moved elsewhere: no free.
static void ae_vec_forfeit(ae_vec *v) {
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}

// File I/O. Paths are UTF-8; on Windows they are widened for _wfopen so
// non-ASCII paths behave like the rest of the platform layer.
#ifdef _WIN32
#include <windows.h>
static FILE *ae_fopen(const char *path, const char *mode) {
    int wlen = MultiByteToWideChar(CP_UTF8, 0, path, -1, NULL, 0);
    int mlen = MultiByteToWideChar(CP_UTF8, 0, mode, -1, NULL, 0);
    if (wlen <= 0 || mlen <= 0) return NULL;
    wchar_t *wpath = malloc((size_t)wlen * sizeof(wchar_t));
    wchar_t *wmode = malloc((size_t)mlen * sizeof(wchar_t));
    if (!wpath || !wmode) { free(wpath); free(wmode); return NULL; }
    MultiByteToWideChar(CP_UTF8, 0, path, -1, wpath, wlen);
    MultiByteToWideChar(CP_UTF8, 0, mode, -1, wmode, mlen);
    FILE *f = _wfopen(wpath, wmode);
    free(wpath);
    free(wmode);
    return f;
}
#else
static FILE *ae_fopen(const char *path, const char *mode) { return fopen(path, mode); }
#endif

static char *ae_read_file(const char *path, size_t line, size_t column) {
    FILE *f = ae_fopen(path, "rb");
    if (!f) ae_fail("cannot read file", line, column);
    if (fseek(f, 0, SEEK_END) != 0) { fclose(f); ae_fail("cannot read file", line, column); }
    long size = ftell(f);
    if (size < 0) { fclose(f); ae_fail("cannot read file", line, column); }
    rewind(f);
    char *buffer = malloc((size_t)size + 1);
    if (!buffer) { fclose(f); ae_fail("out of memory", line, column); }
    size_t read = fread(buffer, 1, (size_t)size, f);
    fclose(f);
    if (read != (size_t)size) ae_fail("cannot read file", line, column);
    buffer[size] = 0;
    return buffer;
}
static int64_t ae_write_file(const char *path, const char *content, size_t line, size_t column) {
    FILE *f = ae_fopen(path, "wb");
    if (!f) ae_fail("cannot write file", line, column);
    size_t n = strlen(content);
    size_t written = fwrite(content, 1, n, f);
    if (fclose(f) != 0 || written != n) ae_fail("cannot write file", line, column);
    return (int64_t)written;
}
static ae_vec ae_bytes(const char *s, size_t line, size_t column) {
    ae_vec v = ae_vec_new(line, column);
    for (const unsigned char *p = (const unsigned char *)s; *p; p++) {
        ae_vec_push(&v, (int64_t)*p, line, column);
    }
    return v;
}
