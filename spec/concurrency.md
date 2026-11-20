# Threads Specification (M4.1 subset, ADR-0214)

Status: describes the implemented language (2026-11-20). The surface, the runtime primitives, and the allocator atomics are implemented and verified (ADR-0214; the closure-only spawn surface and `wait` landed with this file's matrix T1–T8 pinned by tests). `spawn(f)` takes a closure value in this slice; a `fn() -> T` function value rides the same lowering and lands with the next evidence pass. Tasks, channels, and network I/O (M4.2–M4.4) get their own sections and ADRs; nothing here pre-commits them.

## 1. Design Rules

### Hard Rules:
1. **Owned Data Crosses, Nothing Else**: a thread boundary accepts owned values only. Views (`StrView`), borrows (`VecRef`, `StrVecRef`, `VecFRef`, `StructRef`, `StructMut`), and raw pointers are rejected at the `spawn` call site with a positioned diagnostic — the same proof that a borrow never outlives its frame (`borrow_view_escape`) is what makes the boundary safe.
2. **The Thread Owns Its Captures**: a spawned closure's value captures move at the `spawn` site, exactly like a plain call's move. After `spawn`, the outer variables are moved: reading or re-spawning them is the ordinary use-after-move rejection (E0102). There is no shared stack, no escaping borrow, no detach.
3. **One Result, One Join**: `wait(t)` blocks until the thread finishes and returns the spawned callable's return value as an owned value. `wait` consumes the handle (a second `wait` is use-after-move). The handle is an owned value like everything else; a dropped handle without `wait` leaks the thread's slot and shows up in a leak check. The name is `wait`, not `join`: `join` is the string-vector builtin's reserved name.
4. **Failure Propagates Through the Join**: a failure inside the thread (`ae_fail`) records itself on that thread's handle and ends only that thread; the process ends when the failure is observed — at the `wait` of that handle, after the join completes, or at process exit if the handle is never waited. The stderr line (`ae:<line>:<column>: <message>`) and the exit status (1) are the same an immediate abort produced. There is no panic, no unwinding, no catch (ADR-0217).

### Non-Goals (this version):
- No scoped borrows (`thread::scope`), no `Send`/`Sync` marker traits, no mutex/atomics/`Arc`, no channels, no worker pools, no detached threads in safe code.
- `spawn` takes no explicit arguments: the data a thread needs travels in its closure's captures. A spawned callable takes **zero visible parameters** (a function value must be `fn() -> T`).

## 2. Surface

```
let t = spawn(|| -> i64 { return total + 1; });   // owned captures move in
let value = join(t);                               // blocks; returns owned i64
```

- `spawn(f)` takes one argument: a closure value or a `fn() -> T` function value. The result type is the thread handle (spelled `Thread` in diagnostics); the handle records the return type internally, so `join` on it yields that type.
- `wait(t) -> T`: the return type comes from the spawned callable's return annotation, not from `wait`'s call site — two threads returning different types have different handles and the checker keeps them apart.
- `spawn`/`wait` are reserved names (the builtin namespace, like `find`/`sort`); they cannot be shadowed.
- Both forms are ordinary expressions: `spawn` in any expression position, `wait` wherever an expression is allowed. Neither is `unsafe`.

## 3. Capture Transfer

- The spawn-site classification reuses the closure's inferred captures: a **value capture** moves its outer variable into the thread (the closure is consumed by the spawn — calling it afterwards is use-after-move); a **borrow capture** (`string` view, `&vec`, `&Struct`, tokens cursor) is rejected:
  `spawn moves its captures into the thread; 'name' is captured by reference and cannot cross`.
- Mutating a captured container or field stays rejected where the closure is written (the closure's own rule); spawn adds nothing permissive.
- A capture of borrowable type that the body uses as owned (the owned probe's outcome) is a value capture and moves ✓. A pointer capture (`*const T`) in an `unsafe`-constructed closure carries the spec/unsafe.md 2.4 promise across the boundary; safe code cannot construct one.

## 4. Interaction with Ownership

- The handle moves like any owned value: into structs, enums, or other closures (a closure capturing a handle value-captures it; spawning it again moves the handle on).
- `wait` inside a loop is legal: each iteration spawns and waits on its own handle.
- Whole-enum `==`, `print` of a handle, and moving the same handle twice are the ordinary owned-value rules, not special cases.

## 5. Runtime Contract (ADR-0214, implemented)

- `spawn` lowers to `ae_thread_spawn` (C11 `thrd_create` on Linux, Win32 `CreateThread` on Windows — one spelling per platform, behind the pruned `ae_thread_entry_fn` unit); `wait` lowers to `ae_thread_join`. The thread's return value rides the heap context's first member, packaged with the captures; `wait` reads it after the platform's join provides the happens-before edge, then releases the slot and context.
- The allocator's budget accounting is atomic (charge-and-rollback): `AE_ALLOC_LIMIT` stays exact under concurrency, and a substituted allocator must be thread-safe by contract (installed before any spawn).
- No thread usage, no thread symbols in the binary (charter rule 8): the unit is pruned like the allocator's.

## 6. Diagnostics Summary

Rejections are compile-time: borrow-capture crossing (§3), non-zero visible parameters (§1 non-goal), non-callable spawn argument, use-after-move of the handle or of moved captures (E0102). Human-readable messages are not stable identifiers; no dedicated E-codes beyond the existing move rules.

## 7. Test Matrix

| # | Case | Expected |
|---|---|---|
| T1 | Spawn a `||` closure with one owned string capture, join, print | Runs; the capture moved; `--leak` clean |
| T2 | Spawn with a borrow capture (`slice_view` of an outer string) | Rejected (capture by reference) |
| T3 | Spawn a closure with a visible parameter | Rejected (zero-parameter rule) |
| T4 | `spawn(non_callable)` | Rejected |
| T5 | Read a value-captured variable after `spawn` | Rejected E0102 (use after move) |
| T6 | `wait` the same handle twice | Rejected E0102 |
| T7 | Two threads with different return types; both joined | Both compile; types kept apart |
| T8 | Spawn in a loop, wait each iteration | Runs; every value released once (`--leak` clean) |
| T9 | Thread does heavy allocation under `AE_ALLOC_LIMIT` | Budget accounting exact; limit trips at most one request |
| T10 | Failure inside the thread | The failing thread stops; waiting its handle reports the child's diagnostic and exits 1 (`tests/run/thread_failure.ae`) |

Evidence anchors: `scripts/thread_harness.c` (runtime layer, dual platform), the spawn slice's UI/execution fixtures (language layer), `bench/results/2026-11-20-linux-threads/`.

## 8. Atomics (M4.1.04)

Status: target for the atomics slice — this section governs the implementation that follows; the ground rules are ADR-0214's.

### 8.1 Design Rules

1. **The Cell Is Shared by Copying the Handle**: the `atomic` type's value is a handle to one process-lifetime cell (`i64`). Copying the handle — assignment, a by-value argument, a spawn capture — copies the pointer, so every copy reaches the same cell. There is no explicit clone and no refcount: the cell lives for the whole program and a leak check reports the retained cells as a documented baseline (the same honesty as the argv cache).
2. **Copy, Never Borrow**: `atomic` (the type spelling) is a copy-semantics type like the scalars — it never moves, never borrows, and no operation takes it by reference. The borrow checker's move/borrow tracking does not apply to it.
3. **Sequentially Consistent, Always**: every operation is C11 seq_cst (`v1` has no ordering parameter). A store in one thread followed by a load in another that observed it is the release/acquire handoff M4.1.04 requires; weaker orderings are future work and would need their own evidence.
4. **i64 Cells Only (v1)**: `cell_new(v) -> atomic` seeds a cell; `cell_load(a) -> i64`, `cell_store(a, v)`, `cell_add(a, v) -> i64` (returns the previous value), and `cell_cas(a, expect, new) -> bool` are the whole surface. The names say `cell` because `std/core/atomic.ae` — the self-hosted M1.4 model of the future standard-library surface — already owns the `atomic_` prefix. All five are reserved names.
5. **Spawn Captures Share**: the owned-only boundary admits `atomic` captures because copying the handle into the thread's context is the sharing. Two threads capturing one handle increment the same cell.

### 8.2 Test Matrix

| # | Case | Expected |
|---|---|---|
| A1 | One cell, two threads each add N times, print the final load | Exactly 2N additions; deterministic total |
| A2 | `atomic_add`'s return values across threads | Each old value observed exactly once (no lost update) |
| A3 | Store in a thread, load in `main` after `wait` | The store is visible (the handoff) |
| A4 | Copying the handle to a second name | Both names reach one cell |
| A5 | Cell count in a leak check | Retained cells reported as the documented baseline |

## 9. Shared Boxes (`shared[T]`, ADR-0216)

1. **`shared[T]` is a copy-semantics handle to one process-lifetime box.** `shared_new(v)` allocates a heap box holding `v` and returns a `shared[T]` handle. Copying the handle — including capturing it in `spawn` closures or passing it to functions — shares the same box without moving the caller's binding.
2. **No handle towers.** `T` cannot be another `shared[...]` handle (`shared_new(shared_new(x))` and `shared[shared[T]]` are rejected at compile time). `shared[atomic]` is permitted.
3. **Reads copy out (v1).** `shared_get(s) -> T` returns a copy of the stored value and is admitted in v1 for copy-semantics `T` (`i64`, `bool`, `f64`, `atomic`).
4. **Leak baseline.** Because shared boxes have process lifetime (no reference counting), a leak check reports one retained allocation per `shared_new` call as the documented baseline.

| Case | Scenario | Expected |
|---|---|---|
| S1 | Two threads read one `shared[i64]` box | Both threads and main observe identical values; compiled and `--interp` agree (`tests/run/shared_box.ae`) |
| S2 | Two threads share `shared[atomic]` and increment via `cell_add` | Final `cell_load` equals initial value plus both deltas (`tests/run/shared_box.ae`) |
| S3 | Handle tower `shared_new(shared_new(1))` | Compile-time error (`tests/ui/shared_tower.ae`) |
| S4 | Non-copy `T` in v1 (`shared_new("hello")`) | Compile-time error (`tests/ui/shared_non_copy.ae`) |
| S5 | `shared_get` on non-shared value | Compile-time error (`tests/ui/shared_get_non_handle.ae`) |

### 9.1 Mutexes, Guards, and `shared_set` (M4.1.03)

1. **`mutex` is a copy-semantics handle to one process-lifetime OS mutex.** `mutex_new()` allocates and initializes an OS mutex (`CreateMutexA` on Windows, `pthread_mutex_t` on POSIX). Copying the handle (including into `spawn` closures) shares the same mutex.
2. **`mutex_lock(m)` returns an owned `guard`.** A `guard` is non-copyable and unlocks the mutex automatically on every scope exit (normal exit, early `return`, `break`, `continue`) via the standard `Frees` release path. Guards cannot be captured by `spawn`.
3. **`shared_set(s, v)` requires a live `guard` in scope.** Calling `shared_set` without a live, unmoved `guard` dominating the statement is rejected at compile time.

| Case | Scenario | Expected |
|---|---|---|
| M1 | Early return inside a locked function + two-thread 500× counter increment | No deadlock; deterministic final count `1010`; compiled and `--interp` agree (`tests/run/mutex_guard.ae`) |
| M2 | `shared_set` without a live guard | Compile-time error (`tests/ui/mutex_unpaired_set.ae`) |
| M3 | `shared_set` after guard scope exited | Compile-time error (`tests/ui/mutex_set_after_scope.ae`) |
| M4 | `guard` captured by `spawn` | Compile-time error (`tests/ui/mutex_guard_escape_spawn.ae`) |
| M5 | `mutex_lock` on non-mutex value | Compile-time error (`tests/ui/mutex_lock_non_mutex.ae`) |

## 10. Channels (`chan`, M4.3.01–M4.3.06)

1. **`chan` is a copy-semantics handle to one process-lifetime i64 channel.** `chan_new(cap)` creates a rendezvous channel (`cap == 0`) or a bounded ring-buffer channel (`cap > 0`). Copying the handle (including into `spawn` closures) shares the channel.
2. **`chan_send(ch, v) -> bool` blocks when full (or until a receiver arrives for `cap == 0`).** Returns `true` when delivered/buffered, or `false` if the channel is closed or closed while the sender is parked.
3. **`chan_recv(ch) -> i64` blocks when empty.** After `chan_close(ch);`, buffered values drain in FIFO order; once empty, `chan_recv` returns `0` without blocking.
4. **`chan_close(ch);` marks the channel closed and wakes all parked senders and receivers.**

| Case | Scenario | Expected |
|---|---|---|
| C1 | Rendezvous (`cap == 0`) handoff across threads | Receiver gets `42`; sender returns `true` (`tests/run/channels.ae`) |
| C2 | Bounded buffer (`cap == 1`) backpressure + resume | First send succeeds; second send resumes after first recv (`tests/run/channels.ae`) |
| C3 | Close with buffered values | Buffered values drain FIFO (`100`, `200`), then `0`; later send returns `false` (`tests/run/channels.ae`) |
| C4 | `chan_send` / `chan_close` on non-chan value | Compile-time errors (`tests/ui/chan_send_non_chan.ae`, `tests/ui/chan_close_non_chan.ae`) |
| C5 | `chan_select(c1, c2)` with both arms ready, then woken by spawned sender + `chan_try_send` backpressure | Chooses once (`0`), then (`1`), then (`1` with `77`); `chan_try_send` returns `false` when full (`tests/run/chan_select.ae`) |
| C6 | `chan_select` on non-chan value | Compile-time error (`tests/ui/chan_select_non_chan.ae`) |

## 11. Receiver cancellation (M4.3.05)

Cancellation has no dedicated primitive: a **cancellation channel** is an ordinary `chan` that only ever closes. Closing it makes the arm ready forever, so a receiver waiting on it wakes even when no value arrives.

1. **`chan_close(stop);` is the cancellation signal.** Any `chan_select` arm over `stop` becomes ready; nothing is sent, so the channel carries no payload.
2. **A closed arm wins the select.** When one arm is closed and the other merely holds a value (a buffered element, or a rendezvous sender parked with its slot), `chan_select` returns the closed arm. Among two closed arms the earlier one wins. This rule is what makes cancellation lossless.
3. **A cancelled receiver consumes nothing.** `chan_select` itself never receives; the caller receives only after the select reports the data arm. A receiver that sees the cancel arm returns and leaves the value in place, so the next receiver gets it — no duplicate delivery, no lost wakeup.
4. **A parked receiver is cancelled within the poll interval.** A receiver parked in `chan_select` re-checks both arms about every millisecond, so `chan_close(stop)` from another thread releases it shortly after. There is no separate waiter registry, so a waiter cannot outlive a cancellation in this implementation.
5. **A parked rendezvous sender keeps its slot.** Cancelling every receiver leaves `chan_send` parked; the value moves only when a live receiver arrives, or the data channel closes and the sender returns `false`.

| Case | Scenario | Expected |
|---|---|---|
| C7 | A buffered value and a closed cancel arm are ready together, with the cancel arm listed second | `chan_select(ch, stop)` returns `1` and the following `chan_recv(ch)` still returns the buffered value — the closed arm wins without consuming (`tests/run/chan_cancel.ae`) |
| C8 | A rendezvous sender parks with its value while the receiver's cancel arm is already closed | Select returns the cancel arm (`1`), a second receiver's `chan_recv` gets `7`, and the sender returns `true` (`tests/run/chan_cancel.ae`) |
| C9 | A receiver parked in `chan_select` is cancelled by another thread, then a value is sent | The parked receiver wakes on the cancel arm (`1`), and the next receiver takes `9` (`tests/run/chan_cancel.ae`) |

## 12. Thread failure and propagation (M4.1.06)

1. **A failure stops its thread.** Inside a spawned body `ae_fail` (division by zero, overflow, bounds, out of memory, an explicit failure) records `{message, line, column}` on that thread's handle and ends the thread. Nothing unwinds across the thread boundary and nothing is caught.
2. **The join reports it.** `wait` joins first and reports after: a failed handle prints `ae:<line>:<column>: <message>` and ends the process with status 1. Output printed before the wait is flushed and survives.
3. **The order of waits is the policy.** Siblings joined before the failing handle run to completion; waiting the failed handle first ends the process at that wait. The runtime does not join siblings on the program's behalf (there is no runtime scope object).
4. **Unwaited failures surface at exit.** The thread unit latches the most recent failure and an `atexit` hook reports it if the process reaches exit without a wait having seen it. This is best effort: a failure that lands after `main` returned cannot be observed.
5. **The main thread is unchanged.** A failure outside a spawned body has no joiner and aborts immediately.
6. **The interpreter replays at the same point.** `spawn` stores a body's trap and `wait` re-raises it, so both backends print the same bytes and fail at the same wait; `exit` inside a thread leaves immediately in both.

| Case | Scenario | Expected |
|---|---|---|
| T11 | Sibling joined first, then the failed handle | The sibling's output survives (`sibling`, `7`), then `ae:<line>:<column>: division by zero` and exit 1 (`tests/run/thread_failure.ae`) |
| T12 | Same program under `--interp` | Identical stdout and the same failing wait (`a_failed_child_surfaces_at_its_wait_after_the_sibling_joined`) |
| T13 | Failure that is never waited | The `atexit` latch reports it and exits 1 when it is set before exit |
| T14 | Failure on the main thread | Immediate abort, unchanged: the diagnostic and exit 1 print at the failure point |

## 13. Tasks (`exec`, M4.2.01–M4.2.03, ADR-0218)

1. **`exec` is a copy-semantics handle to a single-worker deterministic FIFO task executor.** `exec_new()` allocates an executor; copying the handle shares the same queue.
2. **`task_spawn(ex, || -> i64 { ... })` enqueues a zero-parameter closure returning `i64`.** Value captures move at the `task_spawn` site (ordinary E0102 use-after-move afterwards); borrow captures are rejected at compile time.
3. **`task_yield()` suspends the running task and moves it to the tail of the FIFO ready queue.** All live locals survive across yields and the task resumes at the statement immediately after `task_yield()` with no duplicate execution. Outside a task, `task_yield()` is a no-op.
4. **`exec_run(ex) -> i64` runs all queued tasks to completion in deterministic FIFO order and returns the count of completed tasks.** Each task's stack and context are reclaimed immediately upon completion so spawning many tasks retains bounded memory.

| Case | Scenario | Expected |
|---|---|---|
| K1 | Three tasks enqueued on one executor without yields | Execute in exact FIFO submission order `1, 2, 3`; `exec_run` returns `3` (`tests/run/tasks_fifo.ae`) |
| K2 | Two tasks interleave via `task_yield()` while mutating locals | Locals stay intact across yields; exact interleaved FIFO output; no duplicate execution (`tests/run/tasks_fifo.ae`) |
| K3 | 200 tasks spawned and run to completion | `exec_run` returns `200`; stacks reclaimed on completion with bounded memory (`tests/run/tasks_fifo.ae`) |
| K4 | `task_spawn` with non-`exec` first argument | Compile-time error (`tests/ui/task_spawn_non_exec.ae`) |
| K5 | `exec_run` with non-`exec` argument | Compile-time error (`tests/ui/exec_run_non_exec.ae`) |

### 13.1 Structured Task Scope (`task_scope`, M4.2.04, ADR-0220)

1. **`task_scope` is an owned handle** created by `scope_new()`. `scope_spawn(sc, || -> i64 { ... })` enqueues child tasks onto `sc` without consuming `sc`.
2. **Automatic join on scope exit**: leaving the scope — whether by normal fallthrough or early `return` — drains all unfinished child tasks to completion before continuing.

### 13.2 Cancellation Points (`cancel`, M4.2.05, ADR-0220)

1. **`cancel` is a copy-semantics atomic cancellation token** created by `cancel_new()`.
2. **`cancel_trigger(tok) -> bool`** and **`cancel_complete(tok) -> bool`** race atomically for one terminal outcome (`cancelled` vs `completed`); at most one succeeds. `cancel_is_set(tok) -> bool` observes whether cancellation won.

### 13.3 Multiworker Work-Stealing Pool (`pool`, M4.2.07, ADR-0220)

1. **`pool_new(workers) -> pool`** creates a multi-worker stealing pool (`1 <= workers <= 16`). `pool_spawn(p, || -> i64 { ... })` enqueues tasks across worker queues; `pool_join(p) -> i64` runs workers with cross-queue stealing until all tasks finish and returns the exact count of completed tasks.
2. **Exactly-once execution**: under multi-worker stealing contention, every spawned task runs exactly once (`tests/run/tasks_advanced.ae`).

## 14. Controlled Clocks and Timers (`clock`, `timer`, M3.2.05 / M4.2.06, ADR-0219)

1. **`clock` is a copy-semantics handle to a controlled monotonic clock starting at `0`.** `clock_advance(clk, delta)` advances time monotonically (`delta >= 0`) and returns the new timestamp; `clock_now(clk)` reads it.
2. **`timer` is a copy-semantics handle to a deadline bound to a `clock`.** `timer_new(clk, delay)` sets deadline `clock_now(clk) + delay`. `timer_ready(t)` returns `true` once `clock_now(clk) >= deadline` unless cancelled.
3. **`timer_cancel(t) -> bool`** cancels a pending timer and returns `true` if it was still pending (not already cancelled and not already ready), `false` otherwise.

| Case | Scenario | Expected |
|---|---|---|
| L1 | Two timers with equal deadlines on one controlled clock | Both stay unready before deadline, both become ready together at exact deadline (`tests/run/timers_clock.ae`) |
| L2 | Cancel one timer before deadline, cancel another after deadline | Pre-deadline cancel returns `true` and keeps `timer_ready` `false`; post-deadline cancel returns `false` (`tests/run/timers_clock.ae`) |
| L3 | Task executor drives a timeout loop using `clock_advance` and `task_yield` | Waiting task wakes cleanly when controller task advances the clock (`tests/run/timers_clock.ae`) |
| L4 | `clock_advance` on non-`clock` or `timer_ready` on non-`timer` | Compile-time error (`tests/ui/timer_non_handle.ae`) |
