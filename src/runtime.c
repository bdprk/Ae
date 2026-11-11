#include <stdint.h>
#include <inttypes.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <limits.h>
// ---- Output (ADR-55) --------------------------------------------------------
// A console decodes the bytes it receives with its own output code page (936 on
// a Chinese Windows), so raw UTF-8 showed up there as unrelated characters.
#ifdef _WIN32
static void *ae_console(void) { /* win32-declared */
    // Declared here, not through <windows.h>: the header costs about half a
    // second of build time, and a declaration inside a function is pruned with
    // it, where top-level text is always kept. The types match the header, so
    // both coexist where a program needs the header for something else (ADR-55).
    extern void *GetStdHandle(unsigned long index);
    extern int GetConsoleMode(void *handle, unsigned long *mode);
    // GetConsoleMode fails on a pipe or a file: exactly when bytes must pass
    // through untouched. Resolved once, so a redirected run pays two calls.
    static void *handle = NULL;
    static int resolved = 0;
    if (!resolved) {
        unsigned long mode = 0;
        handle = GetStdHandle((unsigned long)-11);
        if (!handle || handle == (void *)-1 || !GetConsoleMode(handle, &mode)) handle = NULL;
        resolved = 1;
    }
    return handle;
}
// WriteConsoleW accepts fewer units than asked for; retry rather than drop.
static bool ae_write_units(void *handle, const wchar_t *text, int count) { /* win32-declared */
    extern int WriteConsoleW(void *handle, const void *text, unsigned long count, unsigned long *written, void *reserved);
    while (count > 0) {
        unsigned long written = 0;
        if (!WriteConsoleW(handle, text, (unsigned long)count, &written, NULL) || written == 0) return false;
        text += written;
        count -= (int)written;
    }
    return true;
}
static bool ae_write_console(void *handle, const char *ptr, int64_t len, int newline) { /* win32-declared */
    extern int MultiByteToWideChar(unsigned page, unsigned long flags, const char *text, int bytes, wchar_t *wide, int units);
    // Converts in 1 KiB chunks, so printing a multi-megabyte string costs a 2 KiB
    // stack frame rather than a heap buffer the size of the text. A chunk never
    // ends inside a character, an invalid sequence becomes U+FFFD (a console
    // decodes, it does not validate), and the newline joins the last chunk
    // because one console write costs microseconds: a printed line is one call.
    wchar_t buffer[1024];
    const int capacity = newline ? 1023 : 1024;
    int units = 0;
    for (;;) {
        int chunk = len > capacity ? capacity : (int)len;
        while (chunk > 1 && (int64_t)chunk < len && ((unsigned char)ptr[chunk] & 0xC0) == 0x80) chunk -= 1;
        units = chunk > 0 ? MultiByteToWideChar(65001, 0, ptr, chunk, buffer, capacity) : 0;
        if (chunk > 0 && units <= 0) return false;
        ptr += chunk;
        len -= chunk;
        if (len <= 0) break;
        if (units > 0 && !ae_write_units(handle, buffer, units)) return false;
    }
    if (newline) buffer[units++] = L'\n';
    return units == 0 || ae_write_units(handle, buffer, units);
}
#endif
static bool ae_emit(const char *ptr, int64_t len, int newline) {
#ifdef _WIN32
    void *console = ae_console();
    if (console) return ae_write_console(console, ptr, len, newline);
#endif
    if (fwrite(ptr, 1, (size_t)len, stdout) != (size_t)len) return false;
    return !newline || fputc('\n', stdout) != EOF;
}

// Deliberate termination with a status. Buffered output is flushed first, so a
// program that prints and then exits does not lose the last line to a pipe. The
// code is truncated to int: POSIX shells only see the low eight bits, and
// pretending otherwise in the language would be worse than documenting it.
_Noreturn static void ae_exit(int64_t code) {
    fflush(stdout);
    exit((int)code);
}
// Runtime messages are fixed ASCII text (an operation that quotes user data
// returns an Err the program prints itself, through the console sink), so stderr
// keeps plain fprintf and a program that never prints carries none of it.
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
// NUL-terminated for C interop. Concatenation consumes both operands; moves
// copy the struct and forfeit the slot; releases are idempotent so
// overlapping release paths can never double-free.
typedef struct { char *ptr; int64_t len; } ae_string;
static const char ae_missing_key_text[] = "key not found";
static void ae_str_release(ae_string *slot) {
    // The miss text is read-only data, so release skips it by identity instead
    // of freeing it. Equality is defined for unrelated pointers; a range test
    // with < or > would not be.
    if (slot->ptr && slot->ptr != ae_missing_key_text) ae_free(slot->ptr);
    slot->ptr = NULL;
    slot->len = 0;
}
// Borrowed string view (ADR-8): zero-copy, never released. Views point into
// an owning buffer; the compile-time borrow tracker keeps the base alive.
typedef struct { const char *ptr; int64_t len; } ae_strview;
#define AE_VIEW(s) ((ae_strview){(s).ptr, (s).len})

// `print`'s shapes, all ending in a newline and all through one sink, so output
// keeps the order the program wrote it in.
static void ae_print_view(ae_strview text) {
    if (!ae_emit(text.ptr, text.len, 1)) ae_fail("output error", 0, 0);
}
static void ae_print_i64(int64_t value) {
    char buffer[32];
    const int length = snprintf(buffer, sizeof buffer, "%" PRId64, value);
    if (length <= 0 || !ae_emit(buffer, length, 1)) ae_fail("output error", 0, 0);
}
static void ae_print_cstr(const char *text) {
    ae_print_view((ae_strview){ text, (int64_t)strlen(text) });
}
static void ae_print_bool(bool value) {
    if (!ae_emit(value ? "true" : "false", value ? 4 : 5, 1)) ae_fail("output error", 0, 0);
}
static ae_string ae_str_from(const char *text, size_t line, size_t column) {
    size_t n = strlen(text);
    char *copy = ae_alloc(n + 1, line, column);
    memcpy(copy, text, n + 1);
    ae_string s = { copy, (int64_t)n };
    return s;
}
static ae_string ae_str_cat(ae_string a, ae_string b, size_t line, size_t column) {
    if (a.len > INT64_MAX - b.len) { ae_str_release(&a); ae_str_release(&b); ae_fail("string too long", line, column); }
    const int64_t total = a.len + b.len;
    char *out = ae_alloc((size_t)total + 1, line, column);
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, b.ptr, (size_t)b.len + 1);
    ae_str_release(&a); ae_str_release(&b);
    ae_string s = { out, total };
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
    if (b.len > (int64_t)(INT64_MAX - (int64_t)n)) { ae_str_release(&b); ae_fail("string too long", line, column); }
    const int64_t total = (int64_t)n + b.len;
    char *out = ae_alloc((size_t)total + 1, line, column);
    memcpy(out, lit, n);
    memcpy(out + n, b.ptr, (size_t)b.len + 1);
    ae_str_release(&b);
    ae_string s = { out, total };
    return s;
}
static ae_string ae_str_cat_lit_right(ae_string a, const char *lit, size_t line, size_t column) {
    const size_t n = strlen(lit);
    if ((int64_t)n > INT64_MAX - a.len) { ae_str_release(&a); ae_fail("string too long", line, column); }
    const int64_t total = a.len + (int64_t)n;
    char *out = ae_alloc((size_t)total + 1, line, column);
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, lit, n + 1);
    ae_str_release(&a);
    ae_string s = { out, total };
    return s;
}
static void ae_str_forfeit(ae_string *slot) { slot->ptr = NULL; slot->len = 0; }
static bool ae_str_eq(ae_strview a, ae_strview b) {
    if (a.len != b.len) return false;
    // Map keys are short ("k1234"): a byte loop inlines and beats a memcmp call
    // whose fixed cost is several times the comparison itself (ADR-56).
    if (a.len <= 8) {
        for (int64_t i = 0; i < a.len; i++) {
            if (a.ptr[i] != b.ptr[i]) return false;
        }
        return true;
    }
    return memcmp(a.ptr, b.ptr, (size_t)a.len) == 0;
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
//
// `arena` is NULL for vectors built element by element (`push`, `args`, `keys`):
// every element owns its own buffer and is released individually. `split` and
// `lines` instead copy all tokens into one arena and point every element into
// it, so N tokens cost two allocations instead of N (ADR-49). Elements whose
// pointer lies inside the arena are cleared without freeing; the arena is
// released with the vector.
typedef struct { ae_string *data; int64_t len; int64_t cap; char *arena; int64_t arena_len; } ae_strvec;
static int ae_strvec_borrowed(const ae_strvec *v, const ae_string *s) {
    // Compare as integers: two pointers into unrelated objects have no defined
    // ordering in ISO C, but their uintptr_t values are comparable here.
    return v->arena != NULL
        && s->ptr != NULL
        && (uintptr_t)s->ptr >= (uintptr_t)v->arena
        && (uintptr_t)s->ptr < (uintptr_t)v->arena + (uintptr_t)v->arena_len;
}
static ae_strvec ae_strvec_new(size_t line, size_t column) {
    ae_strvec v;
    v.len = 0;
    v.cap = 4;
    v.data = ae_alloc((size_t)v.cap * sizeof(ae_string), line, column);
    v.arena = NULL;
    v.arena_len = 0;
    return v;
}
static void ae_strvec_push(ae_strvec *v, ae_string s, size_t line, size_t column) {
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
static void ae_strvec_pop(ae_strvec *v, size_t line, size_t column) {
    if (v->len == 0) ae_fail("pop from empty vector", line, column);
    v->len = v->len - 1;
    if (ae_strvec_borrowed(v, &v->data[v->len])) {
        v->data[v->len].ptr = NULL;
        v->data[v->len].len = 0;
    } else {
        ae_str_release(&v->data[v->len]);
    }
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
static void ae_strvec_clear_slot(ae_strvec *v, int64_t index, size_t line, size_t column) {
    ae_strvec_index(index, v->len, line, column);
    if (ae_strvec_borrowed(v, &v->data[index])) {
        v->data[index].ptr = NULL;
        v->data[index].len = 0;
    } else {
        ae_str_release(&v->data[index]);
    }
}
static void ae_strvec_forfeit(ae_strvec *v) {
    v->data = NULL;
    v->arena = NULL;
    v->arena_len = 0;
    v->len = 0;
    v->cap = 0;
}
// Next separator position at or after `from`, or -1 when none is left. A
// one-byte separator (the common `" "` and `"\n"` shapes) goes through memchr,
// which is SIMD-optimised in both libc implementations, instead of comparing
// byte by byte (ADR-49).
static int64_t ae_find_sep(const char *base, int64_t from, int64_t len, ae_strview sep) {
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
static ae_strvec ae_split(ae_strview s, ae_strview sep, size_t line, size_t column) {
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
static ae_tokens ae_tokens_new(ae_strview text, ae_strview sep, size_t line, size_t column) {
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
static void ae_tokens_reset(ae_tokens *cursor) { cursor->pos = 0; cursor->done = 0; }
// Returns false once the stream is exhausted; `out` points into the base text.
static bool ae_tokens_next(ae_tokens *cursor, ae_strview *out) {
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

// lines(): split on newlines. Trailing newlines do not produce a final empty
// line, matching common stdlib lines(). Two passes with memchr (ADR-49): count
// lines first, then fill one exact-size pointer array and one byte arena.
static ae_strvec ae_lines(ae_strview s, size_t line, size_t column) {
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

typedef struct { double *data; int64_t len; int64_t cap; } ae_f64vec; // ADR-61
static ae_f64vec ae_f64vec_new(size_t line, size_t column) {
    // Same shape as ae_vec with double elements (ADR-61). The element
    // type is a plain value — no ownership, no borrowed views, no release
    // per element — so this family has no borrow tracking at all.
    ae_f64vec v;
    v.len = 0;
    v.cap = 4;
    v.data = ae_alloc((size_t)v.cap * sizeof(double), line, column);
    return v;
}
static void ae_f64vec_push(ae_f64vec *v, double x, size_t line, size_t column) {
    if (v->len == v->cap) {
        if (v->cap > INT64_MAX / 2) ae_fail("vector too large", line, column);
        int64_t ncap = v->cap * 2;
        double *nd = ae_alloc_resize(v->data, (size_t)ncap * sizeof(double), line, column);
        v->data = nd;
        v->cap = ncap;
    }
    v->data[v->len] = x;
    v->len = v->len + 1;
}
static void ae_f64vec_pop(ae_f64vec *v, size_t line, size_t column) {
    if (v->len == 0) ae_fail("pop from empty vector", line, column);
    v->len = v->len - 1;
}
static int64_t ae_f64vec_len(ae_f64vec v, size_t line, size_t column) {
    (void)line; (void)column;
    return v.len;
}
static double ae_f64vec_get(ae_f64vec v, int64_t index, size_t line, size_t column) {
    ae_vec_index(index, v.len, line, column);
    return v.data[index];
}
static void ae_f64vec_release(ae_f64vec *v) {
    ae_free(v->data);
    v->data = NULL;
    v->len = 0;
    v->cap = 0;
}
static void ae_f64vec_forfeit(ae_f64vec *v) {
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

#ifdef _WIN32
// Windows hands `main` the command line in the ANSI code page, so a path typed
// on a Chinese system arrives as GBK bytes while every file API expects UTF-8 --
// the program could not open its own arguments (ADR-54). Convert each argument
// through UTF-16 once, here, so `args()` matches the rest of the language.
// This keeps <windows.h> in programs that read their arguments; programs that
// do not still prune it (ADR-39).
static char *ae_utf8_from_acp(const char *text) { /* win32-header */
    const int wide_len = MultiByteToWideChar(CP_ACP, 0, text, -1, NULL, 0);
    if (wide_len <= 0) return NULL;
    wchar_t *wide = ae_alloc_try((size_t)wide_len * sizeof(wchar_t));
    if (!wide) return NULL;
    char *utf8 = NULL;
    if (MultiByteToWideChar(CP_ACP, 0, text, -1, wide, wide_len) > 0) {
        const int size = WideCharToMultiByte(CP_UTF8, 0, wide, wide_len - 1, NULL, 0, NULL, NULL);
        if (size > 0) {
            utf8 = ae_alloc_try((size_t)size + 1);
            if (utf8) {
                WideCharToMultiByte(CP_UTF8, 0, wide, wide_len - 1, utf8, size, NULL, NULL);
                utf8[size] = 0;
            }
        }
    }
    ae_free(wide);
    return utf8;
}
#endif
static ae_strvec ae_args(size_t line, size_t column) {
    ae_strvec out = ae_strvec_new(line, column);
    for (int64_t i = 0; i < ae_argc_g; i++) {
#ifdef _WIN32
        // Falls back to the raw bytes if the conversion fails (out of memory),
        // which keeps an argument usable even when it cannot be decoded.
        char *utf8 = ae_utf8_from_acp(ae_argv_g[i]);
        ae_strvec_push(&out, ae_str_from(utf8 ? utf8 : ae_argv_g[i], line, column), line, column);
        ae_free(utf8);
#else
        ae_strvec_push(&out, ae_str_from(ae_argv_g[i], line, column), line, column);
#endif
    }
    return out;
}

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
// Open addressing with linear probing. A slot holds the key and the value; the
// empty (0), full (1) or deleted (2) byte lives in a parallel array (ADR-57), so
// slots stay 24 bytes and a probe usually settles on packed state bytes without
// touching one. A deleted slot keeps probing alive until the next rehash, so
// removing a key never breaks a chain.
typedef struct { ae_string key; int64_t value; } ae_map_slot;
typedef struct { ae_map_slot *slots; unsigned char *states; int64_t cap; int64_t len; int64_t used; } ae_map;

#define AE_MAP_EMPTY 0
#define AE_MAP_FULL 1
#define AE_MAP_DELETED 2

static ae_map ae_map_new(size_t line, size_t column) {
    (void)line; (void)column;
    ae_map m;
    m.slots = NULL;
    m.states = NULL;
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
        const unsigned char state = m->states[at];
        if (state == AE_MAP_EMPTY) return -1;
        if (state == AE_MAP_FULL && ae_str_eq(AE_VIEW(m->slots[at].key), key)) return at;
        at = (at + 1) & (m->cap - 1);
    }
    return -1;
}
static void ae_map_grow(ae_map *m, size_t line, size_t column) {
    int64_t cap = m->cap == 0 ? 8 : m->cap;
    // Rehash into a table twice as large once live entries fill half of it,
    // which also drops every tombstone. Half rather than three quarters: linear
    // probing walks one slot at a time, so the average probe count is what the
    // load factor really buys (ADR-56: measured 1.17x on the map workload for
    // one extra table, against 1.03x at five eighths).
    while ((m->len + 1) * 2 >= cap) {
        if (cap > INT64_MAX / 4) ae_fail("map too large", line, column);
        cap = cap * 2;
    }
    if (cap == m->cap && m->used == m->len) return;
    // Both arrays are allocated and cleared by hand rather than with a
    // calloc-style interface: every state byte must start EMPTY, and keeping one
    // allocation primitive makes the allocator interface a single
    // implementation instead of two.
    ae_map_slot *slots = ae_alloc((size_t)cap * sizeof(ae_map_slot), line, column);
    unsigned char *states = ae_alloc((size_t)cap, line, column);
    memset(states, AE_MAP_EMPTY, (size_t)cap);
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->states[i] != AE_MAP_FULL) continue;
        int64_t at = ae_map_slot_of(AE_VIEW(m->slots[i].key), cap);
        while (states[at] == AE_MAP_FULL) at = (at + 1) & (cap - 1);
        slots[at] = m->slots[i];
        states[at] = AE_MAP_FULL;
    }
    ae_free(m->slots);
    ae_free(m->states);
    m->slots = slots;
    m->states = states;
    m->cap = cap;
    m->used = m->len;
}
// Inserts or overwrites. The key is copied; the caller keeps its own string.
static void ae_map_put(ae_map *m, ae_strview key, int64_t value, size_t line, size_t column) {
    ae_map_grow(m, line, column);
    int64_t at = ae_map_slot_of(key, m->cap);
    int64_t reuse = -1;
    for (;;) {
        const unsigned char state = m->states[at];
        if (state == AE_MAP_EMPTY) break;
        if (state == AE_MAP_DELETED) {
            if (reuse < 0) reuse = at;
        } else if (ae_str_eq(AE_VIEW(m->slots[at].key), key)) {
            m->slots[at].value = value;
            return;
        }
        at = (at + 1) & (m->cap - 1);
    }
    if (reuse >= 0) at = reuse;
    else m->used = m->used + 1;
    m->slots[at].key = ae_str_from_len(key.ptr, key.len, line, column);
    m->slots[at].value = value;
    m->states[at] = AE_MAP_FULL;
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
        const unsigned char state = m->states[at];
        if (state == AE_MAP_EMPTY) break;
        if (state == AE_MAP_DELETED) {
            if (reuse < 0) reuse = at;
        } else if (ae_str_eq(AE_VIEW(m->slots[at].key), key)) {
            m->slots[at].value = m->slots[at].value + delta;
            return m->slots[at].value;
        }
        at = (at + 1) & (m->cap - 1);
    }
    if (reuse >= 0) at = reuse;
    else m->used = m->used + 1;
    m->slots[at].key = ae_str_from_len(key.ptr, key.len, line, column);
    m->slots[at].value = delta;
    m->states[at] = AE_MAP_FULL;
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
    m->states[at] = AE_MAP_DELETED;
    m->len = m->len - 1;
    return true;
}
static int64_t ae_map_len(const ae_map *m, size_t line, size_t column) {
    (void)line; (void)column;
    return m->len;
}
static void ae_map_release(ae_map *m) {
    for (int64_t i = 0; i < m->cap; i++) {
        if (m->states[i] == AE_MAP_FULL) ae_str_release(&m->slots[i].key);
    }
    ae_free(m->slots);
    ae_free(m->states);
    m->slots = NULL;
    m->states = NULL;
    m->cap = 0;
    m->len = 0;
    m->used = 0;
}
static void ae_map_forfeit(ae_map *m) {
    m->slots = NULL;
    m->states = NULL;
    m->cap = 0;
    m->len = 0;
    m->used = 0;
}
