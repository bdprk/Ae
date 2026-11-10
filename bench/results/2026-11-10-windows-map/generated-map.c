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
void ae_free(void *block){ if (block) ae_allocator_impl.release(block); }
void ae_set_allocator(ae_allocator impl){ ae_allocator_impl = impl; }

int64_t ae_add(int64_t a, int64_t b, size_t line, size_t column){
    if ((b > 0 && a > INT64_MAX - b) || (b < 0 && a < INT64_MIN - b)) ae_fail("integer overflow", line, column);
    return a + b;
}
int64_t ae_mul(int64_t a, int64_t b, size_t line, size_t column){
    if (a > 0 ? (b > 0 ? a > INT64_MAX / b : b < INT64_MIN / a)
              : (a < 0 ? (b > 0 ? a < INT64_MIN / b : b < 0 && a < INT64_MAX / b) : 0))
        ae_fail("integer overflow", line, column);
    return a * b;
}
int64_t ae_mod(int64_t a, int64_t b, size_t line, size_t column){
    if (b == 0) ae_fail("division by zero", line, column);
    if (a == INT64_MIN && b == -1) ae_fail("integer overflow", line, column);
    return a % b;
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
typedef struct { ae_string *data; int64_t len; int64_t cap; } ae_strvec;
// Borrowed string vectors (&vec[string]) read elements through this accessor.
// Views only: splitting never takes ownership of its operands, so a borrowed
// `&string` argument works exactly like an owned one.


// lines(): split on newlines with a word-at-a-time scan (SWAR). Trailing
// newlines do not produce a final empty line, matching common stdlib lines().
#define AE_HASZERO(x) (((x) - 0x0101010101010101ULL) & ~(x) & 0x8080808080808080ULL)

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

ae_map ae_map_new(size_t line, size_t column){
    (void)line; (void)column;
    ae_map m;
    m.slots = NULL;
    m.cap = 0;
    m.len = 0;
    m.used = 0;
    return m;
}
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
void ae_map_grow(ae_map *m, size_t line, size_t column){
    int64_t cap = m->cap == 0 ? 8 : m->cap;
    // Rehash into a table twice as large when live entries fill three quarters,
    // which also drops every tombstone.
    while ((m->len + 1) * 4 >= cap * 3) {
        if (cap > INT64_MAX / 4) ae_fail("map too large", line, column);
        cap = cap * 2;
    }
    if (cap == m->cap && m->used == m->len) return;
    // Zeroed by hand rather than with a calloc-style interface: the slot table
    // needs EMPTY in every byte, and keeping one allocation primitive makes the
    // allocator interface a single implementation instead of two.
    ae_map_slot *slots = ae_alloc((size_t)cap * sizeof(ae_map_slot), line, column);
    memset(slots, 0, (size_t)cap * sizeof(ae_map_slot));
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state != AE_MAP_FULL) continue;
        int64_t at = ae_map_slot_of(AE_VIEW(m->slots[i].key), cap);
        while (slots[at].state == AE_MAP_FULL) at = (at + 1) & (cap - 1);
        slots[at] = m->slots[i];
    }
    ae_free(m->slots);
    m->slots = slots;
    m->cap = cap;
    m->used = m->len;
}
// Inserts or overwrites. The key is copied; the caller keeps its own string.
void ae_map_put(ae_map *m, ae_strview key, int64_t value, size_t line, size_t column){
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
bool ae_map_lookup(const ae_map *m, ae_strview key, int64_t *out){
    const int64_t at = ae_map_find(m, key);
    if (at < 0) return false;
    if (out) *out = m->slots[at].value;
    return true;
}
int64_t ae_map_len(const ae_map *m, size_t line, size_t column){
    (void)line; (void)column;
    return m->len;
}
void ae_map_release(ae_map *m){
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state == AE_MAP_FULL) ae_str_release(&m->slots[i].key);
    }
    ae_free(m->slots);
    m->slots = NULL;
    m->cap = 0;
    m->len = 0;
    m->used = 0;
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
void ae_e2_release(ae_e2 *v){
    if (v->tag == AE_E2_0) {
        v->u.v0.p0 = 0;
    }
    if (v->tag == AE_E2_1) {
        ae_str_release(&v->u.v1.p0);
    }
}

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
    ae_map t0 = ae_map_new(9, 18);
    ae_map v0 = t0;
    int64_t v1 = INT64_C(1234567);
    int64_t v3 = INT64_C(0);
    int64_t v2 = INT64_C(1000000);
    while (1) {
    if (!(v3 < v2)) break;
    int64_t t1 = ae_mul(v1, INT64_C(48271), 12, 16);
    int64_t t2 = ae_mod(t1, INT64_C(2147483647), 12, 25);
    v1 = t2;
    ae_string t3 = ae_str_from("k", 13, 19);
    int64_t t4 = ae_mod(v1, INT64_C(10000), 13, 37);
    ae_string t5 = ae_to_string(t4, 13, 25);
    ae_string t6 = ae_str_cat(t3, t5, 13, 23);
    ae_string v4 = t6;
    if (!v4.ptr) ae_fail("use of moved string", 14, 33);
    ae_e2 t7 = ae_map_get(&v0, AE_VIEW(v4), 14, 17);
    ae_e2 v5 = t7;
    if (v5.tag == AE_E2_0) {
    int64_t v6 = v5.u.v0.p0;
    if (!v4.ptr) ae_fail("use of moved string", 16, 42);
    int64_t t8 = ae_add(v6, INT64_C(1), 16, 51);
    ae_map_put(&v0, AE_VIEW(v4), t8, 16, 41);
    } else if (v5.tag == AE_E2_1) {
    ae_strview v7 = AE_VIEW(v5.u.v1.p0);
    if (!v4.ptr) ae_fail("use of moved string", 17, 48);
    ae_map_put(&v0, AE_VIEW(v4), INT64_C(1), 17, 47);
    }
    ae_str_release(&v4);
    ae_e2_release(&v5);
    ae_next_0: ;
    v3 = ae_add(v3, 1, 11, 25);
    }
    int64_t v8 = INT64_C(0);
    int64_t v10 = INT64_C(0);
    int64_t v9 = INT64_C(10000);
    while (1) {
    if (!(v10 < v9)) break;
    ae_string t9 = ae_str_from("k", 22, 19);
    ae_string t10 = ae_to_string(v10, 22, 25);
    ae_string t11 = ae_str_cat(t9, t10, 22, 23);
    ae_string v11 = t11;
    if (!v11.ptr) ae_fail("use of moved string", 23, 33);
    ae_e2 t12 = ae_map_get(&v0, AE_VIEW(v11), 23, 17);
    ae_e2 v12 = t12;
    if (v12.tag == AE_E2_0) {
    int64_t v13 = v12.u.v0.p0;
    int64_t t13 = ae_add(v10, INT64_C(1), 25, 64);
    int64_t t14 = ae_mul(v13, t13, 25, 59);
    int64_t t15 = ae_add(v8, t14, 25, 55);
    v8 = t15;
    } else if (v12.tag == AE_E2_1) {
    ae_strview v14 = AE_VIEW(v12.u.v1.p0);
    int64_t t16 = ae_add(v8, INT64_C(0), 26, 61);
    v8 = t16;
    }
    ae_str_release(&v11);
    ae_e2_release(&v12);
    ae_next_1: ;
    v10 = ae_add(v10, 1, 21, 23);
    }
    if (fputs("draws=1000000\n", stdout) == EOF) ae_fail("output error", 0, 0);
    if (fputs("keys=10000\n", stdout) == EOF) ae_fail("output error", 0, 0);
    ae_string t17 = ae_str_from("distinct=", 31, 11);
    if (!v0.slots) ae_fail("use of moved map", 31, 35);
    int64_t t18 = ae_map_len(&v0, 31, 35);
    ae_string t19 = ae_to_string(t18, 31, 25);
    ae_string t20 = ae_str_cat(t17, t19, 31, 23);
    ae_strview t21 = AE_VIEW(t20); if (fwrite(t21.ptr, 1, (size_t)t21.len, stdout) != (size_t)t21.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t20);
    if (fputs("total=1000000\n", stdout) == EOF) ae_fail("output error", 0, 0);
    ae_string t22 = ae_str_from("checksum=", 33, 11);
    ae_string t23 = ae_to_string(v8, 33, 25);
    ae_string t24 = ae_str_cat(t22, t23, 33, 23);
    ae_strview t25 = AE_VIEW(t24); if (fwrite(t25.ptr, 1, (size_t)t25.len, stdout) != (size_t)t25.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t24);
    ae_map_release(&v0);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
