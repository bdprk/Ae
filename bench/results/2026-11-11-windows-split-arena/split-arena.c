#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
// Deliberate termination with a status. Buffered output is flushed first, so a
// program that prints and then exits does not lose the last line to a pipe. The
// code is truncated to int: POSIX shells only see the low eight bits, and
// pretending otherwise in the language would be worse than documenting it.
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
// Concatenations with a literal keep the program text out of the allocator:
// a literal is not an owned value, so it is neither copied into the heap
// first nor freed afterwards; the result owns one new buffer. strlen on a
// literal folds to a constant, so the length costs nothing.
// Formats an i64 into a caller-owned buffer (always >= 24 bytes) and returns
// the length. Same formatter as ae_to_string, minus the heap round-trip:
// a temp that dies inside one expression never needs to own memory.
// `"lit" + to_string(n)` with the digits formatted on the stack: the result
// owns one buffer, and neither the literal nor the digits touch the allocator.
// `to_string(n) + "lit"`: the digits go in front, same zero-alloc rule.
void ae_str_release(ae_string *slot){ ae_free(slot->ptr); slot->ptr = NULL; slot->len = 0; }
bool ae_str_eq(ae_strview a, ae_strview b){
    return a.len == b.len && (a.len == 0 || memcmp(a.ptr, b.ptr, (size_t)a.len) == 0);
}
// UTF-8 (ADR-17): len/slice count bytes, but a slice may not end in the middle
// of a character. Every byte of a multi-byte sequence except the first has the
// pattern 10xxxxxx, so checking that one byte is O(1) and cannot be fooled by
// the slice length. Strings that already contain malformed bytes are left
// alone: the rule forbids introducing broken sequences, it does not validate
// input (read_file and argv are byte streams, and forcing a validation pass
// would make every load O(n) for a guarantee callers rarely want).
// ASCII whitespace, matching what a text file usually contains. Trimming is
// deliberately byte-oriented: it never removes part of a multi-byte character
// (no UTF-8 continuation byte is whitespace).
// Prefix/suffix tests compare bytes, so a suffix check like ends_with(name,
// ".txt") cannot be fooled by a name such as "a.txt.bak" the way contains() can.

ae_string ae_str_from_len(const char *p, int64_t n, size_t line, size_t column){
    char *copy = ae_alloc((size_t)n + 1, line, column);
    memcpy(copy, p, (size_t)n);
    copy[n] = 0;
    ae_string out = { copy, n };
    return out;
}
// Concatenation may not consume a borrowed view, so views are copied into the
// new buffer first. The view itself stays valid and untouched.
// Number of characters, i.e. number of bytes that are not continuations. For
// well-formed text that is the codepoint count; for stray continuation bytes
// the count is still monotone and never exceeds the byte length.
// First `n` characters as an owned string: the character-safe way to truncate
// text without computing byte offsets by hand. `n` beyond the end copies
// everything, so a caller can clamp with utf8_len or just ask for a big number.
// Leading and trailing whitespace removed, as a fresh owned string. The result
// is always a valid slice of the input, so it cannot split a character.

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
void ae_strvec_pop(ae_strvec *v, size_t line, size_t column){
    if (v->len == 0) ae_fail("pop from empty vector", line, column);
    v->len = v->len - 1;
    if (ae_strvec_borrowed(v, &v->data[v->len])) {
        v->data[v->len].ptr = NULL;
        v->data[v->len].len = 0;
    } else {
        ae_str_release(&v->data[v->len]);
    }
}
int64_t ae_strvec_len(ae_strvec v, size_t line, size_t column){
    (void)line; (void)column;
    return v.len;
}
int64_t ae_strvec_index(int64_t index, int64_t len, size_t line, size_t column){
    if (index < 0 || index >= len) ae_fail("array index out of bounds", line, column);
    return index;
}
ae_string *ae_strvec_get(ae_strvec *v, int64_t index, size_t line, size_t column){
    ae_strvec_index(index, v->len, line, column);
    return &v->data[index];
}
// Borrowed string vectors (&vec[string]) read elements through this accessor.
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
// Overwriting an element releases the old value first; an element inside the
// vector's own arena is cleared without freeing (ADR-49).
void ae_strvec_clear_slot(ae_strvec *v, int64_t index, size_t line, size_t column){
    ae_strvec_index(index, v->len, line, column);
    if (ae_strvec_borrowed(v, &v->data[index])) {
        v->data[index].ptr = NULL;
        v->data[index].len = 0;
    } else {
        ae_str_release(&v->data[index]);
    }
}
// Next separator position at or after `from`, or -1 when none is left. A
// one-byte separator (the common `" "` and `"\n"` shapes) goes through memchr,
// which is SIMD-optimised in both libc implementations, instead of comparing
// byte by byte (ADR-49).
int64_t ae_find_sep(const char *base, int64_t from, int64_t len, ae_strview sep){
    if (from >= len) return -1;
    if (sep.len == 1) {
        const char *hit = memchr(base + from, sep.ptr[0], (size_t)(len - from));
        return hit == NULL ? -1 : (int64_t)(hit - base);
    }
    for (int64_t i = from; i + sep.len <= len; i++) {
        if (memcmp(base + i, sep.ptr, (size_t)sep.len) == 0) return i;
    }
    return -1;
}

// Views only: splitting never takes ownership of its operands, so a borrowed
// `&string` argument works exactly like an owned one.
//
// Two passes (ADR-49): the first counts tokens and the bytes they carry, so the
// pointer array and the byte arena are allocated once at their exact size.
// Each token keeps its terminator, so `ptr` stays usable with C string APIs.
ae_strvec ae_split(ae_strview s, ae_strview sep, size_t line, size_t column){
    if (sep.len == 0) ae_fail("empty separator", line, column);
    int64_t count = 1;
    for (int64_t at = ae_find_sep(s.ptr, 0, s.len, sep); at >= 0; at = ae_find_sep(s.ptr, at + sep.len, s.len, sep)) {
        count += 1;
    }
    if (count > INT64_MAX / (int64_t)sizeof(ae_string)) ae_fail("vector too large", line, column);
    // Input bytes minus the separators, plus one terminator per token.
    int64_t bytes = (s.len - (count - 1) * sep.len) + count;
    ae_strvec out;
    out.data = ae_alloc((size_t)count * sizeof(ae_string), line, column);
    out.len = 0;
    out.cap = count;
    out.arena = ae_alloc((size_t)bytes, line, column);
    out.arena_len = bytes;
    char *cursor = out.arena;
    int64_t start = 0;
    for (int64_t at = ae_find_sep(s.ptr, 0, s.len, sep); at >= 0; at = ae_find_sep(s.ptr, at + sep.len, s.len, sep)) {
        int64_t n = at - start;
        if (n > 0) memcpy(cursor, s.ptr + start, (size_t)n);
        cursor[n] = 0;
        out.data[out.len].ptr = cursor;
        out.data[out.len].len = n;
        out.len += 1;
        cursor += n + 1;
        start = at + sep.len;
    }
    int64_t n = s.len - start;
    if (n > 0) memcpy(cursor, s.ptr + start, (size_t)n);
    cursor[n] = 0;
    out.data[out.len].ptr = cursor;
    out.data[out.len].len = n;
    out.len += 1;
    return out;
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

// lines(): split on newlines. Trailing newlines do not produce a final empty
// line, matching common stdlib lines(). Two passes with memchr (ADR-49): count
// lines first, then fill one exact-size pointer array and one byte arena.
ae_strvec ae_lines(ae_strview s, size_t line, size_t column){
    int64_t newlines = 0;
    const char *p = s.ptr;
    int64_t rem = s.len;
    while (rem > 0) {
        const char *hit = memchr(p, '\n', (size_t)rem);
        if (hit == NULL) break;
        newlines += 1;
        rem -= (int64_t)(hit - p) + 1;
        p = hit + 1;
    }
    int64_t count = newlines + ((s.len > 0 && s.ptr[s.len - 1] != '\n') ? 1 : 0);
    if (count > INT64_MAX / (int64_t)sizeof(ae_string)) ae_fail("vector too large", line, column);
    int64_t bytes = (s.len - newlines) + count;
    ae_strvec out;
    out.data = ae_alloc((size_t)(count > 0 ? count : 1) * sizeof(ae_string), line, column);
    out.len = 0;
    out.cap = count;
    out.arena = ae_alloc((size_t)(bytes > 0 ? bytes : 1), line, column);
    out.arena_len = bytes > 0 ? bytes : 1;
    char *cursor = out.arena;
    p = s.ptr;
    rem = s.len;
    while (rem > 0) {
        const char *hit = memchr(p, '\n', (size_t)rem);
        if (hit == NULL) break;
        int64_t n = (int64_t)(hit - p);
        if (n > 0) memcpy(cursor, p, (size_t)n);
        cursor[n] = 0;
        out.data[out.len].ptr = cursor;
        out.data[out.len].len = n;
        out.len += 1;
        cursor += n + 1;
        rem -= n + 1;
        p = hit + 1;
    }
    if (rem > 0) {
        if (rem > 0) memcpy(cursor, p, (size_t)rem);
        cursor[rem] = 0;
        out.data[out.len].ptr = cursor;
        out.data[out.len].len = rem;
        out.len += 1;
    }
    return out;
}

// count_lines_with(text, pattern): newline-delimited line count where the
// line contains the pattern. Single pass, zero allocations; line search
// uses memchr for first-byte skipping.

// Program arguments bridge: the generated main records argc/argv here.
static int64_t ae_argc_g = 0;
static char **ae_argv_g = NULL;

// Owned growable i64 vector. Moves copy the struct and forfeit the source
// slot (zeroed without freeing); releases are idempotent, so overlapping
// release paths can never double-free.
typedef struct { int64_t *data; int64_t len; int64_t cap; } ae_vec;
// Zero a slot whose contents were moved elsewhere: no free.

// In-place orderings for i64 vectors. qsort is the C library's introsort and
// the comparator is a plain function, so no allocation and no closure state is
// involved; vectors are at most INT64_MAX/8 elements, which qsort accepts as a
// size_t count on both platforms.

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
// UTF-8 to UTF-16 for the wide-character Win32 APIs; callers free the result.
// UTF-16 to a fresh UTF-8 buffer of `len` units; returns NULL on failure.
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

// FNV-1a: fixed and platform independent, so iteration order is reproducible.
uint64_t ae_map_hash(ae_strview key){
    uint64_t hash = UINT64_C(1469598103934665603);
    for (int64_t i = 0; i < key.len; i++) {
        hash ^= (uint64_t)(unsigned char)key.ptr[i];
        hash *= UINT64_C(1099511628211);
    }
    return hash;
}
int64_t ae_map_slot_of(ae_strview key, int64_t cap){
    return (int64_t)(ae_map_hash(key) & (uint64_t)(cap - 1));
}
// Index of the matching full slot, or -1. Stops only at an empty slot.
int64_t ae_map_find(const ae_map *m, ae_strview key){
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
// Inserts or overwrites. The key is copied; the caller keeps its own string.
// `map_inc(m, key, delta)`: the canonical counting operation. One hash
// locates the slot, the delta is added in place, and the new value comes
// back; a missing key starts from zero, which is what every histogram wants
// (Go's `m[k] += d`, Rust's `entry(k).or_insert(0) += d`). Unlike
// ae_map_get there is no error value: absence is part of the semantics.
// An existing key is never copied; only a first sighting allocates.
bool ae_map_lookup(const ae_map *m, ae_strview key, int64_t *out){
    const int64_t at = ae_map_find(m, key);
    if (at < 0) return false;
    if (out) *out = m->slots[at].value;
    return true;
}

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


// Wall-clock time in milliseconds since the Unix epoch. File metadata reports
// whole seconds, so now_s exists to compare against it without dividing on
// every call. Both are i64: the epoch range fits comfortably, and negative
// values (before 1970) stay meaningful.

// Error text carrying a platform status code where strerror does not apply.



// Appending exists as its own operation because "read, concatenate, write" is
// not the same thing: it rewrites the whole file, loses the atomicity of a
// single append, and fails on files the process may only append to.

// Environment lookup. A variable that is set but empty is Ok(""), a variable
// that does not exist is Err: collapsing the two would silently turn "PATH is
// empty" into "PATH is missing".

// Everything on standard input, read as bytes. Windows starts in text mode,
// which would turn CRLF into LF and make the byte count differ between
// platforms, so the descriptor is switched to binary first. The buffer grows by
// doubling; a fully read pipe returns however much arrived, which is not an
// error -- "no input" and "failed to read input" must stay distinguishable.

// Current working directory, in the same `/`-separated form the rest of the
// language uses for paths.

// Byte offset of the first match, or an Err value when there is none. The
// views carry explicit lengths, so embedded NUL bytes are searched as well —
// strstr() would stop at the first one. memchr() is the platform's optimized
// byte scan; the success path allocates nothing, and the failure path only
// allocates the error string the result enum already owns.

// Strict integer parsing: optional sign, ASCII digits only, whole string, and
// no wrap-around. Anything else is an error value, never a silent default.

ae_string ae_to_string(int64_t value, size_t line, size_t column){
    char buffer[24];
    const int length = snprintf(buffer, sizeof buffer, "%" PRId64, value);
    if (length < 0) ae_fail("cannot format integer", line, column);
    return ae_str_from_len(buffer, (int64_t)length, line, column);
}

// ---- f64 text (ADR-37) ------------------------------------------------------
// One spelling on every platform. `%g` alone is not enough: UCRT writes a NaN
// as "-nan(ind)" while glibc writes "-nan", and exponent widths have differed
// between CRTs. So specials are written here, and finite values are rendered by
// the shortest precision that round-trips, choosing the shorter of fixed and
// scientific notation (ties go to fixed). This is a canonical form, not the
// shortest possible decimal: 0.1 prints as 0.1, 1.0 as 1.0, 1/3 with the 17
// digits needed to get the same bits back.


// Rewrites "1.5e-005" as "1.5e-5": lowercase marker, sign always present, and
// no leading zeros in the exponent. Both CRTs already agree on this for the
// values tested, but the language must not depend on that agreement.









// Directory listing: entry names only, never "." or "..", sorted by bytes so
// repeated runs and the two platforms agree on order.
// In-place orderings for owned string vectors, byte-order like keys() and
// directory listings so every sorted output is comparable across platforms.

// ---- Path algebra -----------------------------------------------------------
// Separators: both '/' and '\' are accepted on input, but '/'-joined output is
// what this layer emits, because Windows accepts '/' and identical output on
// both platforms keeps tests and recorded results comparable.


// Concatenate `dir` and `name`, inserting a separator only when needed.
// An absolute name (leading separator, or a drive letter) replaces the
// directory instead of being appended to it.
// Final component of a path. Trailing separators are dropped first so
// "dir/name/" yields "name"; a path that is only separators keeps one so the
// root stays recognisable.
// Everything before the final component. A path without a separator has no
// parent, which reports as the empty string rather than a fabricated "." .

// ---- Metadata ---------------------------------------------------------------
// Size in bytes, modification time in seconds since the Unix epoch, and whether
// the target is a directory. Failure is a value, never a silent zero size.

// ---- Mutation --------------------------------------------------------------


// Removes a file. Directories are refused on both platforms: POSIX remove()
// would also delete an empty directory, and reporting the same outcome on two
// platforms matters more than the extra convenience.

// Returns an empty string on success, or the failure text. Keeping the message
// as the return value lets the caller decide whether a race is acceptable.
// Creates the directory and every missing parent, so a caller can write
// "out/2026/march/" in one step. Existing directories are not an error.

// Changes the process working directory. This is a global mutation: every
// later relative path, including a child process's, resolves against the new
// directory. It is a result value for the same reason the rest are - the
// target may be gone or unreadable - and no undo is provided, so callers keep
// the old cwd() if they intend to come back.

// Removes an empty directory. Recursive removal is deliberately absent: a
// helper that silently deletes a tree is the easiest way to lose data.

// ---- Map helpers that need the prelude result types -------------------------
// A missing key is a value, not a zero: `0` is a perfectly normal count.
AE_INT_RESULT ae_map_get(const ae_map *m, ae_strview key, size_t line, size_t column){
    int64_t value = 0;
    if (!ae_map_lookup(m, key, &value)) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from("key not found", line, column) } } };
        return out;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { value } } };
    return out;
}
// Keys are copied and sorted by bytes, so iteration is reproducible and the
// caller owns a plain string vector.

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

// Decodes one JSON string at *i into an owned string, resolving escapes (and
// surrogate pairs) so the language never sees a \uXXXX sequence.
// Locates one member inside the object at *pos, leaving its value span in
// *vs/*ve. *found separates "no such member" from "not an object at all".
// Validates the path on its own first, so a typo in the path is reported as a
// path error even when the document would have failed on the same segment.
// Path grammar: `key`, `key.sub`, `key[2].sub`, `[0].key`. A segment is a run of
// bytes up to the next '.' or '['; keys are compared after unescaping.

// ---- Child processes (ADR-29) ----------------------------------------------
// The child's stdout and stderr share one pipe: a program that reports its
// failure on stderr still comes back in the order it was written, and the
// parent never has to drain two pipes at once (which deadlocks as soon as one
// of them fills). A non-zero exit status is a result, not an error.
#ifndef _WIN32
#else
#endif

int main(int argc, char **argv) {
    ae_argc_g = (int64_t)argc;
    ae_argv_g = argv;
    ae_string t0 = ae_str_from("alpha beta gamma delta", 7, 23);
    ae_string t1 = ae_str_from(" ", 7, 49);
    ae_strvec t2 = ae_split(AE_VIEW(t0), AE_VIEW(t1), 7, 17);
    ae_str_release(&t0);
    ae_str_release(&t1);
    ae_strvec v0 = t2;
    if (!v0.data) ae_fail("use of moved vector", 8, 5);
    ae_strvec_pop(&v0, 8, 5);
    if (!v0.data) ae_fail("use of moved vector", 9, 10);
    int64_t t3 = ae_strvec_index(INT64_C(0), v0.len, 9, 10);
    ae_string t4 = ae_str_from("ALPHA", 9, 16);
    ae_strvec_clear_slot(&v0, t3, 9, 10);
    v0.data[t3] = t4;
    if (!v0.data) ae_fail("use of moved vector", 10, 5);
    ae_string t5 = ae_str_from("tail", 10, 17);
    ae_strvec_push(&v0, t5, 10, 5);
    if (!v0.data) ae_fail("use of moved vector", 11, 11);
    int64_t t6 = ae_strvec_len(v0, 11, 11);
    if (printf("%" PRId64 "\n", t6) < 0) ae_fail("output error", 0, 0);
    if (!v0.data) ae_fail("use of moved vector", 12, 16);
    ae_strview t7 = AE_VIEW((*ae_strvec_get(&v0, INT64_C(0), 12, 16))); if (fwrite(t7.ptr, 1, (size_t)t7.len, stdout) != (size_t)t7.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    if (!v0.data) ae_fail("use of moved vector", 13, 11);
    ae_string t8 = ae_str_from("|", 13, 23);
    ae_string t9 = ae_join(&v0, AE_VIEW(t8), 13, 11);
    ae_str_release(&t8);
    ae_strview t10 = AE_VIEW(t9); if (fwrite(t10.ptr, 1, (size_t)t10.len, stdout) != (size_t)t10.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t9);
    ae_string t11 = ae_str_from("one\ntwo\nthree\n", 14, 22);
    ae_strvec t12 = ae_lines(AE_VIEW(t11), 14, 16);
    ae_str_release(&t11);
    ae_strvec v1 = t12;
    if (!v1.data) ae_fail("use of moved vector", 15, 11);
    int64_t t13 = ae_strvec_len(v1, 15, 11);
    if (printf("%" PRId64 "\n", t13) < 0) ae_fail("output error", 0, 0);
    if (!v1.data) ae_fail("use of moved vector", 16, 15);
    ae_strview t14 = AE_VIEW((*ae_strvec_get(&v1, INT64_C(2), 16, 15))); if (fwrite(t14.ptr, 1, (size_t)t14.len, stdout) != (size_t)t14.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_string t15 = ae_str_from("", 19, 23);
    ae_strvec t16 = ae_lines(AE_VIEW(t15), 19, 17);
    ae_str_release(&t15);
    ae_strvec v2 = t16;
    if (!v2.data) ae_fail("use of moved vector", 20, 5);
    ae_string t17 = ae_str_from("grown", 20, 17);
    ae_strvec_push(&v2, t17, 20, 5);
    if (!v2.data) ae_fail("use of moved vector", 21, 11);
    int64_t t18 = ae_strvec_len(v2, 21, 11);
    if (printf("%" PRId64 "\n", t18) < 0) ae_fail("output error", 0, 0);
    if (!v2.data) ae_fail("use of moved vector", 22, 16);
    ae_strview t19 = AE_VIEW((*ae_strvec_get(&v2, INT64_C(0), 22, 16))); if (fwrite(t19.ptr, 1, (size_t)t19.len, stdout) != (size_t)t19.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_strvec_release(&v0);
    ae_strvec_release(&v1);
    ae_strvec_release(&v2);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
