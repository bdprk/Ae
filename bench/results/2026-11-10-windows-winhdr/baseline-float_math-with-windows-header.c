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
ae_string ae_str_copy_view(ae_strview s, size_t line, size_t column){
    return ae_str_from_len(s.ptr, s.len, line, column);
}
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
ae_vec ae_vec_new(size_t line, size_t column){
    ae_vec v;
    v.len = 0;
    v.cap = 4;
    v.data = ae_alloc((size_t)v.cap * sizeof(int64_t), line, column);
    return v;
}
void ae_vec_push(ae_vec *v, int64_t x, size_t line, size_t column){
    if (v->len == v->cap) {
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap * 2;
        int64_t *nd = ae_alloc_resize(v->data, (size_t)ncap * sizeof(int64_t), line, column);
        v->data = nd;
        v->cap = ncap;
    }
    v->data[v->len] = x;
    v->len = v->len + 1;
}
int64_t ae_vec_len(ae_vec v, size_t line, size_t column){
    (void)line; (void)column;
    return v.len;
}
int64_t ae_vec_index(int64_t index, int64_t len, size_t line, size_t column){
    if (index < 0 || index >= len) ae_fail("array index out of bounds", line, column);
    return index;
}
int64_t ae_vec_get(ae_vec v, int64_t index, size_t line, size_t column){
    ae_vec_index(index, v.len, line, column);
    return v.data[index];
}
void ae_vec_release(ae_vec *v){
    ae_free(v->data);
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}
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
// header costs ~690 ms, with this define ~520 ms. Every generated program pays
// that cost at build time, so keep the header surface minimal.
#ifndef WIN32_LEAN_AND_MEAN
#define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>
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
// Index of the matching full slot, or -1. Stops only at an empty slot.
// Inserts or overwrites. The key is copied; the caller keeps its own string.

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
size_t ae_f64_fixed(double value, char *out, size_t cap){
    for (int decimals = 0; decimals <= 17; decimals++) {
        const int written = snprintf(out, cap, "%.*f", decimals, value);
        if (written < 0 || (size_t)written >= cap) return 0;
        if (strtod(out, NULL) != value) continue;
        size_t length = (size_t)written;
        char *dot = memchr(out, '.', length);
        if (dot) {
            char *end = out + length;
            // Keep one fractional digit: "1.0" reads as a float, "1." does not.
            while (end > dot + 2 && end[-1] == '0') end -= 1;
            length = (size_t)(end - out);
        }
        return length;
    }
    return 0;
}

size_t ae_f64_scientific(double value, char *out, size_t cap){
    for (int digits = 1; digits <= 17; digits++) {
        const int written = snprintf(out, cap, "%.*e", digits - 1, value);
        if (written < 0 || (size_t)written >= cap) return 0;
        if (strtod(out, NULL) == value) return (size_t)written;
    }
    return 0;
}

// Rewrites "1.5e-005" as "1.5e-5": lowercase marker, sign always present, and
// no leading zeros in the exponent. Both CRTs already agree on this for the
// values tested, but the language must not depend on that agreement.
size_t ae_f64_exponent_form(char *text, size_t length){
    char *marker = memchr(text, 'e', length);
    if (!marker) marker = memchr(text, 'E', length);
    if (!marker) return length;
    const size_t mantissa = (size_t)(marker - text);
    char sign = '+';
    size_t at = mantissa + 1;
    if (at < length && (text[at] == '+' || text[at] == '-')) { sign = text[at]; at += 1; }
    while (at < length && text[at] == '0') at += 1;
    size_t cursor = mantissa;
    text[cursor++] = 'e';
    text[cursor++] = sign;
    if (at == length) {
        text[cursor++] = '0';
    } else {
        while (at < length) text[cursor++] = text[at++];
    }
    return cursor;
}

size_t ae_f64_text(double value, char *out, size_t cap){
    if (isnan(value)) { memcpy(out, "nan", 3); return 3; }
    if (isinf(value)) {
        const char *text = value < 0.0 ? "-inf" : "inf";
        const size_t length = strlen(text);
        memcpy(out, text, length);
        return length;
    }
    char fixed[64];
    char scientific[64];
    // Beyond 1e17 fixed notation needs more than 64 bytes; skip it there
    // instead of truncating a value that would then fail to round-trip.
    const size_t fixed_length = (value > -1e17 && value < 1e17) ? ae_f64_fixed(value, fixed, sizeof fixed) : 0;
    const size_t scientific_length = ae_f64_scientific(value, scientific, sizeof scientific);
    char *chosen = fixed;
    size_t length = fixed_length;
    if (scientific_length > 0 && (fixed_length == 0 || scientific_length < fixed_length)) {
        chosen = scientific;
        length = scientific_length;
    }
    if (length == 0) { memcpy(out, "nan", 3); return 3; }
    length = ae_f64_exponent_form(chosen, length);
    const bool plain = !memchr(chosen, '.', length) && !memchr(chosen, 'e', length);
    memcpy(out, chosen, length);
    if (plain && length + 2 <= cap) {
        out[length++] = '.';
        out[length++] = '0';
    }
    return length;
}

ae_string ae_f64_to_string(double value, size_t line, size_t column){
    char buffer[96];
    const size_t length = ae_f64_text(value, buffer, sizeof buffer);
    return ae_str_from_len(buffer, (int64_t)length, line, column);
}


double ae_f64_div(double a, double b, size_t line, size_t column){
    // Same rule as integer division: zero is a reported failure, never a
    // silent infinity. Non-finite results from overflow are still IEEE values.
    if (b == 0.0) ae_fail("f64 division by zero", line, column);
    return a / b;
}

double ae_f64_from_i64(int64_t value, size_t line, size_t column){
    // Every integer up to 2^53 is exact; beyond that the conversion would
    // round silently, so it is refused instead of approximated.
    if (value > INT64_C(9007199254740992) || value < -INT64_C(9007199254740992)) {
        ae_fail("i64 to f64 conversion would lose precision", line, column);
    }
    return (double)value;
}

AE_INT_RESULT ae_f64_to_i64(double value, size_t line, size_t column){
    if (!isfinite(value)) {
        AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("f64 value is not finite", line, column) } } };
        return bad;
    }
    if (value < -9223372036854775808.0 || value >= 9223372036854775808.0) {
        AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("f64 value is outside the i64 range", line, column) } } };
        return bad;
    }
    const int64_t converted = (int64_t)value;
    if ((double)converted != value) {
        AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("f64 value is not an exact integer", line, column) } } };
        return bad;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { converted } } };
    return out;
}



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

static double ae_fn_0(const ae_vec *);

static double ae_fn_1(const ae_vec *, double v1);

static double ae_fn_0(const ae_vec *v0) {
    double v1 = 0.0;
    int64_t v3 = INT64_C(0);
    if (!v0->data) ae_fail("use of moved vector", 6, 17);
    int64_t t0 = ae_vec_len((*v0), 6, 17);
    int64_t v2 = t0;
    while (1) {
    if (!(v3 < v2)) break;
    if (!v0->data) ae_fail("use of moved vector", 6, 60);
    int64_t t1 = ae_vec_get((*v0), v3, 6, 60);
    double t2 = ae_f64_from_i64(t1, 6, 47);
    double t3 = v1 + t2;
    v1 = t3;
    ae_next_0: ;
    v3 = ae_add(v3, 1, 6, 29);
    }
    if (!v0->data) ae_fail("use of moved vector", 7, 27);
    int64_t t4 = ae_vec_len((*v0), 7, 27);
    double t5 = ae_f64_from_i64(t4, 7, 20);
    double t6 = ae_f64_div(v1, t5, 7, 18);
    return t6;
    ae_fail("function exited without return", 0, 0);
}

static double ae_fn_1(const ae_vec *v0, double v1) {
    double v2 = 0.0;
    int64_t v4 = INT64_C(0);
    if (!v0->data) ae_fail("use of moved vector", 12, 17);
    int64_t t0 = ae_vec_len((*v0), 12, 17);
    int64_t v3 = t0;
    while (1) {
    if (!(v4 < v3)) break;
    if (!v0->data) ae_fail("use of moved vector", 13, 32);
    int64_t t1 = ae_vec_get((*v0), v4, 13, 32);
    double t2 = ae_f64_from_i64(t1, 13, 19);
    double t3 = t2 - v1;
    double v5 = t3;
    double t4 = v5 * v5;
    double t5 = v2 + t4;
    v2 = t5;
    ae_next_1: ;
    v4 = ae_add(v4, 1, 12, 29);
    }
    if (!v0->data) ae_fail("use of moved vector", 16, 27);
    int64_t t6 = ae_vec_len((*v0), 16, 27);
    double t7 = ae_f64_from_i64(t6, 16, 20);
    double t8 = ae_f64_div(v2, t7, 16, 18);
    return t8;
    ae_fail("function exited without return", 0, 0);
}

int main(int argc, char **argv) {
    ae_argc_g = (int64_t)argc;
    ae_argv_g = argv;
    ae_vec t0 = ae_vec_new(20, 19);
    ae_vec_push(&t0, INT64_C(2), 20, 19);
    ae_vec_push(&t0, INT64_C(4), 20, 19);
    ae_vec_push(&t0, INT64_C(4), 20, 19);
    ae_vec_push(&t0, INT64_C(4), 20, 19);
    ae_vec_push(&t0, INT64_C(5), 20, 19);
    ae_vec_push(&t0, INT64_C(5), 20, 19);
    ae_vec_push(&t0, INT64_C(7), 20, 19);
    ae_vec_push(&t0, INT64_C(9), 20, 19);
    ae_vec v0 = t0;
    double t1 = ae_fn_0(&v0);
    double v1 = t1;
    ae_string t2 = ae_str_from("count ", 22, 11);
    if (!v0.data) ae_fail("use of moved vector", 22, 32);
    int64_t t3 = ae_vec_len(v0, 22, 32);
    ae_string t4 = ae_to_string(t3, 22, 22);
    ae_string t5 = ae_str_cat(t2, t4, 22, 20);
    ae_strview t6 = AE_VIEW(t5); if (fwrite(t6.ptr, 1, (size_t)t6.len, stdout) != (size_t)t6.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t5);
    ae_string t7 = ae_str_from("mean ", 23, 11);
    ae_string t8 = ae_f64_to_string(v1, 23, 21);
    ae_string t9 = ae_str_cat(t7, t8, 23, 19);
    ae_strview t10 = AE_VIEW(t9); if (fwrite(t10.ptr, 1, (size_t)t10.len, stdout) != (size_t)t10.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t9);
    ae_string t11 = ae_str_from("variance ", 24, 11);
    double t12 = ae_fn_1(&v0, v1);
    ae_string t13 = ae_f64_to_string(t12, 24, 25);
    ae_string t14 = ae_str_cat(t11, t13, 24, 23);
    ae_strview t15 = AE_VIEW(t14); if (fwrite(t15.ptr, 1, (size_t)t15.len, stdout) != (size_t)t15.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t14);
    ae_string t16 = ae_str_from("share ", 25, 11);
    if (!v0.data) ae_fail("use of moved vector", 25, 46);
    int64_t t17 = ae_vec_get(v0, INT64_C(3), 25, 46);
    double t18 = ae_f64_from_i64(t17, 25, 32);
    if (!v0.data) ae_fail("use of moved vector", 25, 60);
    int64_t t19 = ae_vec_len(v0, 25, 60);
    double t20 = ae_f64_from_i64(t19, 25, 53);
    double t21 = ae_f64_div(t18, t20, 25, 51);
    ae_string t22 = ae_f64_to_string(t21, 25, 22);
    ae_string t23 = ae_str_cat(t16, t22, 25, 20);
    ae_strview t24 = AE_VIEW(t23); if (fwrite(t24.ptr, 1, (size_t)t24.len, stdout) != (size_t)t24.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t23);
    ae_string t25 = ae_str_from("third ", 26, 11);
    double t26 = ae_f64_div(1.0, 3.0, 26, 36);
    ae_string t27 = ae_f64_to_string(t26, 26, 22);
    ae_string t28 = ae_str_cat(t25, t27, 26, 20);
    ae_strview t29 = AE_VIEW(t28); if (fwrite(t29.ptr, 1, (size_t)t29.len, stdout) != (size_t)t29.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t28);
    ae_e2 t30 = ae_f64_to_i64(v1, 28, 17);
    ae_e2 v2 = t30;
    if (v2.tag == AE_E2_0) {
    int64_t v3 = v2.u.v0.p0;
    ae_string t31 = ae_str_from("exact ", 30, 37);
    ae_string t32 = ae_to_string(v3, 30, 48);
    ae_string t33 = ae_str_cat(t31, t32, 30, 46);
    ae_strview t34 = AE_VIEW(t33); if (fwrite(t34.ptr, 1, (size_t)t34.len, stdout) != (size_t)t34.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t33);
    } else if (v2.tag == AE_E2_1) {
    ae_strview v4 = AE_VIEW(v2.u.v1.p0);
    ae_string t35 = ae_str_from("exact failed: ", 31, 43);
    ae_string t36 = ae_str_cat(t35, ae_str_copy_view(AE_VIEW(v4), 31, 60), 31, 60);
    ae_strview t37 = AE_VIEW(t36); if (fwrite(t37.ptr, 1, (size_t)t37.len, stdout) != (size_t)t37.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t36);
    }
    double t38 = v1 + 0.5;
    ae_e2 t39 = ae_f64_to_i64(t38, 33, 23);
    ae_e2 v5 = t39;
    if (v5.tag == AE_E2_0) {
    int64_t v6 = v5.u.v0.p0;
    ae_string t40 = ae_str_from("exact ", 35, 37);
    ae_string t41 = ae_to_string(v6, 35, 48);
    ae_string t42 = ae_str_cat(t40, t41, 35, 46);
    ae_strview t43 = AE_VIEW(t42); if (fwrite(t43.ptr, 1, (size_t)t43.len, stdout) != (size_t)t43.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t42);
    } else if (v5.tag == AE_E2_1) {
    ae_strview v7 = AE_VIEW(v5.u.v1.p0);
    ae_string t44 = ae_str_from("exact failed: ", 36, 43);
    ae_string t45 = ae_str_cat(t44, ae_str_copy_view(AE_VIEW(v7), 36, 60), 36, 60);
    ae_strview t46 = AE_VIEW(t45); if (fwrite(t46.ptr, 1, (size_t)t46.len, stdout) != (size_t)t46.len || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
    ae_str_release(&t45);
    }
    ae_vec_release(&v0);
    ae_e2_release(&v2);
    ae_e2_release(&v5);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
