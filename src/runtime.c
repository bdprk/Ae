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

// Owned length-carrying strings (ADR-8): len/slice are O(1); buffers stay
// NUL-terminated for C interop. ae_str_cat consumes both operands;
// moves copy the struct and forfeit the slot; releases are idempotent so
// overlapping release paths can never double-free.
typedef struct { char *ptr; int64_t len; } ae_string;
// Borrowed string view (ADR-8): zero-copy, never released. Views point into
// an owning buffer; the compile-time borrow tracker keeps the base alive.
typedef struct { const char *ptr; int64_t len; } ae_strview;
#define AE_VIEW(s) ((ae_strview){(s).ptr, (s).len})

static ae_string ae_str_from(const char *text, size_t line, size_t column) {
    size_t n = strlen(text);
    char *copy = malloc(n + 1);
    if (!copy) ae_fail("out of memory", line, column);
    memcpy(copy, text, n + 1);
    ae_string s = { copy, (int64_t)n };
    return s;
}
static ae_string ae_str_cat(ae_string a, ae_string b, size_t line, size_t column) {
    if (a.len > INT64_MAX - b.len) { free(a.ptr); free(b.ptr); ae_fail("string too long", line, column); }
    char *out = malloc((size_t)(a.len + b.len) + 1);
    if (!out) { free(a.ptr); free(b.ptr); ae_fail("out of memory", line, column); }
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, b.ptr, (size_t)b.len + 1);
    free(a.ptr); free(b.ptr);
    ae_string s = { out, a.len + b.len };
    return s;
}
static void ae_str_release(ae_string *slot) { free(slot->ptr); slot->ptr = NULL; slot->len = 0; }
static void ae_str_forfeit(ae_string *slot) { slot->ptr = NULL; slot->len = 0; }
static bool ae_str_eq(ae_strview a, ae_strview b) {
    return a.len == b.len && (a.len == 0 || memcmp(a.ptr, b.ptr, (size_t)a.len) == 0);
}
static int64_t ae_str_len(ae_strview s, size_t line, size_t column) {
    (void)line; (void)column;
    return s.len;
}
static ae_strview ae_slice_view(ae_strview s, int64_t start, int64_t end, size_t line, size_t column) {
    if (start < 0 || end < start || end > s.len) ae_fail("string index out of bounds", line, column);
    ae_strview r = { s.ptr + start, end - start };
    return r;
}
static ae_string ae_slice(ae_strview s, int64_t start, int64_t end, size_t line, size_t column) {
    if (start < 0 || end < start || end > s.len) ae_fail("string index out of bounds", line, column);
    char *out = malloc((size_t)(end - start) + 1);
    if (!out) ae_fail("out of memory", line, column);
    memcpy(out, s.ptr + start, (size_t)(end - start));
    out[end - start] = 0;
    ae_string r = { out, end - start };
    return r;
}
static bool ae_contains(ae_strview haystack, ae_strview needle, size_t line, size_t column) {
    // strstr carries the platform's optimized substring search; buffers are
    // NUL-terminated so embedded-NUL truncation matches len/bytes semantics.
    (void)line; (void)column;
    if (needle.len == 0) return true;
    if (haystack.len < needle.len) return false;
    return strstr(haystack.ptr, needle.ptr) != NULL;
}

static ae_string ae_str_from_len(const char *p, int64_t n, size_t line, size_t column) {
    char *copy = malloc((size_t)n + 1);
    if (!copy) ae_fail("out of memory", line, column);
    memcpy(copy, p, (size_t)n);
    copy[n] = 0;
    ae_string out = { copy, n };
    return out;
}
// Concatenation may not consume a borrowed view, so views are copied into the
// new buffer first. The view itself stays valid and untouched.
static ae_string ae_str_copy_view(ae_strview s, size_t line, size_t column) {
    return ae_str_from_len(s.ptr, s.len, line, column);
}

// Owned vector of owned strings. Elements are borrow-only reads.
typedef struct { ae_string *data; int64_t len; int64_t cap; } ae_strvec;
static ae_strvec ae_strvec_new(size_t line, size_t column) {
    ae_strvec v;
    v.len = 0;
    v.cap = 4;
    v.data = malloc((size_t)v.cap * sizeof(ae_string));
    if (!v.data) { v.cap = 0; ae_fail("out of memory", line, column); }
    return v;
}
static void ae_strvec_push(ae_strvec *v, ae_string s, size_t line, size_t column) {
    if (v->len == v->cap) {
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap * 2;
        ae_string *nd = realloc(v->data, (size_t)ncap * sizeof(ae_string));
        if (!nd) ae_fail("out of memory", line, column);
        v->data = nd;
        v->cap = ncap;
    }
    v->data[v->len] = s;
    v->len = v->len + 1;
}
static void ae_strvec_pop(ae_strvec *v, size_t line, size_t column) {
    if (v->len == 0) ae_fail("pop from empty vector", line, column);
    v->len = v->len - 1;
    ae_str_release(&v->data[v->len]);
}
static int64_t ae_strvec_len(ae_strvec v, size_t line, size_t column) {
    (void)line; (void)column;
    return v.len;
}
static int64_t ae_strvec_index(int64_t index, int64_t len, size_t line, size_t column) {
    if (index < 0 || index >= len) ae_fail("array index out of bounds", line, column);
    return index;
}
static ae_string *ae_strvec_get(ae_strvec *v, int64_t index, size_t line, size_t column) {
    ae_strvec_index(index, v->len, line, column);
    return &v->data[index];
}
// Borrowed string vectors (&vec[string]) read elements through this accessor.
static const ae_string *ae_strvec_get_const(const ae_strvec *v, int64_t index, size_t line, size_t column) {
    ae_strvec_index(index, v->len, line, column);
    return &v->data[index];
}
static void ae_strvec_release(ae_strvec *v) {
    for (int64_t i = 0; i < v->len; i++) ae_str_release(&v->data[i]);
    free(v->data);
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}
static void ae_strvec_forfeit(ae_strvec *v) {
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}
// Views only: splitting never takes ownership of its operands, so a borrowed
// `&string` argument works exactly like an owned one.
static ae_strvec ae_split(ae_strview s, ae_strview sep, size_t line, size_t column) {
    if (sep.len == 0) ae_fail("empty separator", line, column);
    ae_strvec out = ae_strvec_new(line, column);
    int64_t start = 0;
    int64_t i = 0;
    while (i + sep.len <= s.len) {
        if (memcmp(s.ptr + i, sep.ptr, (size_t)sep.len) == 0) {
            ae_strvec_push(&out, ae_str_from_len(s.ptr + start, i - start, line, column), line, column);
            i = i + sep.len;
            start = i;
        } else {
            i = i + 1;
        }
    }
    ae_strvec_push(&out, ae_str_from_len(s.ptr + start, s.len - start, line, column), line, column);
    return out;
}

static ae_string ae_join(const ae_strvec *v, ae_strview sep, size_t line, size_t column) {
    if (v->len == 0) {
        char *empty = malloc(1);
        if (!empty) ae_fail("out of memory", line, column);
        empty[0] = 0;
        ae_string out = { empty, 0 };
        return out;
    }
    if (sep.len != 0 && v->len - 1 > INT64_MAX / sep.len) ae_fail("string too long", line, column);
    int64_t total = sep.len * (v->len - 1);
    for (int64_t i = 0; i < v->len; i++) {
        if (total > INT64_MAX - v->data[i].len) ae_fail("string too long", line, column);
        total += v->data[i].len;
    }
    char *out = malloc((size_t)total + 1);
    if (!out) ae_fail("out of memory", line, column);
    int64_t at = 0;
    for (int64_t i = 0; i < v->len; i++) {
        if (i > 0) { memcpy(out + at, sep.ptr, (size_t)sep.len); at += sep.len; }
        memcpy(out + at, v->data[i].ptr, (size_t)v->data[i].len);
        at += v->data[i].len;
    }
    out[at] = 0;
    ae_string result = { out, total };
    return result;
}

// lines(): split on newlines with a word-at-a-time scan (SWAR). Trailing
// newlines do not produce a final empty line, matching common stdlib lines().
#define AE_HASZERO(x) (((x) - 0x0101010101010101ULL) & ~(x) & 0x8080808080808080ULL)
static ae_strvec ae_lines(ae_strview s, size_t line, size_t column) {
    ae_strvec out = ae_strvec_new(line, column);
    int64_t start = 0;
    int64_t i = 0;
    while (i < s.len) {
        while (i + 8 <= s.len) {
            uint64_t chunk;
            memcpy(&chunk, s.ptr + i, 8);
            chunk ^= 0x0a0a0a0a0a0a0a0aULL;
            if (AE_HASZERO(chunk)) { break; }
            i += 8;
        }
        if (i >= s.len) { break; }
        if (s.ptr[i] == 0x0a) {
            ae_strvec_push(&out, ae_str_from_len(s.ptr + start, i - start, line, column), line, column);
            i = i + 1;
            start = i;
        } else {
            i = i + 1;
        }
    }
    if (start < s.len) {
        ae_strvec_push(&out, ae_str_from_len(s.ptr + start, s.len - start, line, column), line, column);
    }
    return out;
}

// count_lines_with(text, pattern): newline-delimited line count where the
// line contains the pattern. Single pass, zero allocations; line search
// uses memchr for first-byte skipping.
static int ae_line_contains(const char *h, int64_t hl, ae_strview pat) {
    if (pat.len == 0) return 1;
    if (pat.len > hl) return 0;
    const char first = pat.ptr[0];
    int64_t i = 0;
    while (i + pat.len <= hl) {
        const char *hit = memchr(h + i, first, (size_t)(hl - i - pat.len + 1));
        if (!hit) return 0;
        i = (int64_t)(hit - h);
        if (memcmp(h + i, pat.ptr, (size_t)pat.len) == 0) return 1;
        i = i + 1;
    }
    return 0;
}
static int64_t ae_count_lines_with(ae_strview s, ae_strview pat, size_t line, size_t column) {
    if (s.len == 0) return 0;
    int64_t count = 0;
    const char *p = s.ptr;
    int64_t remain = s.len;
    while (remain > 0) {
        const char *nl = memchr(p, 0x0a, (size_t)remain);
        int64_t llen = nl ? (int64_t)(nl - p) : remain;
        if (ae_line_contains(p, llen, pat)) count = count + 1;
        if (!nl) break;
        remain -= llen + 1;
        p += llen + 1;
    }
    return count;
}

// Program arguments bridge: the generated main records argc/argv here.
static int64_t ae_argc_g = 0;
static char **ae_argv_g = NULL;
static ae_strvec ae_args(size_t line, size_t column) {
    ae_strvec out = ae_strvec_new(line, column);
    for (int64_t i = 0; i < ae_argc_g; i++) {
        ae_strvec_push(&out, ae_str_from(ae_argv_g[i], line, column), line, column);
    }
    return out;
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
// UTF-8 to UTF-16 for the wide-character Win32 APIs; callers free the result.
static wchar_t *ae_widen(const char *text) {
    const int len = MultiByteToWideChar(CP_UTF8, 0, text, -1, NULL, 0);
    if (len <= 0) return NULL;
    wchar_t *wide = malloc((size_t)len * sizeof(wchar_t));
    if (!wide) return NULL;
    MultiByteToWideChar(CP_UTF8, 0, text, -1, wide, len);
    return wide;
}
// UTF-16 to a fresh UTF-8 buffer of `len` units; returns NULL on failure.
static char *ae_narrow(const wchar_t *text, int len) {
    if (len <= 0) return NULL;
    const int size = WideCharToMultiByte(CP_UTF8, 0, text, len, NULL, 0, NULL, NULL);
    if (size <= 0) return NULL;
    char *narrow = malloc((size_t)size + 1);
    if (!narrow) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, text, len, narrow, size, NULL, NULL);
    narrow[size] = 0;
    return narrow;
}
static FILE *ae_fopen(const char *path, const char *mode) {
    wchar_t *wpath = ae_widen(path);
    wchar_t *wmode = ae_widen(mode);
    if (!wpath || !wmode) { free(wpath); free(wmode); return NULL; }
    FILE *f = _wfopen(wpath, wmode);
    free(wpath);
    free(wmode);
    return f;
}
#else
static FILE *ae_fopen(const char *path, const char *mode) { return fopen(path, mode); }
#endif

static ae_string ae_read_file(const char *path, size_t line, size_t column) {
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
    ae_string out = { buffer, (int64_t)size };
    return out;
}
static int64_t ae_write_file(ae_string path, ae_strview content, size_t line, size_t column) {
    FILE *f = ae_fopen(path.ptr, "wb");
    if (!f) ae_fail("cannot write file", line, column);
    size_t written = fwrite(content.ptr, 1, (size_t)content.len, f);
    if (fclose(f) != 0 || written != (size_t)content.len) ae_fail("cannot write file", line, column);
    return (int64_t)written;
}
static ae_vec ae_bytes(ae_strview s, size_t line, size_t column) {
    ae_vec v = ae_vec_new(line, column);
    for (int64_t i = 0; i < s.len; i++) {
        ae_vec_push(&v, (int64_t)(unsigned char)s.ptr[i], line, column);
    }
    return v;
}

// ---- Hash map: string keys -> i64 values ------------------------------------
// Open addressing with linear probing. Slots are empty (state 0), full (1) or
// deleted (2); a deleted slot keeps probing alive until the next rehash, so
// removing a key never breaks a probe chain. Keys are owned copies, values are
// plain i64, which makes release straightforward and deterministic.
typedef struct { ae_string key; int64_t value; unsigned char state; } ae_map_slot;
typedef struct { ae_map_slot *slots; int64_t cap; int64_t len; int64_t used; } ae_map;

#define AE_MAP_EMPTY 0
#define AE_MAP_FULL 1
#define AE_MAP_DELETED 2

static ae_map ae_map_new(size_t line, size_t column) {
    (void)line; (void)column;
    ae_map m;
    m.slots = NULL;
    m.cap = 0;
    m.len = 0;
    m.used = 0;
    return m;
}
// FNV-1a: fixed and platform independent, so iteration order is reproducible.
static uint64_t ae_map_hash(ae_strview key) {
    uint64_t hash = UINT64_C(1469598103934665603);
    for (int64_t i = 0; i < key.len; i++) {
        hash ^= (uint64_t)(unsigned char)key.ptr[i];
        hash *= UINT64_C(1099511628211);
    }
    return hash;
}
static int64_t ae_map_slot_of(ae_strview key, int64_t cap) {
    return (int64_t)(ae_map_hash(key) & (uint64_t)(cap - 1));
}
// Index of the matching full slot, or -1. Stops only at an empty slot.
static int64_t ae_map_find(const ae_map *m, ae_strview key) {
    if (m->cap == 0) return -1;
    int64_t at = ae_map_slot_of(key, m->cap);
    for (int64_t probes = 0; probes < m->cap; probes++) {
        const ae_map_slot *slot = &m->slots[at];
        if (slot->state == AE_MAP_EMPTY) return -1;
        if (slot->state == AE_MAP_FULL && ae_str_eq(AE_VIEW(slot->key), key)) return at;
        at = (at + 1) & (m->cap - 1);
    }
    return -1;
}
static void ae_map_grow(ae_map *m, size_t line, size_t column) {
    int64_t cap = m->cap == 0 ? 8 : m->cap;
    // Rehash into a table twice as large when live entries fill three quarters,
    // which also drops every tombstone.
    while ((m->len + 1) * 4 >= cap * 3) {
        if (cap > INT64_MAX / 4) ae_fail("map too large", line, column);
        cap = cap * 2;
    }
    if (cap == m->cap && m->used == m->len) return;
    // malloc + memset rather than calloc: the allocation-counting checker in
    // scripts/leakcheck.c wraps malloc/free/realloc only, and an unwrapped
    // calloc buffer freed through the wrapper would corrupt the heap.
    ae_map_slot *slots = malloc((size_t)cap * sizeof(ae_map_slot));
    if (!slots) ae_fail("out of memory", line, column);
    memset(slots, 0, (size_t)cap * sizeof(ae_map_slot));
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state != AE_MAP_FULL) continue;
        int64_t at = ae_map_slot_of(AE_VIEW(m->slots[i].key), cap);
        while (slots[at].state == AE_MAP_FULL) at = (at + 1) & (cap - 1);
        slots[at] = m->slots[i];
    }
    free(m->slots);
    m->slots = slots;
    m->cap = cap;
    m->used = m->len;
}
// Inserts or overwrites. The key is copied; the caller keeps its own string.
static void ae_map_put(ae_map *m, ae_strview key, int64_t value, size_t line, size_t column) {
    ae_map_grow(m, line, column);
    int64_t at = ae_map_slot_of(key, m->cap);
    int64_t reuse = -1;
    for (;;) {
        ae_map_slot *slot = &m->slots[at];
        if (slot->state == AE_MAP_EMPTY) break;
        if (slot->state == AE_MAP_DELETED) {
            if (reuse < 0) reuse = at;
        } else if (ae_str_eq(AE_VIEW(slot->key), key)) {
            slot->value = value;
            return;
        }
        at = (at + 1) & (m->cap - 1);
    }
    if (reuse >= 0) at = reuse;
    else m->used = m->used + 1;
    ae_map_slot *slot = &m->slots[at];
    slot->key = ae_str_from_len(key.ptr, key.len, line, column);
    slot->value = value;
    slot->state = AE_MAP_FULL;
    m->len = m->len + 1;
}
static bool ae_map_lookup(const ae_map *m, ae_strview key, int64_t *out) {
    const int64_t at = ae_map_find(m, key);
    if (at < 0) return false;
    if (out) *out = m->slots[at].value;
    return true;
}
static bool ae_map_has(const ae_map *m, ae_strview key, size_t line, size_t column) {
    (void)line; (void)column;
    return ae_map_find(m, key) >= 0;
}
static bool ae_map_remove(ae_map *m, ae_strview key, size_t line, size_t column) {
    (void)line; (void)column;
    const int64_t at = ae_map_find(m, key);
    if (at < 0) return false;
    ae_str_release(&m->slots[at].key);
    m->slots[at].value = 0;
    m->slots[at].state = AE_MAP_DELETED;
    m->len = m->len - 1;
    return true;
}
static int64_t ae_map_len(const ae_map *m, size_t line, size_t column) {
    (void)line; (void)column;
    return m->len;
}
static void ae_map_release(ae_map *m) {
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state == AE_MAP_FULL) ae_str_release(&m->slots[i].key);
    }
    free(m->slots);
    m->slots = NULL;
    m->cap = 0;
    m->len = 0;
    m->used = 0;
}
static void ae_map_forfeit(ae_map *m) {
    m->slots = NULL;
    m->cap = 0;
    m->len = 0;
    m->used = 0;
}
