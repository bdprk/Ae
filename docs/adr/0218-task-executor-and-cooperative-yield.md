# ADR-0218: Single-Worker Task Executor and Cooperative Yield (M4.2.01–M4.2.03)

Status: Accepted (2026-11-20)

## Context

M4.1 (OS threads, atomics, shared boxes, mutex guards, thread failure propagation) and M4.3 (channels, `chan_select`, cancellation) are implemented and verified. M4.2 requires lightweight cooperative tasks with:

- **M4.2.01**: A single-worker deterministic FIFO executor that replays ready tasks in submission order.
- **M4.2.02**: Stack lifecycle management that creates and reclaims many task stacks with bounded retained memory.
- **M4.2.03**: Cooperative `task_yield()` that suspends and resumes a task with all live locals intact and no duplicate execution.

Per the ae Charter:
- **No function coloring**: tasks run regular functions and closures without `async`/`await`.
- **Pay-as-you-go**: programs that do not use tasks must not link or emit task runtime symbols.
- **Dual-backend parity**: compiled C binaries and the MIR interpreter (`--interp`) must produce identical deterministic output.

## Decision

1. **Executor handle (`exec`)**: `exec` is a copy-semantics handle to a heap-allocated single-worker FIFO task queue, matching the handle pattern of `atomic`, `mutex`, and `chan`. Copying an `exec` handle (including into a task's closure) lets tasks enqueue follow-up tasks onto the same executor.
2. **Surface builtins** (all reserved names, ordinary expressions/statements, non-`unsafe`):
   - `exec_new() -> exec`: creates a new single-worker FIFO executor.
   - `task_spawn(ex, || -> i64 { ... })`: enqueues a zero-parameter closure returning `i64` onto `ex`. Captures follow the same owned-only transfer rules as `spawn` (value captures move at the `task_spawn` site; borrow captures are rejected at compile time).
   - `task_yield()`: statement that yields the currently running task to the tail of its executor's FIFO ready queue, preserving all live locals across suspension and resumption. Outside a running task, `task_yield()` is a no-op.
   - `exec_run(ex) -> i64`: drains the executor's FIFO ready queue on the calling thread until empty, reclaiming each task's stack and context immediately upon completion, and returns the total number of tasks completed in this run.
3. **Runtime implementation (`ae_task` unit in `runtime.c`)**:
   - **Windows**: uses Win32 Fibers (`ConvertThreadToFiber`, `CreateFiber` with 64 KiB stack, `SwitchToFiber`, `DeleteFiber`, `ConvertFiberToThread`), self-declared with `/* win32-declared */` so `<windows.h>` is not pulled in. Each completed task's fiber is deleted and its node freed before switching to the next task, guaranteeing bounded memory across arbitrarily many tasks.
   - **POSIX / Linux**: uses C11 `<threads.h>` (`thrd_t`, `mtx_t`, `cnd_t`) with deterministic baton-passing so `-std=c11` compiles cleanly without `_GNU_SOURCE` or platform-specific assembly, and each completed task thread is joined and freed immediately before advancing.
   - **Pruning**: the `ae_task` unit is pruned when a program does not reference `exec_new`/`task_spawn`/`task_yield`/`exec_run`.
4. **MIR interpreter (`interp.rs`)**:
   - Uses a deterministic FIFO baton-passing scheduler over Rust `std::thread` with `Mutex` + `Condvar`, giving byte-for-byte identical execution order to the C backend across arbitrary `task_yield()` points.

## Consequences

- M4.2.01 (deterministic FIFO replay), M4.2.02 (bounded stack reclamation), and M4.2.03 (live-local-preserving `task_yield()`) are all satisfied by one cohesive, zero-coloring design.
- Future slices (M4.2.04 task scope join, M4.2.05 cancellation points, M4.2.06 controlled-clock timers, M4.2.07 multi-worker work stealing) build directly on `exec` without breaking existing code.
