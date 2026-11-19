// Thread + allocator accounting stress used by scripts/test_thread_harness.py
// (ADR-0214 decisions 1 and 2). The harness compiles the runtime prefix
// directly, spawns several OS threads through the same primitives the
// language layer will lower to, drives every allocation through the runtime's
// checked entry points, and asserts the cumulative-ask counter lands on the
// exact deterministic total the arithmetic predicts. A data race in the
// budget accounting shows up as a wrong total or a spurious out-of-memory;
// a lost join shows up as a hang the runner times out.
// runtime.c first: it carries the system includes, and a later system include
// on Windows would pull <windows.h> after the runtime's win32 declarations and
// conflict with them.
#include "../crates/ae-compiler/src/runtime.c"

#define THREADS 8
#define CYCLES 4000

// Deterministic per-cycle sizes, mirrored by expected_total() below: each
// cycle asks for `size` once (the initial alloc) and for `2 * size` again
// (the resize charges the whole new size), and frees both.
static size_t cycle_size(long id, int i) {
    return 16 + (size_t)((id * 37 + i * 13) % 512);
}

static unsigned long long expected_total(void) {
    // Each spawn itself asks for the job block (entry + ctx) and the handle;
    // the join frees only the handle, and the thread body frees the job, so
    // the cumulative-ask counter keeps every byte the run requested.
    unsigned long long total =
        (unsigned long long)THREADS * (2 * sizeof(void *) + sizeof(ae_thread_handle));
    for (long id = 0; id < THREADS; id++) {
        for (int i = 0; i < CYCLES; i++) {
            unsigned long long size = (unsigned long long)cycle_size(id, i);
            total += size + 2 * size;
        }
    }
    return total;
}

static void worker(void *raw) {
    long id = (long)(intptr_t)raw;
    for (int i = 0; i < CYCLES; i++) {
        size_t size = cycle_size(id, i);
        char *block = ae_alloc(size, 1, 1);
        block[0] = (char)i;
        block[size - 1] = (char)i;
        block = ae_alloc_resize(block, size * 2, 1, 1);
        block[2 * size - 1] = (char)i;
        ae_free(block);
    }
}

int main(void) {
    void *handles[THREADS];
    for (long id = 0; id < THREADS; id++) {
        handles[id] = ae_thread_spawn(worker, (void *)(intptr_t)id);
        if (!handles[id]) {
            fprintf(stderr, "spawn failed for %ld\n", id);
            return 2;
        }
    }
    for (int id = 0; id < THREADS; id++) {
        if (!ae_thread_join(handles[id])) {
            fprintf(stderr, "join failed for %d\n", id);
            return 3;
        }
    }
    unsigned long long expected = expected_total();
    unsigned long long actual = (unsigned long long)atomic_load(&ae_alloc_total);
    if (actual != expected) {
        fprintf(stderr, "total mismatch: expected %llu, got %llu\n", expected, actual);
        return 4;
    }
    printf("OK threads=%d cycles=%d total=%llu\n", THREADS, CYCLES, actual);
    return 0;
}
