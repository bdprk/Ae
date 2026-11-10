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
_Noreturn static void ae_exit(int64_t code) {
    fflush(stdout);
    exit((int)code);
}
_Noreturn static void ae_fail(const char *message, size_t line, size_t column) { fprintf(stderr, "ae:%zu:%zu: %s\n", line, column, message); exit(1); }

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
static void *ae_os_alloc(size_t size) { return malloc(size); }
static void *ae_os_resize(void *block, size_t size) { return realloc(block, size); }
static void ae_os_release(void *block) { free(block); }
static ae_allocator ae_allocator_impl = { ae_os_alloc, ae_os_resize, ae_os_release };
// `AE_ALLOC_LIMIT` caps the cumulative number of bytes handed out, which is
// how the test suite reaches the out-of-memory path on purpose. It is a
// diagnostic, not a policy knob: the default is unlimited.
static uint64_t ae_alloc_budget = UINT64_MAX;
static uint64_t ae_alloc_total = 0;
static bool ae_alloc_ready = false;
static void ae_alloc_env(void) {
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
static void *ae_alloc_try(size_t size) {
    if (!ae_alloc_ready) ae_alloc_env();
    if (size > ae_alloc_budget - ae_alloc_total) return NULL;
    void *block = ae_allocator_impl.alloc(size);
    if (!block) return NULL;
    ae_alloc_total += (uint64_t)size;
    return block;
}
static void *ae_alloc(size_t size, size_t line, size_t column) {
    void *block = ae_alloc_try(size);
    if (!block) ae_fail("out of memory", line, column);
    return block;
}
static void *ae_alloc_resize(void *block, size_t size, size_t line, size_t column) {
    if (!ae_alloc_ready) ae_alloc_env();
    // The budget is charged for the whole new size: the accounting is a coarse
    // "how much did this program ask for" measure, not a live byte count.
    if (size > ae_alloc_budget - ae_alloc_total) { ae_allocator_impl.release(block); ae_fail("out of memory", line, column); }
    void *resized = ae_allocator_impl.resize(block, size);
    if (!resized) ae_fail("out of memory", line, column);
    ae_alloc_total += (uint64_t)size;
    return resized;
}
static void ae_free(void *block) { if (block) ae_allocator_impl.release(block); }
static void ae_set_allocator(ae_allocator impl) { ae_allocator_impl = impl; }

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
    char *copy = ae_alloc(n + 1, line, column);
    memcpy(copy, text, n + 1);
    ae_string s = { copy, (int64_t)n };
    return s;
}
static ae_string ae_str_cat(ae_string a, ae_string b, size_t line, size_t column) {
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
static int64_t ae_int_to_buf(int64_t value, char *buffer, size_t line, size_t column) {
    const int length = snprintf(buffer, 24, "%" PRId64, value);
    if (length < 0 || length >= 24) ae_fail("cannot format integer", line, column);
    return length;
}
// `"lit" + to_string(n)` with the digits formatted on the stack: the result
// owns one buffer, and neither the literal nor the digits touch the allocator.
static ae_string ae_str_cat_lit_buf(const char *lit, const char *digits, int64_t len, size_t line, size_t column) {
    const size_t n = strlen(lit);
    if (len > INT64_MAX - (int64_t)n) ae_fail("string too long", line, column);
    char *out = ae_alloc(n + (size_t)len + 1, line, column);
    memcpy(out, lit, n);
    memcpy(out + n, digits, (size_t)len);
    out[n + len] = 0;
    ae_string s = { out, (int64_t)n + len };
    return s;
}
// `to_string(n) + "lit"`: the digits go in front, same zero-alloc rule.
static ae_string ae_str_cat_buf_lit(const char *digits, int64_t len, const char *lit, size_t line, size_t column) {
    const size_t n = strlen(lit);
    if (len > INT64_MAX - (int64_t)n) ae_fail("string too long", line, column);
    char *out = ae_alloc((size_t)len + n + 1, line, column);
    memcpy(out, digits, (size_t)len);
    memcpy(out + len, lit, n + 1);
    ae_string s = { out, len + (int64_t)n };
    return s;
}
static ae_string ae_str_cat_lit_left(const char *lit, ae_string b, size_t line, size_t column) {
    const size_t n = strlen(lit);
    if (b.len > (int64_t)(INT64_MAX - (int64_t)n)) { ae_free(b.ptr); ae_fail("string too long", line, column); }
    char *out = ae_alloc(n + (size_t)b.len + 1, line, column);
    memcpy(out, lit, n);
    memcpy(out + n, b.ptr, (size_t)b.len + 1);
    ae_free(b.ptr);
    ae_string s = { out, (int64_t)n + b.len };
    return s;
}
static ae_string ae_str_cat_lit_right(ae_string a, const char *lit, size_t line, size_t column) {
    const size_t n = strlen(lit);
    if ((int64_t)n > INT64_MAX - a.len) { ae_free(a.ptr); ae_fail("string too long", line, column); }
    char *out = ae_alloc((size_t)a.len + n + 1, line, column);
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, lit, n + 1);
    ae_free(a.ptr);
    ae_string s = { out, a.len + (int64_t)n };
    return s;
}
static void ae_str_release(ae_string *slot) { ae_free(slot->ptr); slot->ptr = NULL; slot->len = 0; }
static void ae_str_forfeit(ae_string *slot) { slot->ptr = NULL; slot->len = 0; }
static bool ae_str_eq(ae_strview a, ae_strview b) {
    return a.len == b.len && (a.len == 0 || memcmp(a.ptr, b.ptr, (size_t)a.len) == 0);
}
static int64_t ae_str_len(ae_strview s, size_t line, size_t column) {
    (void)line; (void)column;
    return s.len;
}
// UTF-8 (ADR-17): len/slice count bytes, but a slice may not end in the middle
// of a character. Every byte of a multi-byte sequence except the first has the
// pattern 10xxxxxx, so checking that one byte is O(1) and cannot be fooled by
// the slice length. Strings that already contain malformed bytes are left
// alone: the rule forbids introducing broken sequences, it does not validate
// input (read_file and argv are byte streams, and forcing a validation pass
// would make every load O(n) for a guarantee callers rarely want).
static bool ae_utf8_continue(unsigned char byte) { return (byte & 0xC0) == 0x80; }
static void ae_utf8_check(ae_strview s, int64_t start, int64_t end, size_t line, size_t column) {
    if ((start > 0 && start < s.len && ae_utf8_continue((unsigned char)s.ptr[start]))
        || (end > 0 && end < s.len && ae_utf8_continue((unsigned char)s.ptr[end]))) {
        ae_fail("string index splits a UTF-8 character", line, column);
    }
}
static ae_strview ae_slice_view(ae_strview s, int64_t start, int64_t end, size_t line, size_t column) {
    if (start < 0 || end < start || end > s.len) ae_fail("string index out of bounds", line, column);
    ae_utf8_check(s, start, end, line, column);
    ae_strview r = { s.ptr + start, end - start };
    return r;
}
static ae_string ae_slice(ae_strview s, int64_t start, int64_t end, size_t line, size_t column) {
    if (start < 0 || end < start || end > s.len) ae_fail("string index out of bounds", line, column);
    ae_utf8_check(s, start, end, line, column);
    char *out = ae_alloc((size_t)(end - start) + 1, line, column);
    memcpy(out, s.ptr + start, (size_t)(end - start));
    out[end - start] = 0;
    ae_string r = { out, end - start };
    return r;
}
// ASCII whitespace, matching what a text file usually contains. Trimming is
// deliberately byte-oriented: it never removes part of a multi-byte character
// (no UTF-8 continuation byte is whitespace).
static bool ae_is_space(unsigned char byte) {
    return byte == ' ' || byte == '\t' || byte == '\n' || byte == '\r' || byte == '\v' || byte == '\f';
}
// Prefix/suffix tests compare bytes, so a suffix check like ends_with(name,
// ".txt") cannot be fooled by a name such as "a.txt.bak" the way contains() can.
static bool ae_starts_with(ae_strview s, ae_strview prefix) {
    return prefix.len <= s.len && (prefix.len == 0 || memcmp(s.ptr, prefix.ptr, (size_t)prefix.len) == 0);
}
static bool ae_ends_with(ae_strview s, ae_strview suffix) {
    return suffix.len <= s.len && (suffix.len == 0 || memcmp(s.ptr + (s.len - suffix.len), suffix.ptr, (size_t)suffix.len) == 0);
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
    char *copy = ae_alloc((size_t)n + 1, line, column);
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
// Number of characters, i.e. number of bytes that are not continuations. For
// well-formed text that is the codepoint count; for stray continuation bytes
// the count is still monotone and never exceeds the byte length.
static int64_t ae_utf8_len(ae_strview s, size_t line, size_t column) {
    (void)line; (void)column;
    int64_t count = 0;
    for (int64_t i = 0; i < s.len; i++) {
        if (!ae_utf8_continue((unsigned char)s.ptr[i])) count = count + 1;
    }
    return count;
}
// First `n` characters as an owned string: the character-safe way to truncate
// text without computing byte offsets by hand. `n` beyond the end copies
// everything, so a caller can clamp with utf8_len or just ask for a big number.
static ae_string ae_utf8_prefix(ae_strview s, int64_t n, size_t line, size_t column) {
    if (n < 0) ae_fail("character index out of bounds", line, column);
    int64_t at = 0;
    int64_t seen = 0;
    while (at < s.len && seen < n) {
        at = at + 1;
        while (at < s.len && ae_utf8_continue((unsigned char)s.ptr[at])) at = at + 1;
        seen = seen + 1;
    }
    return ae_str_from_len(s.ptr, at, line, column);
}
// Leading and trailing whitespace removed, as a fresh owned string. The result
// is always a valid slice of the input, so it cannot split a character.
static ae_string ae_trim(ae_strview s, size_t line, size_t column) {
    int64_t start = 0;
    int64_t end = s.len;
    while (start < end && ae_is_space((unsigned char)s.ptr[start])) start = start + 1;
    while (end > start && ae_is_space((unsigned char)s.ptr[end - 1])) end = end - 1;
    return ae_str_from_len(s.ptr + start, end - start, line, column);
}

// Owned vector of owned strings. Elements are borrow-only reads.
typedef struct { ae_string *data; int64_t len; int64_t cap; } ae_strvec;
static ae_strvec ae_strvec_new(size_t line, size_t column) {
    ae_strvec v;
    v.len = 0;
    v.cap = 4;
    v.data = ae_alloc((size_t)v.cap * sizeof(ae_string), line, column);
    return v;
}
static void ae_strvec_push(ae_strvec *v, ae_string s, size_t line, size_t column) {
    if (v->len == v->cap) {
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap * 2;
        ae_string *nd = ae_alloc_resize(v->data, (size_t)ncap * sizeof(ae_string), line, column);
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
    ae_free(v->data);
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
    v.data = ae_alloc((size_t)v.cap * sizeof(int64_t), line, column);
    return v;
}
static void ae_vec_push(ae_vec *v, int64_t x, size_t line, size_t column) {
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
    ae_free(v->data);
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

// In-place orderings for i64 vectors. qsort is the C library's introsort and
// the comparator is a plain function, so no allocation and no closure state is
// involved; vectors are at most INT64_MAX/8 elements, which qsort accepts as a
// size_t count on both platforms.
static int ae_cmp_i64(const void *left, const void *right) {
    const int64_t a = *(const int64_t *)left;
    const int64_t b = *(const int64_t *)right;
    return (a > b) - (a < b);
}
static void ae_vec_sort(ae_vec *v) {
    if (v->len > 1) qsort(v->data, (size_t)v->len, sizeof(int64_t), ae_cmp_i64);
}
static void ae_vec_reverse(ae_vec *v) {
    for (int64_t i = 0, j = v->len - 1; i < j; i = i + 1, j = j - 1) {
        const int64_t swap = v->data[i];
        v->data[i] = v->data[j];
        v->data[j] = swap;
    }
}

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
#include <windows.h>
// UTF-8 to UTF-16 for the wide-character Win32 APIs; callers free the result.
static wchar_t *ae_widen(const char *text) { /* win32-header */
    const int len = MultiByteToWideChar(CP_UTF8, 0, text, -1, NULL, 0);
    if (len <= 0) return NULL;
    wchar_t *wide = ae_alloc_try((size_t)len * sizeof(wchar_t));
    if (!wide) return NULL;
    MultiByteToWideChar(CP_UTF8, 0, text, -1, wide, len);
    return wide;
}
// UTF-16 to a fresh UTF-8 buffer of `len` units; returns NULL on failure.
static char *ae_narrow(const wchar_t *text, int len) { /* win32-header */
    if (len <= 0) return NULL;
    const int size = WideCharToMultiByte(CP_UTF8, 0, text, len, NULL, 0, NULL, NULL);
    if (size <= 0) return NULL;
    char *narrow = ae_alloc_try((size_t)size + 1);
    if (!narrow) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, text, len, narrow, size, NULL, NULL);
    narrow[size] = 0;
    return narrow;
}
static FILE *ae_fopen(const char *path, const char *mode) { /* win32-header */
    wchar_t *wpath = ae_widen(path);
    wchar_t *wmode = ae_widen(mode);
    if (!wpath || !wmode) { ae_free(wpath); ae_free(wmode); return NULL; }
    FILE *f = _wfopen(wpath, wmode);
    ae_free(wpath);
    ae_free(wmode);
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
    size_t wanted = (size_t)size + 1;
    char *buffer = ae_alloc(wanted, line, column);
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
// `map_inc(m, key, delta)`: the canonical counting operation. One hash
// locates the slot, the delta is added in place, and the new value comes
// back; a missing key starts from zero, which is what every histogram wants
// (Go's `m[k] += d`, Rust's `entry(k).or_insert(0) += d`). Unlike
// ae_map_get there is no error value: absence is part of the semantics.
// An existing key is never copied; only a first sighting allocates.
static int64_t ae_map_inc(ae_map *m, ae_strview key, int64_t delta, size_t line, size_t column) {
    ae_map_grow(m, line, column);
    int64_t at = ae_map_slot_of(key, m->cap);
    int64_t reuse = -1;
    for (;;) {
        ae_map_slot *slot = &m->slots[at];
        if (slot->state == AE_MAP_EMPTY) break;
        if (slot->state == AE_MAP_DELETED) {
            if (reuse < 0) reuse = at;
        } else if (ae_str_eq(AE_VIEW(slot->key), key)) {
            slot->value = slot->value + delta;
            return slot->value;
        }
        at = (at + 1) & (m->cap - 1);
    }
    if (reuse >= 0) at = reuse;
    else m->used = m->used + 1;
    ae_map_slot *slot = &m->slots[at];
    slot->key = ae_str_from_len(key.ptr, key.len, line, column);
    slot->value = delta;
    slot->state = AE_MAP_FULL;
    m->len = m->len + 1;
    return delta;
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
    ae_free(m->slots);
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
