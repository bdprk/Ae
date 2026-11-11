#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
_Noreturn void ae_fail(const char *message, size_t line, size_t column){ fprintf(stderr, "ae:%zu:%zu: %s\n", line, column, message); exit(1); }

// ---- Allocation interface (ADR-16) ------------------------------------------
// Every buffer the runtime owns is obtained through one of these three
// pointers, so a host (future embed API, a test harness, a tracing allocator)
// can substitute its own backend without touching generated code. Failure is
// fatal and carries the location of the operation that needed the memory:
// there is no meaningful way to continue with a half-built value, and no
// unwinding machinery to run destructors through.
typedef struct {
    void *(*alloc)(size_t size);
    void *(*resize)(void *block, size_t size);
    void (*release)(void *block);
} ae_allocator;
void *ae_os_alloc(size_t size){ return malloc(size); }
void *ae_os_resize(void *block, size_t size){ return realloc(block, size); }
void ae_os_release(void *block){ free(block); }
static ae_allocator ae_allocator_impl = { ae_os_alloc, ae_os_resize, ae_os_release };
// `AE_ALLOC_LIMIT` caps the cumulative number of bytes handed out, which is
// how the test suite reaches the out-of-memory path on purpose. It is a
// diagnostic, not a policy knob: the default is unlimited.
static uint64_t ae_alloc_budget = UINT64_MAX;
static uint64_t ae_alloc_total = 0;
static bool ae_alloc_ready = false;
void ae_alloc_env(void){
    ae_alloc_ready = true;
    const char *limit = getenv("AE_ALLOC_LIMIT");
    if (!limit || !*limit) return;
    char *end = NULL;
    unsigned long long value = strtoull(limit, &end, 10);
    if (end == limit || *end) return;
    ae_alloc_budget = (uint64_t)value;
}
// NULL-returning variant for helpers that report failure through their own
// return value (path widening, for instance) instead of terminating.
void *ae_alloc_try(size_t size){
    if (!ae_alloc_ready) ae_alloc_env();
    if (size > ae_alloc_budget - ae_alloc_total) return NULL;
    void *block = ae_allocator_impl.alloc(size);
    if (!block) return NULL;
    ae_alloc_total += (uint64_t)size;
    return block;
}
void *ae_alloc(size_t size, size_t line, size_t column){
    void *block = ae_alloc_try(size);
    if (!block) ae_fail("out of memory", line, column);
    return block;
}
void *ae_alloc_resize(void *block, size_t size, size_t line, size_t column){
    if (!ae_alloc_ready) ae_alloc_env();
    // The budget is charged for the whole new size: the accounting is a coarse
    // "how much did this program ask for" measure, not a live byte count.
    if (size > ae_alloc_budget - ae_alloc_total) { ae_allocator_impl.release(block); ae_fail("out of memory", line, column); }
    void *resized = ae_allocator_impl.resize(block, size);
    if (!resized) ae_fail("out of memory", line, column);
    ae_alloc_total += (uint64_t)size;
    return resized;
}
void ae_free(void *block){ if (block) ae_allocator_impl.release(block); }
void ae_set_allocator(ae_allocator impl){ ae_allocator_impl = impl; }

int64_t ae_add(int64_t a, int64_t b, size_t line, size_t column){
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) ae_fail("integer overflow", line, column);
    return a + b;
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

ae_string ae_str_from(const char *text, size_t line, size_t column){
    size_t n = strlen(text);
    char *copy = ae_alloc(n + 1, line, column);
    memcpy(copy, text, n + 1);
    ae_string s = { copy, (int64_t)n };
    return s;
}
ae_string ae_str_cat(ae_string a, ae_string b, size_t line, size_t column){
    if (a.len > INT64_MAX - b.len) { ae_free(a.ptr); ae_free(b.ptr); ae_fail("string too long", line, column); }
    char *out = ae_alloc((size_t)(a.len + b.len) + 1, line, column);
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, b.ptr, (size_t)b.len + 1);
    ae_free(a.ptr); ae_free(b.ptr);
    ae_string s = { out, a.len + b.len };
    return s;
}
void ae_str_release(ae_string *slot){ ae_free(slot->ptr); slot->ptr = NULL; slot->len = 0; }
void ae_str_forfeit(ae_string *slot){ slot->ptr = NULL; slot->len = 0; }
int64_t ae_str_len(ae_strview s, size_t line, size_t column){
    (void)line; (void)column;
    return s.len;
}

ae_string ae_str_from_len(const char *p, int64_t n, size_t line, size_t column){
    char *copy = ae_alloc((size_t)n + 1, line, column);
    memcpy(copy, p, (size_t)n);
    copy[n] = 0;
    ae_string out = { copy, n };
    return out;
}
// Concatenation may not consume a borrowed view, so views are copied into the
// new buffer first. The view itself stays valid and untouched.
ae_string ae_str_copy_view(ae_strview s, size_t line, size_t column){
    return ae_str_from_len(s.ptr, s.len, line, column);
}

// Owned vector of owned strings. Elements are borrow-only reads.
//
// `arena` is NULL for vectors built element by element (`push`, `args`, `keys`):
// every element owns its own buffer and is released individually. `split` and
// `lines` instead copy all tokens into one arena and point every element into
// it, so N tokens cost two allocations instead of N (ADR-49). Elements whose
// pointer lies inside the arena are cleared without freeing; the arena is
// released with the vector.
typedef struct { ae_string *data; int64_t len; int64_t cap; char *arena; int64_t arena_len; } ae_strvec;
int ae_strvec_borrowed(const ae_strvec *v, const ae_string *s){
    // Compare as integers: two pointers into unrelated objects have no defined
    // ordering in ISO C, but their uintptr_t values are comparable here.
    return v->arena != NULL
        && s->ptr != NULL
        && (uintptr_t)s->ptr >= (uintptr_t)v->arena
        && (uintptr_t)s->ptr < (uintptr_t)v->arena + (uintptr_t)v->arena_len;
}
ae_strvec ae_strvec_new(size_t line, size_t column){
    ae_strvec v;
    v.len = 0;
    v.cap = 4;
    v.data = ae_alloc((size_t)v.cap * sizeof(ae_string), line, column);
    v.arena = NULL;
    v.arena_len = 0;
    return v;
}
void ae_strvec_push(ae_strvec *v, ae_string s, size_t line, size_t column){
    if (v->len == v->cap) {
        // A fused `lines("")` vector starts at capacity 0, so growth must have a
        // floor; doubling from zero would ask for zero bytes and then write
        // past it.
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap < 4 ? 4 : v->cap * 2;
        ae_string *nd = ae_alloc_resize(v->data, (size_t)ncap * sizeof(ae_string), line, column);
        v->data = nd;
        v->cap = ncap;
    }
    v->data[v->len] = s;
    v->len = v->len + 1;
}
void ae_strvec_release(ae_strvec *v){
    for (int64_t i = 0; i < v->len; i++) {
        if (ae_strvec_borrowed(v, &v->data[i])) {
            v->data[i].ptr = NULL;
            v->data[i].len = 0;
        } else {
            ae_str_release(&v->data[i]);
        }
    }
    ae_free(v->data);
    ae_free(v->arena);
    v->data = NULL;
    v->arena = NULL;
    v->arena_len = 0;
    v->len = 0;
    v->cap = 0;
}

ae_string ae_join(const ae_strvec *v, ae_strview sep, size_t line, size_t column){
    if (v->len == 0) {
        char *empty = ae_alloc(1, line, column);
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
    char *out = ae_alloc((size_t)total + 1, line, column);
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

#define AE_TOKENS_SEP_MAX 16
typedef struct { ae_strview text; char sep[AE_TOKENS_SEP_MAX]; int64_t sep_len; int64_t pos; int64_t done; } ae_tokens;
// Lazy token cursor (ADR-53): the same token stream as `split`, but produced
// one token at a time. It holds a view of the base string and an inline copy of
// the separator, and allocates nothing, so iterating a 2.6 MB file costs no
// per-token memory. The token boundaries match `split` exactly, including the
// trailing empty token after a final separator and the single empty token for
// empty input. The separator is copied into the cursor rather than pointed at:
// the argument is usually a temporary that is released at the end of the
// statement, and the cursor must stay valid for the whole loop. Sixteen bytes
// cover every realistic delimiter, so the cursor owns no memory and a longer
// one is rejected instead of allocated.
ae_tokens ae_tokens_new(ae_strview text, ae_strview sep, size_t line, size_t column){
    if (sep.len == 0) ae_fail("empty separator", line, column);
    if (sep.len > AE_TOKENS_SEP_MAX) ae_fail("token separator longer than 16 bytes", line, column);
    ae_tokens cursor;
    cursor.text = text;
    memcpy(cursor.sep, sep.ptr, (size_t)sep.len);
    cursor.sep_len = sep.len;
    cursor.pos = 0;
    cursor.done = 0;
    return cursor;
}
// Rewinds to the first token, so a second loop over the same binding sees the
// same stream instead of an empty one.
void ae_tokens_reset(ae_tokens *cursor){ cursor->pos = 0; cursor->done = 0; }
// Returns false once the stream is exhausted; `out` points into the base text.
bool ae_tokens_next(ae_tokens *cursor, ae_strview *out){
    if (cursor->done) return false;
    int64_t start = cursor->pos;
    int64_t at = -1;
    if (cursor->sep_len == 1) {
        const char *hit = memchr(cursor->text.ptr + start, cursor->sep[0], (size_t)(cursor->text.len - start));
        if (hit) at = (int64_t)(hit - cursor->text.ptr);
    } else {
        int64_t i = start;
        while (i + cursor->sep_len <= cursor->text.len) {
            if (memcmp(cursor->text.ptr + i, cursor->sep, (size_t)cursor->sep_len) == 0) { at = i; break; }
            i += 1;
        }
    }
    if (at < 0) {
        out->ptr = cursor->text.ptr + start;
        out->len = cursor->text.len - start;
        cursor->pos = cursor->text.len;
        cursor->done = 1;
    } else {
        out->ptr = cursor->text.ptr + start;
        out->len = at - start;
        cursor->pos = at + cursor->sep_len;
    }
    return true;
}

// Program arguments bridge: the generated main records argc/argv here.
static int64_t ae_argc_g = 0;
static char **ae_argv_g = NULL;

// Owned growable i64 vector. Moves copy the struct and forfeit the source
// slot (zeroed without freeing); releases are idempotent, so overlapping
// release paths can never double-free.
typedef struct { int64_t *data; int64_t len; int64_t cap; } ae_vec;

// File I/O. Paths are UTF-8; on Windows they are widened for _wfopen so
// non-ASCII paths behave like the rest of the platform layer.
#ifdef _WIN32
// The full windows.h pulls in winsock, RPC, GDI and shell declarations that
// this runtime never uses. Measured on WinLibs GCC 16.2: parsing the plain
// header costs ~690 ms, with this define ~520 ms. The runtime pruner drops
// this include entirely when no retained function needs a Win32 declaration,
// so pure-computation programs do not pay for it at build time.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
/* windows.h omitted */
#else
#endif

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

typedef enum {
    AE_E0_0 = 0,
    AE_E0_1 = 1
} ae_e0_tag;
typedef struct {
    ae_string p0;
} ae_e0_v0;
typedef struct {
    ae_string p0;
} ae_e0_v1;
typedef struct {
    ae_e0_tag tag;
    union {
        ae_e0_v0 v0;
        ae_e0_v1 v1;
    } u;
} ae_e0;

typedef enum {
    AE_E1_0 = 0,
    AE_E1_1 = 1
} ae_e1_tag;
typedef struct {
    int64_t p0;
} ae_e1_v0;
typedef struct {
    ae_string p0;
} ae_e1_v1;
typedef struct {
    ae_e1_tag tag;
    union {
        ae_e1_v0 v0;
        ae_e1_v1 v1;
    } u;
} ae_e1;

typedef enum {
    AE_E2_0 = 0,
    AE_E2_1 = 1
} ae_e2_tag;
typedef struct {
    int64_t p0;
} ae_e2_v0;
typedef struct {
    ae_string p0;
} ae_e2_v1;
typedef struct {
    ae_e2_tag tag;
    union {
        ae_e2_v0 v0;
        ae_e2_v1 v1;
    } u;
} ae_e2;

typedef enum {
    AE_E3_0 = 0,
    AE_E3_1 = 1
} ae_e3_tag;
typedef struct {
    ae_strvec p0;
} ae_e3_v0;
typedef struct {
    ae_string p0;
} ae_e3_v1;
typedef struct {
    ae_e3_tag tag;
    union {
        ae_e3_v0 v0;
        ae_e3_v1 v1;
    } u;
} ae_e3;

typedef enum {
    AE_E4_0 = 0,
    AE_E4_1 = 1
} ae_e4_tag;
typedef struct {
    int64_t p0;
    int64_t p1;
    bool p2;
} ae_e4_v0;
typedef struct {
    ae_string p0;
} ae_e4_v1;
typedef struct {
    ae_e4_tag tag;
    union {
        ae_e4_v0 v0;
        ae_e4_v1 v1;
    } u;
} ae_e4;

typedef enum {
    AE_E5_0 = 0,
    AE_E5_1 = 1
} ae_e5_tag;
typedef struct {
    ae_string p0;
} ae_e5_v1;
typedef struct {
    ae_e5_tag tag;
    union {
        ae_e5_v1 v1;
    } u;
} ae_e5;

typedef enum {
    AE_E6_0 = 0,
    AE_E6_1 = 1
} ae_e6_tag;
typedef struct {
    bool p0;
} ae_e6_v0;
typedef struct {
    ae_string p0;
} ae_e6_v1;
typedef struct {
    ae_e6_tag tag;
    union {
        ae_e6_v0 v0;
        ae_e6_v1 v1;
    } u;
} ae_e6;

typedef enum {
    AE_E7_0 = 0,
    AE_E7_1 = 1
} ae_e7_tag;
typedef struct {
    int64_t p0;
    ae_string p1;
} ae_e7_v0;
typedef struct {
    ae_string p0;
} ae_e7_v1;
typedef struct {
    ae_e7_tag tag;
    union {
        ae_e7_v0 v0;
        ae_e7_v1 v1;
    } u;
} ae_e7;
// Fallible built-ins and platform helpers that name prelude enum types.
// Included after the enum layouts, so ae_e0..ae_e3 exist here.
// Invariant (checked by the emitter): the prelude registers exactly
//   0 ReadResult{Ok(string), Err(string)}
//   1 WriteResult{Ok(i64), Err(string)}
//   2 IntResult{Ok(i64), Err(string)}
//   3 DirResult{Ok(vec[string]), Err(string)}
// with Ok at variant index 0 and Err at index 1.

#include <errno.h>
#include <math.h>
#include <time.h>
#ifndef _WIN32
#include <dirent.h>
#include <fcntl.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>
#else
#include <io.h>
#include <fcntl.h>
#endif

#define AE_READ_RESULT ae_e0
#define AE_READ_OK AE_E0_0
#define AE_READ_ERR AE_E0_1
#define AE_WRITE_RESULT ae_e1
#define AE_WRITE_OK AE_E1_0
#define AE_WRITE_ERR AE_E1_1
#define AE_INT_RESULT ae_e2
#define AE_INT_OK AE_E2_0
#define AE_INT_ERR AE_E2_1
#define AE_DIR_RESULT ae_e3
#define AE_DIR_OK AE_E3_0
#define AE_DIR_ERR AE_E3_1
#define AE_FILE_RESULT ae_e4
#define AE_FILE_OK AE_E4_0
#define AE_FILE_ERR AE_E4_1
#define AE_STATUS_RESULT ae_e5
#define AE_STATUS_OK AE_E5_0
#define AE_STATUS_ERR AE_E5_1
#define AE_BOOL_RESULT ae_e6
#define AE_BOOL_OK AE_E6_0
#define AE_BOOL_ERR AE_E6_1
#define AE_COMMAND_RESULT ae_e7
#define AE_COMMAND_OK AE_E7_0
#define AE_COMMAND_ERR AE_E7_1

// ---- JSON reader (ADR-26) ---------------------------------------------------
// A path query, not a document model: one call selects one value and reports it
// as text, an integer or a boolean. The document is rescanned per call, which is
// the right trade for config-sized input and keeps JSON out of the type system:
// there is no container type to own, borrow or release.
#define AE_JSON_OK 0
#define AE_JSON_SYNTAX 1
#define AE_JSON_MISSING 2
#define AE_JSON_EXPECTED 3
#define AE_JSON_DEPTH 64
#define AE_JSON_STRING 0
#define AE_JSON_NUMBER 1
#define AE_JSON_BOOL 2
#define AE_JSON_NULL 3
#define AE_JSON_ARRAY 4
#define AE_JSON_OBJECT 5
#ifndef _WIN32
#else
#endif

typedef struct {
    ae_string body;
} ae_t0;
static void ae_t0_release(ae_t0 *v) {
    ae_str_release(&v->body);
}
static void ae_t0_forfeit(ae_t0 *v) {
    ae_str_forfeit(&v->body);
}

int main(int argc, char **argv) {
    ae_argc_g = (int64_t)argc;
    ae_argv_g = argv;
    ae_string t0 = ae_str_from("the quick brown fox", 10, 16);
    ae_string v0 = t0;
    ae_string t1 = ae_str_from(" ", 11, 30);
    if (!v0.ptr) ae_fail("use of moved string", 11, 15);
    ae_tokens t2 = ae_tokens_new(AE_VIEW(v0), AE_VIEW(t1), 11, 15);
    ae_str_release(&t1);
    ae_tokens v1 = t2;
    int64_t v2 = INT64_C(0);
    int64_t v3 = INT64_C(0);
    int64_t v4 = INT64_C(0);
    ae_tokens_reset(&v1);
    while (1) {
    ae_strview v5;
    if (!ae_tokens_next(&v1, &v5)) break;
    int64_t t3 = ae_add(v2, INT64_C(1), 16, 23);
    v2 = t3;
    int64_t t4 = ae_str_len(AE_VIEW(v5), 17, 27);
    int64_t t5 = ae_add(v3, t4, 17, 25);
    v3 = t5;
    int64_t t6 = ae_str_len(AE_VIEW(v5), 18, 12);
    bool t7 = t6 > v4;
    if (t7) {
    int64_t t8 = ae_str_len(AE_VIEW(v5), 18, 45);
    v4 = t8;
    } else {
    }
    ae_next_0: ;
    }
    if (printf("%" PRId64 "\n", v2) < 0) ae_fail("output error", 0, 0);
    if (printf("%" PRId64 "\n", v3) < 0) ae_fail("output error", 0, 0);
    if (printf("%" PRId64 "\n", v4) < 0) ae_fail("output error", 0, 0);
    ae_string t9 = ae_str_from("alpha beta gamma", 25, 16);
    ae_string v6 = t9;
    ae_string t10 = ae_str_from(" ", 26, 32);
    if (!v6.ptr) ae_fail("use of moved string", 26, 17);
    ae_tokens t11 = ae_tokens_new(AE_VIEW(v6), AE_VIEW(t10), 26, 17);
    ae_str_release(&t10);
    ae_tokens v7 = t11;
    int64_t v8 = INT64_C(0);
    ae_tokens_reset(&v7);
    while (1) {
    ae_strview v9;
    if (!ae_tokens_next(&v7, &v9)) break;
    int64_t t12 = ae_str_len(AE_VIEW(v9), 28, 41);
    int64_t t13 = ae_add(v8, t12, 28, 39);
    v8 = t13;
    ae_next_1: ;
    }
    int64_t v10 = INT64_C(0);
    ae_tokens_reset(&v7);
    while (1) {
    ae_strview v11;
    if (!ae_tokens_next(&v7, &v11)) break;
    int64_t t14 = ae_str_len(AE_VIEW(v11), 30, 43);
    int64_t t15 = ae_add(v10, t14, 30, 41);
    v10 = t15;
    ae_next_2: ;
    }
    if (printf("%" PRId64 "\n", v8) < 0) ae_fail("output error", 0, 0);
    if (printf("%" PRId64 "\n", v10) < 0) ae_fail("output error", 0, 0);
    ae_string t16 = ae_str_from("one two three", 35, 30);
    ae_t0 t17 = { t16 };
    ae_t0 v12 = t17;
    ae_string t18 = ae_str_from(" ", 36, 40);
    if (!v12.body.ptr) ae_fail("use of moved string", 36, 18);
    ae_tokens t19 = ae_tokens_new(AE_VIEW(v12.body), AE_VIEW(t18), 36, 18);
    ae_str_release(&t18);
    ae_tokens v13 = t19;
    int64_t v14 = INT64_C(0);
    ae_tokens_reset(&v13);
    while (1) {
    ae_strview v15;
    if (!ae_tokens_next(&v13, &v15)) break;
    int64_t t20 = ae_add(v14, INT64_C(1), 38, 33);
    v14 = t20;
    ae_next_3: ;
    }
    if (printf("%" PRId64 "\n", v14) < 0) ae_fail("output error", 0, 0);
    ae_strvec t21 = ae_strvec_new(42, 16);
    ae_strvec v16 = t21;
    ae_tokens_reset(&v13);
    while (1) {
    ae_strview v17;
    if (!ae_tokens_next(&v13, &v17)) break;
    if (!v16.data) ae_fail("use of moved vector", 43, 27);
    ae_strvec_push(&v16, ae_str_copy_view(AE_VIEW(v17), 43, 27), 43, 27);
    ae_next_4: ;
    }
    if (!v16.data) ae_fail("use of moved vector", 44, 11);
    ae_string t22 = ae_str_from(",", 44, 22);
    ae_string t23 = ae_join(&v16, AE_VIEW(t22), 44, 11);
    ae_str_release(&t22);
    ae_strview t24 = AE_VIEW(t23); if (fwrite(t24.ptr, 1, (size_t)t24.len, stdout) != (size_t)t24.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t23);
    ae_string t25 = ae_str_from("a::b::::c", 47, 16);
    ae_string v18 = t25;
    ae_string t26 = ae_str_from("::", 48, 32);
    if (!v18.ptr) ae_fail("use of moved string", 48, 17);
    ae_tokens t27 = ae_tokens_new(AE_VIEW(v18), AE_VIEW(t26), 48, 17);
    ae_str_release(&t26);
    ae_tokens v19 = t27;
    int64_t v20 = INT64_C(0);
    ae_tokens_reset(&v19);
    while (1) {
    ae_strview v21;
    if (!ae_tokens_next(&v19, &v21)) break;
    int64_t t28 = ae_str_len(AE_VIEW(v21), 50, 28);
    bool t29 = t28 == INT64_C(0);
    if (t29) {
    int64_t t30 = ae_add(v20, INT64_C(1), 50, 73);
    v20 = t30;
    } else {
    }
    ae_next_5: ;
    }
    if (printf("%" PRId64 "\n", v20) < 0) ae_fail("output error", 0, 0);
    ae_str_release(&v0);
    ae_str_release(&v6);
    ae_t0_release(&v12);
    ae_strvec_release(&v16);
    ae_str_release(&v18);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
