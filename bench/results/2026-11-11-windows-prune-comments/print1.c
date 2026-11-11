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
ae_string ae_str_cat(ae_string a, ae_string b, size_t line, size_t column){
    if (a.len > INT64_MAX - b.len) { ae_free(a.ptr); ae_free(b.ptr); ae_fail("string too long", line, column); }
    char *out = ae_alloc((size_t)(a.len + b.len) + 1, line, column);
    memcpy(out, a.ptr, (size_t)a.len);
    memcpy(out + a.len, b.ptr, (size_t)b.len + 1);
    ae_free(a.ptr); ae_free(b.ptr);
    ae_string s = { out, a.len + b.len };
    return s;
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

int main(int argc, char **argv) {
    ae_argc_g = (int64_t)argc;
    ae_argv_g = argv;
    if (printf("%" PRId64 "\n", INT64_C(1)) < 0) ae_fail("output error", 0, 0);
    if (fflush(stdout) == EOF) return 1;
    return 0;
}
