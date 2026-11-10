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
#include <windows.h>
// UTF-8 to UTF-16 for the wide-character Win32 APIs; callers free the result.
static wchar_t *ae_widen(const char *text) {
    const int len = MultiByteToWideChar(CP_UTF8, 0, text, -1, NULL, 0);
    if (len <= 0) return NULL;
    wchar_t *wide = ae_alloc_try((size_t)len * sizeof(wchar_t));
    if (!wide) return NULL;
    MultiByteToWideChar(CP_UTF8, 0, text, -1, wide, len);
    return wide;
}
// UTF-16 to a fresh UTF-8 buffer of `len` units; returns NULL on failure.
static char *ae_narrow(const wchar_t *text, int len) {
    if (len <= 0) return NULL;
    const int size = WideCharToMultiByte(CP_UTF8, 0, text, len, NULL, 0, NULL, NULL);
    if (size <= 0) return NULL;
    char *narrow = ae_alloc_try((size_t)size + 1);
    if (!narrow) return NULL;
    WideCharToMultiByte(CP_UTF8, 0, text, len, narrow, size, NULL, NULL);
    narrow[size] = 0;
    return narrow;
}
static FILE *ae_fopen(const char *path, const char *mode) {
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
static void ae_e0_release(ae_e0 *v) {
    if (v->tag == AE_E0_0) {
        ae_str_release(&v->u.v0.p0);
    }
    if (v->tag == AE_E0_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e0_forfeit(ae_e0 *v) {
    if (v->tag == AE_E0_0) {
        ae_str_forfeit(&v->u.v0.p0);
    }
    if (v->tag == AE_E0_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e1_release(ae_e1 *v) {
    if (v->tag == AE_E1_0) {
        v->u.v0.p0 = 0;
    }
    if (v->tag == AE_E1_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e1_forfeit(ae_e1 *v) {
    if (v->tag == AE_E1_0) {
        v->u.v0.p0 = 0;
    }
    if (v->tag == AE_E1_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e2_release(ae_e2 *v) {
    if (v->tag == AE_E2_0) {
        v->u.v0.p0 = 0;
    }
    if (v->tag == AE_E2_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e2_forfeit(ae_e2 *v) {
    if (v->tag == AE_E2_0) {
        v->u.v0.p0 = 0;
    }
    if (v->tag == AE_E2_1) {
        ae_str_forfeit(&v->u.v1.p0);
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
static void ae_e3_release(ae_e3 *v) {
    if (v->tag == AE_E3_0) {
        ae_strvec_release(&v->u.v0.p0);
    }
    if (v->tag == AE_E3_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e3_forfeit(ae_e3 *v) {
    if (v->tag == AE_E3_0) {
        ae_strvec_forfeit(&v->u.v0.p0);
    }
    if (v->tag == AE_E3_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e4_release(ae_e4 *v) {
    if (v->tag == AE_E4_0) {
        v->u.v0.p0 = 0;
        v->u.v0.p1 = 0;
        v->u.v0.p2 = false;
    }
    if (v->tag == AE_E4_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e4_forfeit(ae_e4 *v) {
    if (v->tag == AE_E4_0) {
        v->u.v0.p0 = 0;
        v->u.v0.p1 = 0;
        v->u.v0.p2 = false;
    }
    if (v->tag == AE_E4_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e5_release(ae_e5 *v) {
    if (v->tag == AE_E5_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e5_forfeit(ae_e5 *v) {
    if (v->tag == AE_E5_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e6_release(ae_e6 *v) {
    if (v->tag == AE_E6_0) {
        v->u.v0.p0 = false;
    }
    if (v->tag == AE_E6_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e6_forfeit(ae_e6 *v) {
    if (v->tag == AE_E6_0) {
        v->u.v0.p0 = false;
    }
    if (v->tag == AE_E6_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}

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
static void ae_e7_release(ae_e7 *v) {
    if (v->tag == AE_E7_0) {
        v->u.v0.p0 = 0;
        ae_str_release(&v->u.v0.p1);
    }
    if (v->tag == AE_E7_1) {
        ae_str_release(&v->u.v1.p0);
    }
}
static void ae_e7_forfeit(ae_e7 *v) {
    if (v->tag == AE_E7_0) {
        v->u.v0.p0 = 0;
        ae_str_forfeit(&v->u.v0.p1);
    }
    if (v->tag == AE_E7_1) {
        ae_str_forfeit(&v->u.v1.p0);
    }
}
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

static AE_STATUS_RESULT ae_status_ok(void) {
    AE_STATUS_RESULT out = { AE_STATUS_OK, { { 0 } } };
    return out;
}
static AE_STATUS_RESULT ae_status_failed(ae_string message) {
    AE_STATUS_RESULT out = { AE_STATUS_ERR, { .v1 = { message } } };
    return out;
}

// Wall-clock time in milliseconds since the Unix epoch. File metadata reports
// whole seconds, so now_s exists to compare against it without dividing on
// every call. Both are i64: the epoch range fits comfortably, and negative
// values (before 1970) stay meaningful.
static int64_t ae_now_ms(void) {
#ifdef _WIN32
    FILETIME ft;
    GetSystemTimeAsFileTime(&ft);
    const int64_t ticks = ((int64_t)ft.dwHighDateTime << 32) | (int64_t)ft.dwLowDateTime;
    // 100-nanosecond ticks since 1601 -> milliseconds since 1970.
    return ticks / 10000 - 11644473600000LL;
#else
    // timespec_get is ISO C11, so the generated file still compiles under
    // -std=c11 -pedantic-errors without defining a feature-test macro;
    // clock_gettime would need _POSIX_C_SOURCE and is hidden by strict mode.
    struct timespec ts;
    if (timespec_get(&ts, TIME_UTC) != TIME_UTC) return 0;
    return (int64_t)ts.tv_sec * 1000 + (int64_t)(ts.tv_nsec / 1000000);
#endif
}
static int64_t ae_now_s(void) {
    const int64_t ms = ae_now_ms();
    // Floor division, so a negative timestamp still rounds consistently.
    return ms / 1000 - (ms % 1000 < 0 ? 1 : 0);
}

static ae_string ae_io_message(int code, size_t line, size_t column) {
    const char *text = strerror(code == 0 ? EIO : code);
    return ae_str_from(text ? text : "I/O error", line, column);
}
// Error text carrying a platform status code where strerror does not apply.
static ae_string ae_status_message(const char *what, unsigned long code, size_t line, size_t column) {
    char buffer[160];
    const int length = snprintf(buffer, sizeof buffer, "%s (status %lu)", what, code);
    if (length < 0) return ae_str_from(what, line, column);
    return ae_str_from_len(buffer, (int64_t)(length < (int)sizeof buffer ? length : (int)sizeof buffer - 1), line, column);
}

static AE_READ_RESULT ae_try_read_file(const char *path, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "rb");
    if (!f) { AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(errno, line, column) } } }; return out; }
    ae_string failure = { NULL, 0 };
    if (fseek(f, 0, SEEK_END) != 0) failure = ae_io_message(errno, line, column);
    long size = failure.ptr ? -1 : ftell(f);
    if (!failure.ptr && size < 0) failure = ae_io_message(errno, line, column);
    if (!failure.ptr) rewind(f);
    char *buffer = NULL;
    if (!failure.ptr) {
        buffer = ae_alloc_try((size_t)size + 1);
        if (!buffer) failure = ae_str_from("out of memory", line, column);
    }
    if (!failure.ptr) {
        const size_t read = fread(buffer, 1, (size_t)size, f);
        if (read != (size_t)size) { ae_free(buffer); buffer = NULL; failure = ae_io_message(errno, line, column); }
    }
    fclose(f);
    if (failure.ptr) { AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { failure } } }; return out; }
    buffer[size] = 0;
    ae_string content = { buffer, (int64_t)size };
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { content } } };
    return out;
}

static AE_WRITE_RESULT ae_try_write_file(const char *path, ae_strview content, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "wb");
    if (!f) { AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } }; return out; }
    const size_t written = fwrite(content.ptr, 1, (size_t)content.len, f);
    const bool closed = fclose(f) == 0;
    if (!closed || written != (size_t)content.len) {
        AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } };
        return out;
    }
    AE_WRITE_RESULT out = { AE_WRITE_OK, { .v0 = { (int64_t)written } } };
    return out;
}

// Appending exists as its own operation because "read, concatenate, write" is
// not the same thing: it rewrites the whole file, loses the atomicity of a
// single append, and fails on files the process may only append to.
static AE_WRITE_RESULT ae_try_append_file(const char *path, ae_strview content, size_t line, size_t column) {
    errno = 0;
    FILE *f = ae_fopen(path, "ab");
    if (!f) { AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } }; return out; }
    const size_t written = fwrite(content.ptr, 1, (size_t)content.len, f);
    const bool closed = fclose(f) == 0;
    if (!closed || written != (size_t)content.len) {
        AE_WRITE_RESULT out = { AE_WRITE_ERR, { .v1 = { ae_io_message(errno, line, column) } } };
        return out;
    }
    AE_WRITE_RESULT out = { AE_WRITE_OK, { .v0 = { (int64_t)written } } };
    return out;
}

// Environment lookup. A variable that is set but empty is Ok(""), a variable
// that does not exist is Err: collapsing the two would silently turn "PATH is
// empty" into "PATH is missing".
static AE_READ_RESULT ae_try_env(const char *name, size_t line, size_t column) {
    if (name[0] == 0) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable name is empty", line, column) } } };
        return out;
    }
#ifdef _WIN32
    wchar_t *wname = ae_widen(name);
    if (!wname) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    // A variable whose value is empty also reports 0, so the last error is
    // cleared first: ERROR_SUCCESS afterwards means "set but empty", and only
    // ERROR_ENVVAR_NOT_FOUND means the name is absent.
    SetLastError(ERROR_SUCCESS);
    const DWORD needed = GetEnvironmentVariableW(wname, NULL, 0);
    if (needed == 0) {
        const DWORD code = GetLastError();
        ae_free(wname);
        if (code == ERROR_SUCCESS) {
            AE_READ_RESULT empty = { AE_READ_OK, { .v0 = { ae_str_from("", line, column) } } };
            return empty;
        }
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { code == ERROR_ENVVAR_NOT_FOUND ? ae_str_from("variable not set", line, column) : ae_status_message("cannot read variable", code, line, column) } } };
        return out;
    }
    wchar_t *buffer = ae_alloc_try((size_t)needed * sizeof(wchar_t));
    if (!buffer) {
        ae_free(wname);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    SetLastError(ERROR_SUCCESS);
    const DWORD length = GetEnvironmentVariableW(wname, buffer, needed);
    const DWORD read_code = GetLastError();
    ae_free(wname);
    if (length >= needed) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable changed while reading", line, column) } } };
        return out;
    }
    if (length == 0 && read_code != ERROR_SUCCESS) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read variable", read_code, line, column) } } };
        return out;
    }
    // An empty value is a successful read of zero characters: narrow() rejects
    // a zero length, so build the empty string directly.
    ae_string value = length == 0 ? ae_str_from("", line, column) : (ae_string){ NULL, 0 };
    if (length > 0) {
        char *utf8 = ae_narrow(buffer, (int)length);
        if (!utf8) {
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        value = ae_str_from(utf8, line, column);
        ae_free(utf8);
    }
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { value } } };
    return out;
#else
    (void)line; (void)column;
    const char *value = getenv(name);
    if (!value) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("variable not set", line, column) } } };
        return out;
    }
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { ae_str_from(value, line, column) } } };
    return out;
#endif
}

// Everything on standard input, read as bytes. Windows starts in text mode,
// which would turn CRLF into LF and make the byte count differ between
// platforms, so the descriptor is switched to binary first. The buffer grows by
// doubling; a fully read pipe returns however much arrived, which is not an
// error -- "no input" and "failed to read input" must stay distinguishable.
static AE_READ_RESULT ae_try_read_stdin(size_t line, size_t column) {
#ifdef _WIN32
    _setmode(_fileno(stdin), _O_BINARY);
#endif
    size_t capacity = 8192;
    size_t length = 0;
    char *buffer = ae_alloc_try(capacity);
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    errno = 0;
    for (;;) {
        if (length == capacity) {
            if (capacity > ((size_t)-1) / 2) {
                ae_free(buffer);
                AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("input too large", line, column) } } };
                return out;
            }
            char *grown = ae_alloc_try(capacity * 2);
            if (!grown) {
                ae_free(buffer);
                AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
                return out;
            }
            memcpy(grown, buffer, length);
            ae_free(buffer);
            buffer = grown;
            capacity = capacity * 2;
        }
        const size_t got = fread(buffer + length, 1, capacity - length, stdin);
        length = length + got;
        if (got == 0) break;
    }
    if (ferror(stdin)) {
        const int code = errno;
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(code, line, column) } } };
        return out;
    }
    ae_string text = ae_str_from_len(buffer, (int64_t)length, line, column);
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { text } } };
    return out;
}

// Current working directory, in the same `/`-separated form the rest of the
// language uses for paths.
static AE_READ_RESULT ae_try_cwd(size_t line, size_t column) {
#ifdef _WIN32
    const DWORD needed = GetCurrentDirectoryW(0, NULL);
    if (needed == 0) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read working directory", (unsigned long)GetLastError(), line, column) } } };
        return out;
    }
    wchar_t *buffer = ae_alloc_try((size_t)needed * sizeof(wchar_t));
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    const DWORD length = GetCurrentDirectoryW(needed, buffer);
    if (length == 0 || length >= needed) {
        ae_free(buffer);
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_status_message("cannot read working directory", (unsigned long)GetLastError(), line, column) } } };
        return out;
    }
    char *utf8 = ae_narrow(buffer, (int)length);
    ae_free(buffer);
    if (!utf8) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    ae_string raw = ae_str_from(utf8, line, column);
    ae_free(utf8);
    // Unify separators so path_join and friends see one form.
    for (int64_t i = 0; i < raw.len; i++) if (raw.ptr[i] == '\\') raw.ptr[i] = '/';
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { raw } } };
    return out;
#else
    size_t capacity = 256;
    char *buffer = ae_alloc_try(capacity);
    if (!buffer) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    while (!getcwd(buffer, capacity)) {
        if (errno != ERANGE || capacity > ((size_t)-1) / 2) {
            const int code = errno;
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_io_message(code, line, column) } } };
            return out;
        }
        char *grown = ae_alloc_try(capacity * 2);
        if (!grown) {
            ae_free(buffer);
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        ae_free(buffer);
        buffer = grown;
        capacity = capacity * 2;
    }
    ae_string text = ae_str_from(buffer, line, column);
    ae_free(buffer);
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { text } } };
    return out;
#endif
}

// Byte offset of the first match, or an Err value when there is none. The
// views carry explicit lengths, so embedded NUL bytes are searched as well —
// strstr() would stop at the first one. memchr() is the platform's optimized
// byte scan; the success path allocates nothing, and the failure path only
// allocates the error string the result enum already owns.
static AE_INT_RESULT ae_find(ae_strview haystack, ae_strview needle, size_t line, size_t column) {
    if (needle.len <= 0) {
        AE_INT_RESULT out = { AE_INT_OK, { .v0 = { 0 } } };
        return out;
    }
    const int64_t last = haystack.len - needle.len;
    for (int64_t at = 0; at <= last; ) {
        const char *hit = memchr(haystack.ptr + at, needle.ptr[0], (size_t)(last - at + 1));
        if (!hit) break;
        at = (int64_t)(hit - haystack.ptr);
        if (needle.len == 1 || memcmp(hit, needle.ptr, (size_t)needle.len) == 0) {
            AE_INT_RESULT out = { AE_INT_OK, { .v0 = { at } } };
            return out;
        }
        at++;
    }
    AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("substring not found", line, column) } } };
    return bad;
}

// Strict integer parsing: optional sign, ASCII digits only, whole string, and
// no wrap-around. Anything else is an error value, never a silent default.
static AE_INT_RESULT ae_parse_int(ae_strview text, size_t line, size_t column) {
    int64_t at = 0;
    bool negative = false;
    if (text.len > 0 && (text.ptr[0] == '-' || text.ptr[0] == '+')) { negative = text.ptr[0] == '-'; at = 1; }
    if (at >= text.len) {
        AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("empty or sign-only integer", line, column) } } };
        return bad;
    }
    uint64_t magnitude = 0;
    const uint64_t limit = negative ? (uint64_t)INT64_MAX + 1u : (uint64_t)INT64_MAX;
    for (; at < text.len; at++) {
        const char c = text.ptr[at];
        if (c < '0' || c > '9') {
            AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("not a base-10 integer", line, column) } } };
            return bad;
        }
        const uint64_t digit = (uint64_t)(c - '0');
        if (magnitude > (limit - digit) / 10u) {
            AE_INT_RESULT bad = { AE_INT_ERR, { .v1 = { ae_str_from("integer out of i64 range", line, column) } } };
            return bad;
        }
        magnitude = magnitude * 10u + digit;
    }
    const int64_t value = negative ? (int64_t)(0u - magnitude) : (int64_t)magnitude;
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { value } } };
    return out;
}

static ae_string ae_to_string(int64_t value, size_t line, size_t column) {
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
static size_t ae_f64_fixed(double value, char *out, size_t cap) {
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

static size_t ae_f64_scientific(double value, char *out, size_t cap) {
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
static size_t ae_f64_exponent_form(char *text, size_t length) {
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

static size_t ae_f64_text(double value, char *out, size_t cap) {
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

static ae_string ae_f64_to_string(double value, size_t line, size_t column) {
    char buffer[96];
    const size_t length = ae_f64_text(value, buffer, sizeof buffer);
    return ae_str_from_len(buffer, (int64_t)length, line, column);
}

static void ae_f64_print(double value) {
    char buffer[96];
    const size_t length = ae_f64_text(value, buffer, sizeof buffer);
    if (fwrite(buffer, 1, length, stdout) != length || fputc('\n', stdout) == EOF) ae_fail("output error", 0, 0);
}

static double ae_f64_div(double a, double b, size_t line, size_t column) {
    // Same rule as integer division: zero is a reported failure, never a
    // silent infinity. Non-finite results from overflow are still IEEE values.
    if (b == 0.0) ae_fail("f64 division by zero", line, column);
    return a / b;
}

static double ae_f64_from_i64(int64_t value, size_t line, size_t column) {
    // Every integer up to 2^53 is exact; beyond that the conversion would
    // round silently, so it is refused instead of approximated.
    if (value > INT64_C(9007199254740992) || value < -INT64_C(9007199254740992)) {
        ae_fail("i64 to f64 conversion would lose precision", line, column);
    }
    return (double)value;
}

static AE_INT_RESULT ae_f64_to_i64(double value, size_t line, size_t column) {
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

static bool ae_path_exists(const char *path, size_t line, size_t column) {
    (void)line; (void)column;
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return false;
    const DWORD attributes = GetFileAttributesW(wide);
    ae_free(wide);
    return attributes != INVALID_FILE_ATTRIBUTES;
#else
    struct stat info;
    return stat(path, &info) == 0;
#endif
}

static bool ae_path_is_dir(const char *path, size_t line, size_t column) {
    (void)line; (void)column;
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return false;
    const DWORD attributes = GetFileAttributesW(wide);
    ae_free(wide);
    return attributes != INVALID_FILE_ATTRIBUTES && (attributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
#else
    struct stat info;
    return stat(path, &info) == 0 && S_ISDIR(info.st_mode);
#endif
}

// Directory listing: entry names only, never "." or "..", sorted by bytes so
// repeated runs and the two platforms agree on order.
static int ae_name_compare(const void *left, const void *right) {
    const ae_string *a = left;
    const ae_string *b = right;
    const int64_t shared = a->len < b->len ? a->len : b->len;
    const int order = shared > 0 ? memcmp(a->ptr, b->ptr, (size_t)shared) : 0;
    if (order != 0) return order;
    return a->len < b->len ? -1 : (a->len > b->len ? 1 : 0);
}
// In-place orderings for owned string vectors, byte-order like keys() and
// directory listings so every sorted output is comparable across platforms.
static void ae_strvec_sort(ae_strvec *v) {
    if (v->len > 1) qsort(v->data, (size_t)v->len, sizeof(ae_string), ae_name_compare);
}
static void ae_strvec_reverse(ae_strvec *v) {
    for (int64_t i = 0, j = v->len - 1; i < j; i = i + 1, j = j - 1) {
        const ae_string swap = v->data[i];
        v->data[i] = v->data[j];
        v->data[j] = swap;
    }
}
static AE_DIR_RESULT ae_try_list_dir(const char *path, size_t line, size_t column) {
    ae_strvec names = ae_strvec_new(line, column);
#ifdef _WIN32
    const size_t length = strlen(path);
    const bool separated = length > 0 && (path[length - 1] == '\\' || path[length - 1] == '/');
    char *pattern = ae_alloc_try(length + (separated ? 2 : 3));
    if (!pattern) {
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    memcpy(pattern, path, length);
    if (!separated) pattern[length] = '\\';
    pattern[length + (separated ? 0 : 1)] = '*';
    pattern[length + (separated ? 1 : 2)] = 0;
    wchar_t *wide = ae_widen(pattern);
    ae_free(pattern);
    if (!wide) {
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    WIN32_FIND_DATAW data;
    HANDLE walk = FindFirstFileW(wide, &data);
    ae_free(wide);
    if (walk == INVALID_HANDLE_VALUE) {
        ae_string message = ae_status_message("cannot list directory", (unsigned long)GetLastError(), line, column);
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { message } } };
        return out;
    }
    do {
        if (wcscmp(data.cFileName, L".") == 0 || wcscmp(data.cFileName, L"..") == 0) continue;
        char *utf8 = ae_narrow(data.cFileName, (int)wcslen(data.cFileName));
        if (!utf8) {
            FindClose(walk);
            ae_strvec_release(&names);
            AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
            return out;
        }
        ae_strvec_push(&names, ae_str_from(utf8, line, column), line, column);
        ae_free(utf8);
    } while (FindNextFileW(walk, &data));
    FindClose(walk);
#else
    DIR *walk = opendir(path);
    if (!walk) {
        ae_string message = ae_io_message(errno, line, column);
        ae_strvec_release(&names);
        AE_DIR_RESULT out = { AE_DIR_ERR, { .v1 = { message } } };
        return out;
    }
    struct dirent *entry;
    while ((entry = readdir(walk)) != NULL) {
        if (strcmp(entry->d_name, ".") == 0 || strcmp(entry->d_name, "..") == 0) continue;
        ae_strvec_push(&names, ae_str_from(entry->d_name, line, column), line, column);
    }
    closedir(walk);
#endif
    if (names.len > 1) qsort(names.data, (size_t)names.len, sizeof(ae_string), ae_name_compare);
    AE_DIR_RESULT out = { AE_DIR_OK, { .v0 = { names } } };
    return out;
}

// ---- Path algebra -----------------------------------------------------------
// Separators: both '/' and '\' are accepted on input, but '/'-joined output is
// what this layer emits, because Windows accepts '/' and identical output on
// both platforms keeps tests and recorded results comparable.

static bool ae_is_separator(char c) { return c == '/' || c == '\\'; }

// Concatenate `dir` and `name`, inserting a separator only when needed.
static ae_string ae_path_concat(ae_strview dir, ae_strview name, bool separator, size_t line, size_t column) {
    const int64_t total = dir.len + name.len + (separator ? 1 : 0);
    char *buffer = ae_alloc((size_t)total + 1, line, column);
    memcpy(buffer, dir.ptr, (size_t)dir.len);
    int64_t at = dir.len;
    if (separator) buffer[at++] = '/';
    memcpy(buffer + at, name.ptr, (size_t)name.len);
    buffer[total] = 0;
    ae_string out = { buffer, total };
    return out;
}
// An absolute name (leading separator, or a drive letter) replaces the
// directory instead of being appended to it.
static bool ae_path_is_absolute(ae_strview name) {
    if (name.len == 0) return false;
    if (ae_is_separator(name.ptr[0])) return true;
    return name.len >= 2 && name.ptr[1] == ':';
}
static ae_string ae_path_join(ae_strview dir, ae_strview name, size_t line, size_t column) {
    if (name.len == 0) return ae_str_copy_view(dir, line, column);
    if (dir.len == 0) return ae_str_copy_view(name, line, column);
    if (ae_path_is_absolute(name)) return ae_str_copy_view(name, line, column);
    const bool separated = ae_is_separator(dir.ptr[dir.len - 1]);
    return ae_path_concat(dir, name, !separated, line, column);
}
// Final component of a path. Trailing separators are dropped first so
// "dir/name/" yields "name"; a path that is only separators keeps one so the
// root stays recognisable.
static ae_string ae_path_name(ae_strview path, size_t line, size_t column) {
    int64_t end = path.len;
    while (end > 1 && ae_is_separator(path.ptr[end - 1])) end--;
    int64_t start = end;
    while (start > 0 && !ae_is_separator(path.ptr[start - 1])) start--;
    if (start == end) {
        // All separators: the name of the root is the root marker itself.
        ae_strview root = { path.ptr, end };
        return ae_str_copy_view(root, line, column);
    }
    ae_strview tail = { path.ptr + start, end - start };
    return ae_str_copy_view(tail, line, column);
}
// Everything before the final component. A path without a separator has no
// parent, which reports as the empty string rather than a fabricated "." .
static ae_string ae_path_parent(ae_strview path, size_t line, size_t column) {
    int64_t end = path.len;
    while (end > 1 && ae_is_separator(path.ptr[end - 1])) end--;
    int64_t at = end;
    while (at > 0 && !ae_is_separator(path.ptr[at - 1])) at--;
    if (at == 0) return ae_str_from_len("", 0, line, column);
    int64_t stop = at;
    while (stop > 1 && ae_is_separator(path.ptr[stop - 1])) stop--;
#ifdef _WIN32
    // Keep the separator after a drive prefix: "C:/dir" has parent "C:/".
    if (stop == 2 && path.ptr[1] == ':' && at > stop) stop = at;
#endif
    if (stop == 1 && ae_is_separator(path.ptr[0])) stop = 1;
    return ae_str_from_len(path.ptr, stop, line, column);
}

// ---- Metadata ---------------------------------------------------------------
// Size in bytes, modification time in seconds since the Unix epoch, and whether
// the target is a directory. Failure is a value, never a silent zero size.
static AE_FILE_RESULT ae_try_file_info(const char *path, size_t line, size_t column) {
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) {
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_str_from("out of memory", line, column) } } };
        return out;
    }
    WIN32_FILE_ATTRIBUTE_DATA data;
    if (!GetFileAttributesExW(wide, GetFileExInfoStandard, &data)) {
        const unsigned long status = (unsigned long)GetLastError();
        ae_free(wide);
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_status_message("cannot read file metadata", status, line, column) } } };
        return out;
    }
    ae_free(wide);
    const int64_t size = (int64_t)(((uint64_t)data.nFileSizeHigh << 32) | (uint64_t)data.nFileSizeLow);
    // FILETIME counts 100 ns ticks from 1601-01-01; this offset lands on the
    // Unix epoch, so the number matches st_mtime on POSIX.
    const int64_t ticks = (int64_t)(((uint64_t)data.ftLastWriteTime.dwHighDateTime << 32) | (uint64_t)data.ftLastWriteTime.dwLowDateTime);
    const int64_t modified = ticks / 10000000 - 11644473600LL;
    const bool is_dir = (data.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) != 0;
    AE_FILE_RESULT out = { AE_FILE_OK, { .v0 = { size, modified, is_dir } } };
    return out;
#else
    errno = 0;
    struct stat info;
    if (stat(path, &info) != 0) {
        AE_FILE_RESULT out = { AE_FILE_ERR, { .v1 = { ae_io_message(errno, line, column) } } };
        return out;
    }
    AE_FILE_RESULT out = { AE_FILE_OK, { .v0 = { (int64_t)info.st_size, (int64_t)info.st_mtime, S_ISDIR(info.st_mode) } } };
    return out;
#endif
}

// ---- Mutation --------------------------------------------------------------
static AE_STATUS_RESULT ae_try_copy_file(const char *from, const char *to, size_t line, size_t column) {
    errno = 0;
    FILE *source = ae_fopen(from, "rb");
    if (!source) return ae_status_failed(ae_io_message(errno, line, column));
    FILE *target = ae_fopen(to, "wb");
    if (!target) {
        fclose(source);
        return ae_status_failed(ae_io_message(errno, line, column));
    }
    ae_string failure = { NULL, 0 };
    char buffer[65536];
    size_t read;
    while ((read = fread(buffer, 1, sizeof buffer, source)) > 0) {
        if (fwrite(buffer, 1, read, target) != read) { failure = ae_io_message(errno, line, column); break; }
    }
    if (!failure.ptr && ferror(source)) failure = ae_io_message(errno, line, column);
    if (fclose(source) != 0 && !failure.ptr) failure = ae_io_message(errno, line, column);
    if (fclose(target) != 0 && !failure.ptr) failure = ae_io_message(errno, line, column);
    if (failure.ptr) return ae_status_failed(failure);
    return ae_status_ok();
}

static AE_STATUS_RESULT ae_try_rename_path(const char *from, const char *to, size_t line, size_t column) {
#ifdef _WIN32
    errno = 0;
    wchar_t *source = ae_widen(from);
    wchar_t *target = ae_widen(to);
    if (!source || !target) {
        ae_free(source); ae_free(target);
        return ae_status_failed(ae_str_from("out of memory", line, column));
    }
    const bool moved = MoveFileExW(source, target, MOVEFILE_REPLACE_EXISTING | MOVEFILE_COPY_ALLOWED) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(source); ae_free(target);
    if (!moved) return ae_status_failed(ae_status_message("cannot rename path", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (rename(from, to) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// Removes a file. Directories are refused on both platforms: POSIX remove()
// would also delete an empty directory, and reporting the same outcome on two
// platforms matters more than the extra convenience.
static AE_STATUS_RESULT ae_try_remove_file(const char *path, size_t line, size_t column) {
    if (ae_path_is_dir(path, line, column)) {
        return ae_status_failed(ae_str_from("cannot remove a directory with try_remove_file", line, column));
    }
#ifdef _WIN32
    errno = 0;
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_status_failed(ae_str_from("out of memory", line, column));
    const bool removed = DeleteFileW(wide) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (!removed) return ae_status_failed(ae_status_message("cannot remove file", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (remove(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// Returns an empty string on success, or the failure text. Keeping the message
// as the return value lets the caller decide whether a race is acceptable.
static ae_string ae_make_one_dir(const char *path, size_t line, size_t column) {
    ae_string none = { NULL, 0 };
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_str_from("out of memory", line, column);
    if (CreateDirectoryW(wide, NULL)) { ae_free(wide); return none; }
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (status == ERROR_ALREADY_EXISTS) return none;
    return ae_status_message("cannot create directory", status, line, column);
#else
    errno = 0;
    if (mkdir(path, 0777) == 0) return none;
    if (errno == EEXIST) return none;
    return ae_io_message(errno, line, column);
#endif
}
// Creates the directory and every missing parent, so a caller can write
// "out/2026/march/" in one step. Existing directories are not an error.
static AE_STATUS_RESULT ae_try_create_dir(const char *path, size_t line, size_t column) {
    const int64_t length = (int64_t)strlen(path);
    if (length == 0) return ae_status_failed(ae_str_from("empty path", line, column));
    char *buffer = ae_alloc_try((size_t)length + 1);
    if (!buffer) return ae_status_failed(ae_str_from("out of memory", line, column));
    memcpy(buffer, path, (size_t)length + 1);
    ae_string failure = { NULL, 0 };
    for (int64_t at = 1; at <= length; at++) {
        if (at < length && !ae_is_separator(buffer[at])) continue;
        const char saved = buffer[at];
        buffer[at] = 0;
        bool skip = false;
#ifdef _WIN32
        if (at == 2 && buffer[1] == ':') skip = true; // drive prefix
#endif
        if (!skip && !ae_path_is_dir(buffer, line, column)) {
            failure = ae_make_one_dir(buffer, line, column);
        }
        buffer[at] = saved;
        if (failure.ptr) break;
    }
    ae_free(buffer);
    if (failure.ptr) return ae_status_failed(failure);
    return ae_status_ok();
}

// Changes the process working directory. This is a global mutation: every
// later relative path, including a child process's, resolves against the new
// directory. It is a result value for the same reason the rest are - the
// target may be gone or unreadable - and no undo is provided, so callers keep
// the old cwd() if they intend to come back.
static AE_STATUS_RESULT ae_set_cwd(const char *path, size_t line, size_t column) {
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_status_failed(ae_str_from("out of memory", line, column));
    const BOOL ok = SetCurrentDirectoryW(wide);
    const unsigned long status = ok ? 0UL : (unsigned long)GetLastError();
    ae_free(wide);
    if (!ok) return ae_status_failed(ae_status_message("cannot change the working directory", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (chdir(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// Removes an empty directory. Recursive removal is deliberately absent: a
// helper that silently deletes a tree is the easiest way to lose data.
static AE_STATUS_RESULT ae_try_remove_dir(const char *path, size_t line, size_t column) {
#ifdef _WIN32
    wchar_t *wide = ae_widen(path);
    if (!wide) return ae_status_failed(ae_str_from("out of memory", line, column));
    const bool removed = RemoveDirectoryW(wide) != 0;
    const unsigned long status = (unsigned long)GetLastError();
    ae_free(wide);
    if (!removed) return ae_status_failed(ae_status_message("cannot remove directory", status, line, column));
    return ae_status_ok();
#else
    errno = 0;
    if (rmdir(path) != 0) return ae_status_failed(ae_io_message(errno, line, column));
    return ae_status_ok();
#endif
}

// ---- Map helpers that need the prelude result types -------------------------
// A missing key is a value, not a zero: `0` is a perfectly normal count.
static AE_INT_RESULT ae_map_get(const ae_map *m, ae_strview key, size_t line, size_t column) {
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
static ae_strvec ae_map_keys(const ae_map *m, size_t line, size_t column) {
    ae_strvec keys = ae_strvec_new(line, column);
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->slots[i].state != AE_MAP_FULL) continue;
        ae_strvec_push(&keys, ae_str_copy_view(AE_VIEW(m->slots[i].key), line, column), line, column);
    }
    if (keys.len > 1) qsort(keys.data, (size_t)keys.len, sizeof(ae_string), ae_name_compare);
    return keys;
}

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

static void ae_json_ws(const char *t, int64_t n, int64_t *i) {
    while (*i < n) {
        const char c = t[*i];
        if (c != ' ' && c != '\t' && c != '\n' && c != '\r') break;
        *i += 1;
    }
}
static int ae_json_hex(char c) {
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}
static int ae_json_utf8(char *out, unsigned cp) {
    if (cp < 0x80u) { out[0] = (char)cp; return 1; }
    if (cp < 0x800u) { out[0] = (char)(0xC0u | (cp >> 6)); out[1] = (char)(0x80u | (cp & 0x3Fu)); return 2; }
    if (cp < 0x10000u) { out[0] = (char)(0xE0u | (cp >> 12)); out[1] = (char)(0x80u | ((cp >> 6) & 0x3Fu)); out[2] = (char)(0x80u | (cp & 0x3Fu)); return 3; }
    out[0] = (char)(0xF0u | (cp >> 18)); out[1] = (char)(0x80u | ((cp >> 12) & 0x3Fu)); out[2] = (char)(0x80u | ((cp >> 6) & 0x3Fu)); out[3] = (char)(0x80u | (cp & 0x3Fu)); return 4;
}
static int ae_json_skip_string(const char *t, int64_t n, int64_t *i) {
    if (*i >= n || t[*i] != '"') return 0;
    int64_t j = *i + 1;
    while (j < n) {
        const unsigned char c = (unsigned char)t[j];
        if (c == '"') { *i = j + 1; return 1; }
        if (c == '\\') { j += 2; continue; }
        if (c < 0x20u) return 0;
        j += 1;
    }
    return 0;
}
static int ae_json_skip_value(const char *t, int64_t n, int64_t *i, int depth) {
    if (depth > AE_JSON_DEPTH) return 0;
    ae_json_ws(t, n, i);
    if (*i >= n) return 0;
    const char c = t[*i];
    if (c == '"') return ae_json_skip_string(t, n, i);
    if (c == '{' || c == '[') {
        const char close = c == '{' ? '}' : ']';
        *i += 1;
        ae_json_ws(t, n, i);
        if (*i < n && t[*i] == close) { *i += 1; return 1; }
        for (;;) {
            ae_json_ws(t, n, i);
            if (c == '{') {
                if (!ae_json_skip_string(t, n, i)) return 0;
                ae_json_ws(t, n, i);
                if (*i >= n || t[*i] != ':') return 0;
                *i += 1;
            }
            if (!ae_json_skip_value(t, n, i, depth + 1)) return 0;
            ae_json_ws(t, n, i);
            if (*i >= n) return 0;
            if (t[*i] == ',') { *i += 1; continue; }
            if (t[*i] == close) { *i += 1; return 1; }
            return 0;
        }
    }
    // A bare token: number, true, false or null. Its end is punctuation or space.
    const int64_t start = *i;
    while (*i < n) {
        const char d = t[*i];
        if (d == ',' || d == '}' || d == ']' || d == ' ' || d == '\t' || d == '\r' || d == '\n') break;
        *i += 1;
    }
    return *i > start;
}
// Decodes one JSON string at *i into an owned string, resolving escapes (and
// surrogate pairs) so the language never sees a \uXXXX sequence.
static int ae_json_decode(const char *t, int64_t n, int64_t *i, ae_string *out, size_t line, size_t column) {
    if (*i >= n || t[*i] != '"') return 0;
    int64_t j = *i + 1;
    while (j < n) {
        const unsigned char c = (unsigned char)t[j];
        if (c == '"') break;
        if (c == '\\') { j += 2; continue; }
        if (c < 0x20u) return 0;
        j += 1;
    }
    if (j >= n) return 0;
    const int64_t raw = j - (*i + 1);
    char *buffer = ae_alloc((size_t)raw + 1, line, column);
    int64_t s = *i + 1;
    int64_t k = 0;
    while (s < j) {
        const unsigned char c = (unsigned char)t[s];
        if (c != '\\') { buffer[k] = (char)c; k += 1; s += 1; continue; }
        s += 1;
        if (s >= j) { ae_free(buffer); return 0; }
        const char e = t[s];
        s += 1;
        switch (e) {
            case '"': buffer[k] = '"'; k += 1; break;
            case '\\': buffer[k] = '\\'; k += 1; break;
            case '/': buffer[k] = '/'; k += 1; break;
            case 'b': buffer[k] = '\b'; k += 1; break;
            case 'f': buffer[k] = '\f'; k += 1; break;
            case 'n': buffer[k] = '\n'; k += 1; break;
            case 'r': buffer[k] = '\r'; k += 1; break;
            case 't': buffer[k] = '\t'; k += 1; break;
            case 'u': {
                unsigned cp = 0;
                if (s + 4 > j) { ae_free(buffer); return 0; }
                for (int d = 0; d < 4; d++) {
                    const int v = ae_json_hex(t[s + d]);
                    if (v < 0) { ae_free(buffer); return 0; }
                    cp = (cp << 4) | (unsigned)v;
                }
                s += 4;
                if (cp >= 0xD800u && cp <= 0xDBFFu && s + 6 <= j && t[s] == '\\' && t[s + 1] == 'u') {
                    unsigned low = 0;
                    int ok = 1;
                    for (int d = 0; d < 4; d++) {
                        const int v = ae_json_hex(t[s + 2 + d]);
                        if (v < 0) { ok = 0; break; }
                        low = (low << 4) | (unsigned)v;
                    }
                    if (ok && low >= 0xDC00u && low <= 0xDFFFu) {
                        cp = 0x10000u + ((cp - 0xD800u) << 10) + (low - 0xDC00u);
                        s += 6;
                    }
                }
                // A NUL would end the string early for every C consumer.
                if (cp == 0) { ae_free(buffer); return 0; }
                k += ae_json_utf8(buffer + k, cp);
                break;
            }
            default: ae_free(buffer); return 0;
        }
    }
    buffer[k] = 0;
    out->ptr = buffer;
    out->len = k;
    *i = j + 1;
    return 1;
}
static int ae_json_kind(const char *t, int64_t n, int64_t start) {
    if (start >= n) return -1;
    const char c = t[start];
    if (c == '"') return AE_JSON_STRING;
    if (c == '{') return AE_JSON_OBJECT;
    if (c == '[') return AE_JSON_ARRAY;
    if (c == 't' || c == 'f') return AE_JSON_BOOL;
    if (c == 'n') return AE_JSON_NULL;
    return AE_JSON_NUMBER;
}
static const char *ae_json_kind_name(int kind) {
    switch (kind) {
        case AE_JSON_STRING: return "a string";
        case AE_JSON_NUMBER: return "a number";
        case AE_JSON_BOOL: return "a boolean";
        case AE_JSON_NULL: return "null";
        case AE_JSON_ARRAY: return "an array";
        case AE_JSON_OBJECT: return "an object";
        default: return "nothing";
    }
}
// Locates one member inside the object at *pos, leaving its value span in
// *vs/*ve. *found separates "no such member" from "not an object at all".
static int ae_json_member(const char *t, int64_t n, int64_t *pos, const char *key, int64_t key_len, int64_t *vs, int64_t *ve, int *found, size_t line, size_t column) {
    *found = 0;
    int64_t i = *pos;
    if (i >= n || t[i] != '{') return 0;
    i += 1;
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == '}') { *pos = i + 1; return 1; }
    for (;;) {
        ae_json_ws(t, n, &i);
        ae_string name;
        if (!ae_json_decode(t, n, &i, &name, line, column)) return 0;
        ae_json_ws(t, n, &i);
        if (i >= n || t[i] != ':') { ae_free(name.ptr); return 0; }
        i += 1;
        ae_json_ws(t, n, &i);
        const int64_t start = i;
        if (!ae_json_skip_value(t, n, &i, 0)) { ae_free(name.ptr); return 0; }
        const int match = name.len == key_len && memcmp(name.ptr, key, (size_t)key_len) == 0;
        ae_free(name.ptr);
        if (match) { *vs = start; *ve = i; *found = 1; return 1; }
        ae_json_ws(t, n, &i);
        if (i >= n) return 0;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == '}') { *pos = i + 1; return 1; }
        return 0;
    }
}
static int ae_json_element(const char *t, int64_t n, int64_t *pos, int64_t index, int64_t *vs, int64_t *ve, int *found) {
    *found = 0;
    int64_t i = *pos;
    if (i >= n || t[i] != '[') return 0;
    i += 1;
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == ']') { *pos = i + 1; return 1; }
    int64_t k = 0;
    for (;;) {
        ae_json_ws(t, n, &i);
        const int64_t start = i;
        if (!ae_json_skip_value(t, n, &i, 0)) return 0;
        if (k == index) { *vs = start; *ve = i; *found = 1; return 1; }
        k += 1;
        ae_json_ws(t, n, &i);
        if (i >= n) return 0;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == ']') { *pos = i + 1; return 1; }
        return 0;
    }
}
// Validates the path on its own first, so a typo in the path is reported as a
// path error even when the document would have failed on the same segment.
static int ae_json_path_check(const char *path, int64_t plen, char *why, size_t why_cap) {
    int64_t p = 0;
    while (p < plen) {
        if (path[p] == '[') {
            const int64_t open = p;
            p += 1;
            int digits = 0;
            while (p < plen && path[p] >= '0' && path[p] <= '9') { p += 1; digits += 1; }
            if (digits == 0 || p >= plen || path[p] != ']') {
                snprintf(why, why_cap, "path index at byte %lld needs digits and a closing ']'", (long long)open);
                return 0;
            }
            p += 1;
            if (p < plen && path[p] == '.') {
                p += 1;
                if (p >= plen) { snprintf(why, why_cap, "path ends with '.'"); return 0; }
            }
            continue;
        }
        const int64_t kstart = p;
        while (p < plen && path[p] != '.' && path[p] != '[') p += 1;
        if (p == kstart) {
            snprintf(why, why_cap, "path has an empty segment at byte %lld", (long long)kstart);
            return 0;
        }
        if (p < plen && path[p] == '.') {
            p += 1;
            if (p >= plen) { snprintf(why, why_cap, "path ends with '.'"); return 0; }
        }
    }
    return 1;
}
// Path grammar: `key`, `key.sub`, `key[2].sub`, `[0].key`. A segment is a run of
// bytes up to the next '.' or '['; keys are compared after unescaping.
static int ae_json_locate(const char *t, int64_t n, const char *path, int64_t plen, int64_t *vs, int64_t *ve, int *kind, char *why, size_t why_cap, size_t line, size_t column) {
    if (!ae_json_path_check(path, plen, why, why_cap)) return AE_JSON_SYNTAX;
    int64_t pos = 0;
    ae_json_ws(t, n, &pos);
    const int64_t start0 = pos;
    if (!ae_json_skip_value(t, n, &pos, 0)) {
        snprintf(why, why_cap, "JSON syntax error near byte %lld", (long long)start0);
        return AE_JSON_SYNTAX;
    }
    int64_t start = start0;
    int64_t end = pos;
    int current = ae_json_kind(t, n, start);
    int64_t p = 0;
    while (p < plen) {
        if (path[p] == '[') {
            const int64_t open = p;
            p += 1;
            int64_t index = 0;
            int digits = 0;
            while (p < plen && path[p] >= '0' && path[p] <= '9' && digits < 10) { index = index * 10 + (path[p] - '0'); p += 1; digits += 1; }
            if (digits == 0 || p >= plen || path[p] != ']') {
                snprintf(why, why_cap, "path index at byte %lld needs digits and a closing ']'", (long long)open);
                return AE_JSON_SYNTAX;
            }
            p += 1;
            if (current != AE_JSON_ARRAY) {
                snprintf(why, why_cap, "path segment [%lld] needs an array, found %s", (long long)index, ae_json_kind_name(current));
                return AE_JSON_EXPECTED;
            }
            int64_t cursor = start;
            int found = 0;
            if (!ae_json_element(t, n, &cursor, index, &start, &end, &found)) {
                snprintf(why, why_cap, "JSON syntax error while reading an array element");
                return AE_JSON_SYNTAX;
            }
            if (!found) {
                snprintf(why, why_cap, "path segment [%lld] is past the end of the array", (long long)index);
                return AE_JSON_MISSING;
            }
        } else {
            const int64_t kstart = p;
            while (p < plen && path[p] != '.' && path[p] != '[') p += 1;
            const int64_t klen = p - kstart;
            if (klen == 0) {
                snprintf(why, why_cap, "path has an empty segment at byte %lld", (long long)kstart);
                return AE_JSON_SYNTAX;
            }
            if (current != AE_JSON_OBJECT) {
                snprintf(why, why_cap, "path segment '%.*s' needs an object, found %s", (int)klen, path + kstart, ae_json_kind_name(current));
                return AE_JSON_EXPECTED;
            }
            int64_t cursor = start;
            int found = 0;
            if (!ae_json_member(t, n, &cursor, path + kstart, klen, &start, &end, &found, line, column)) {
                snprintf(why, why_cap, "JSON syntax error while reading a member");
                return AE_JSON_SYNTAX;
            }
            if (!found) {
                snprintf(why, why_cap, "no member named '%.*s'", (int)klen, path + kstart);
                return AE_JSON_MISSING;
            }
        }
        current = ae_json_kind(t, n, start);
        if (p < plen) {
            if (path[p] != '.' && path[p] != '[') {
                snprintf(why, why_cap, "path segment ends unexpectedly at byte %lld", (long long)p);
                return AE_JSON_SYNTAX;
            }
            if (path[p] == '.') {
                p += 1;
                if (p >= plen) {
                    snprintf(why, why_cap, "path ends with '.'");
                    return AE_JSON_SYNTAX;
                }
            }
        }
    }
    *vs = start;
    *ve = end;
    *kind = current;
    return AE_JSON_OK;
}
static int ae_json_count(const char *t, int64_t n, int64_t start, int kind) {
    int64_t i = start + 1;
    int64_t count = 0;
    const char close = kind == AE_JSON_OBJECT ? '}' : ']';
    ae_json_ws(t, n, &i);
    if (i < n && t[i] == close) return 0;
    for (;;) {
        ae_json_ws(t, n, &i);
        if (kind == AE_JSON_OBJECT) {
            if (!ae_json_skip_string(t, n, &i)) return -1;
            ae_json_ws(t, n, &i);
            if (i >= n || t[i] != ':') return -1;
            i += 1;
        }
        if (!ae_json_skip_value(t, n, &i, 0)) return -1;
        count += 1;
        ae_json_ws(t, n, &i);
        if (i >= n) return -1;
        if (t[i] == ',') { i += 1; continue; }
        if (t[i] == close) return count;
        return -1;
    }
}
static AE_READ_RESULT ae_json_get(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    ae_string value;
    if (kind == AE_JSON_STRING) {
        int64_t at = vs;
        if (!ae_json_decode(doc.ptr, doc.len, &at, &value, line, column)) {
            AE_READ_RESULT out = { AE_READ_ERR, { .v1 = { ae_str_from("JSON string could not be decoded", line, column) } } };
            return out;
        }
    } else {
        value = ae_str_from_len(doc.ptr + vs, ve - vs, line, column);
    }
    AE_READ_RESULT out = { AE_READ_OK, { .v0 = { value } } };
    return out;
}
static AE_INT_RESULT ae_json_int(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    if (kind != AE_JSON_NUMBER) {
        snprintf(why, sizeof why, "'%.*s' is %s, not a number", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    int64_t i = vs;
    int negative = 0;
    if (i < ve && doc.ptr[i] == '-') { negative = 1; i += 1; }
    if (i >= ve) {
        snprintf(why, sizeof why, "'%.*s' is not an integer", (int)path.len, path.ptr);
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    int64_t value = 0;
    for (; i < ve; i++) {
        const char d = doc.ptr[i];
        if (d < '0' || d > '9') {
            // A fraction or exponent is a real number; this reader has no float type.
            snprintf(why, sizeof why, "'%.*s' is not an integer", (int)path.len, path.ptr);
            AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
            return out;
        }
        const int digit = d - '0';
        if (value > (INT64_MAX - digit) / 10) {
            snprintf(why, sizeof why, "'%.*s' does not fit in i64", (int)path.len, path.ptr);
            AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
            return out;
        }
        value = value * 10 + digit;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { negative ? -value : value } } };
    return out;
}
static AE_BOOL_RESULT ae_json_bool(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_BOOL_RESULT out = { AE_BOOL_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    const int64_t width = ve - vs;
    bool value = false;
    if (kind == AE_JSON_BOOL && width == 4 && memcmp(doc.ptr + vs, "true", 4) == 0) value = true;
    else if (!(kind == AE_JSON_BOOL && width == 5 && memcmp(doc.ptr + vs, "false", 5) == 0)) {
        snprintf(why, sizeof why, "'%.*s' is %s, not a boolean", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_BOOL_RESULT out = { AE_BOOL_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    AE_BOOL_RESULT out = { AE_BOOL_OK, { .v0 = { value } } };
    return out;
}
static AE_INT_RESULT ae_json_len(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    if (ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) != AE_JSON_OK) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    if (kind != AE_JSON_ARRAY && kind != AE_JSON_OBJECT) {
        snprintf(why, sizeof why, "'%.*s' is %s; only arrays and objects have a member count", (int)path.len, path.ptr, ae_json_kind_name(kind));
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from(why, line, column) } } };
        return out;
    }
    const int64_t count = ae_json_count(doc.ptr, doc.len, vs, kind);
    if (count < 0) {
        AE_INT_RESULT out = { AE_INT_ERR, { .v1 = { ae_str_from("JSON syntax error while counting members", line, column) } } };
        return out;
    }
    AE_INT_RESULT out = { AE_INT_OK, { .v0 = { count } } };
    return out;
}
static bool ae_json_has(const char *doc_text, int64_t doc_size, const char *path_text, int64_t path_size, size_t line, size_t column) {
    const ae_strview doc = { doc_text, doc_size };
    const ae_strview path = { path_text, path_size };

    int64_t vs = 0;
    int64_t ve = 0;
    int kind = 0;
    char why[192];
    return ae_json_locate(doc.ptr, doc.len, path.ptr, path.len, &vs, &ve, &kind, why, sizeof why, line, column) == AE_JSON_OK;
}

// ---- Child processes (ADR-29) ----------------------------------------------
// The child's stdout and stderr share one pipe: a program that reports its
// failure on stderr still comes back in the order it was written, and the
// parent never has to drain two pipes at once (which deadlocks as soon as one
// of them fills). A non-zero exit status is a result, not an error.
static AE_COMMAND_RESULT ae_command_failed(ae_string message) {
    AE_COMMAND_RESULT out = { AE_COMMAND_ERR, { .v1 = { message } } };
    return out;
}
#ifndef _WIN32
static AE_COMMAND_RESULT ae_run_command(const char *program, const ae_strvec *args, size_t line, size_t column) {
    int fds[2];
    if (pipe(fds) != 0) return ae_command_failed(ae_io_message(errno, line, column));
    // A second pipe carries the exec failure back: closing on exec (CLOEXEC)
    // turns "exec worked" into EOF, so the parent can tell "could not start the
    // program" apart from "the program chose to exit 127".
    int errors[2];
    if (pipe(errors) != 0) {
        const int code = errno;
        close(fds[0]);
        close(fds[1]);
        return ae_command_failed(ae_io_message(code, line, column));
    }
    (void)fcntl(errors[1], F_SETFD, FD_CLOEXEC);
    const pid_t pid = fork();
    if (pid < 0) {
        const int code = errno;
        close(fds[0]);
        close(fds[1]);
        close(errors[0]);
        close(errors[1]);
        return ae_command_failed(ae_io_message(code, line, column));
    }
    if (pid == 0) {
        // Child: point both streams at the pipe, then become the program.
        // _exit skips the parent's stdio buffers and atexit work.
        if (dup2(fds[1], STDOUT_FILENO) < 0 || dup2(fds[1], STDERR_FILENO) < 0) _exit(127);
        close(fds[0]);
        close(fds[1]);
        close(errors[0]);
        char **argv = ae_alloc_try(sizeof(char *) * (size_t)(args->len + 2));
        if (!argv) _exit(127);
        argv[0] = (char *)program;
        for (int64_t i = 0; i < args->len; i++) argv[i + 1] = args->data[i].ptr;
        argv[args->len + 1] = NULL;
        execvp(program, argv);
        const int code = errno;
        (void)!write(errors[1], &code, sizeof code);
        _exit(127);
    }
    close(fds[1]);
    close(errors[1]);
    int exec_error = 0;
    ssize_t reported = 0;
    do {
        reported = read(errors[0], &exec_error, sizeof exec_error);
    } while (reported < 0 && errno == EINTR);
    close(errors[0]);
    if (reported == (ssize_t)sizeof exec_error) {
        close(fds[0]);
        int ignored = 0;
        while (waitpid(pid, &ignored, 0) < 0 && errno == EINTR) {}
        return ae_command_failed(ae_status_message("cannot start the program", (unsigned long)exec_error, line, column));
    }
    int64_t capacity = 4096;
    int64_t length = 0;
    char *buffer = ae_alloc((size_t)capacity + 1, line, column);
    for (;;) {
        if (length == capacity) {
            char *grown = ae_alloc((size_t)capacity * 2 + 1, line, column);
            memcpy(grown, buffer, (size_t)length);
            ae_free(buffer);
            buffer = grown;
            capacity = capacity * 2;
        }
        const ssize_t got = read(fds[0], buffer + length, (size_t)(capacity - length));
        if (got < 0) {
            if (errno == EINTR) continue;
            const int code = errno;
            ae_free(buffer);
            close(fds[0]);
            return ae_command_failed(ae_io_message(code, line, column));
        }
        if (got == 0) break;
        length += got;
    }
    close(fds[0]);
    buffer[length] = 0;
    int status = 0;
    for (;;) {
        if (waitpid(pid, &status, 0) >= 0) break;
        if (errno != EINTR) {
            const int code = errno;
            ae_free(buffer);
            return ae_command_failed(ae_io_message(code, line, column));
        }
    }
    // A signal is reported the way a shell reports it (128 + number), so the
    // caller sees one integer either way.
    const int64_t code = WIFEXITED(status) ? (int64_t)WEXITSTATUS(status)
                                           : (int64_t)(128 + (WIFSIGNALED(status) ? WTERMSIG(status) : 0));
    ae_string output = { buffer, length };
    AE_COMMAND_RESULT out = { AE_COMMAND_OK, { .v0 = { code, output } } };
    return out;
}
#else
static AE_COMMAND_RESULT ae_run_command(const char *program, const ae_strvec *args, size_t line, size_t column) {
    // Quotes and trailing backslashes are refused instead of escaped: guessing
    // CreateProcess's quoting rules wrong silently changes what the child runs.
    for (int64_t i = 0; i < args->len; i++) {
        const ae_string arg = args->data[i];
        for (int64_t k = 0; k < arg.len; k++) {
            if (arg.ptr[k] == '"') return ae_command_failed(ae_str_from("command arguments must not contain a double quote", line, column));
        }
        if (arg.len > 0 && arg.ptr[arg.len - 1] == '\\') return ae_command_failed(ae_str_from("command arguments must not end with a backslash", line, column));
    }
    for (const char *c = program; *c; c++) {
        if (*c == '"') return ae_command_failed(ae_str_from("the program name must not contain a double quote", line, column));
    }
    // Build the command line in UTF-8 first: the quoting characters are ASCII,
    // so one conversion at the end is enough.
    int64_t width = (int64_t)strlen(program) + 3;
    for (int64_t i = 0; i < args->len; i++) width += args->data[i].len + 3;
    char *command = ae_alloc((size_t)width, line, column);
    int64_t at = 0;
    command[at++] = '"';
    for (const char *c = program; *c; c++) command[at++] = *c;
    command[at++] = '"';
    for (int64_t i = 0; i < args->len; i++) {
        const ae_string arg = args->data[i];
        int spaced = 0;
        for (int64_t k = 0; k < arg.len; k++) {
            if (arg.ptr[k] == ' ' || arg.ptr[k] == '\t') { spaced = 1; break; }
        }
        // Quote only what has to be quoted. A shell that re-parses its own
        // command line treats a quoted flag as part of the command name, so
        // quoting every argument breaks the commonest helper program there is.
        command[at++] = ' ';
        if (spaced) command[at++] = '"';
        memcpy(command + at, arg.ptr, (size_t)arg.len);
        at += arg.len;
        if (spaced) command[at++] = '"';
    }
    command[at] = 0;

    SECURITY_ATTRIBUTES sa;
    ZeroMemory(&sa, sizeof sa);
    sa.nLength = sizeof sa;
    sa.bInheritHandle = TRUE;
    HANDLE read_end = NULL;
    HANDLE write_end = NULL;
    if (!CreatePipe(&read_end, &write_end, &sa, 0)) {
        ae_free(command);
        return ae_command_failed(ae_status_message("cannot create a pipe", (unsigned long)GetLastError(), line, column));
    }
    SetHandleInformation(read_end, HANDLE_FLAG_INHERIT, 0);
    STARTUPINFOW si;
    ZeroMemory(&si, sizeof si);
    si.cb = sizeof si;
    si.dwFlags = STARTF_USESTDHANDLES;
    si.hStdOutput = write_end;
    si.hStdError = write_end;
    si.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    PROCESS_INFORMATION pi;
    ZeroMemory(&pi, sizeof pi);
    wchar_t *wide = ae_widen(command);
    ae_free(command);
    if (!wide) {
        CloseHandle(read_end);
        CloseHandle(write_end);
        return ae_command_failed(ae_str_from("out of memory", line, column));
    }
    const BOOL started = CreateProcessW(NULL, wide, NULL, NULL, TRUE, CREATE_NO_WINDOW, NULL, NULL, &si, &pi);
    const DWORD failure = started ? 0 : GetLastError();
    ae_free(wide);
    CloseHandle(write_end);
    if (!started) {
        CloseHandle(read_end);
        return ae_command_failed(ae_status_message("cannot start the program", (unsigned long)failure, line, column));
    }
    CloseHandle(pi.hThread);
    int64_t capacity = 4096;
    int64_t length = 0;
    char *buffer = ae_alloc((size_t)capacity + 1, line, column);
    for (;;) {
        if (length == capacity) {
            char *grown = ae_alloc((size_t)capacity * 2 + 1, line, column);
            memcpy(grown, buffer, (size_t)length);
            ae_free(buffer);
            buffer = grown;
            capacity = capacity * 2;
        }
        DWORD got = 0;
        const int64_t room = capacity - length;
        if (!ReadFile(read_end, buffer + length, (DWORD)(room > 0x7FFFFFFF ? 0x7FFFFFFF : room), &got, NULL)) {
            const DWORD code = GetLastError();
            if (code == ERROR_BROKEN_PIPE) break;
            ae_free(buffer);
            CloseHandle(read_end);
            CloseHandle(pi.hProcess);
            return ae_command_failed(ae_status_message("cannot read the program output", (unsigned long)code, line, column));
        }
        if (got == 0) break;
        length += (int64_t)got;
    }
    CloseHandle(read_end);
    buffer[length] = 0;
    WaitForSingleObject(pi.hProcess, INFINITE);
    DWORD status = 0;
    GetExitCodeProcess(pi.hProcess, &status);
    CloseHandle(pi.hProcess);
    ae_string output = { buffer, length };
    AE_COMMAND_RESULT out = { AE_COMMAND_OK, { .v0 = { (int64_t)status, output } } };
    return out;
}
#endif

int main(int argc, char **argv) {
    ae_argc_g = (int64_t)argc;
    ae_argv_g = argv;
    if (printf("%" PRId64 "\n", INT64_C(1)) < 0) ae_fail("output error", 0, 0);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
