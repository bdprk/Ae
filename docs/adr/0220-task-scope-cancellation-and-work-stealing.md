# ADR-0220: Task Scope Join, Cancellation Tokens, and Multiworker Work-Stealing (M4.2.04, M4.2.05, M4.2.07)

Status: Accepted (2026-11-20)

## Context

With M4.2.01–M4.2.03 (ADR-0218) and M4.2.06 (ADR-0219) landed, three items remain to complete M4.2 Tasks:

- **M4.2.04 (Scope join)**: leaving a task scope with unfinished children must join/drain them according to the scope contract.
- **M4.2.05 (Cancellation points)**: cancelling before park, during park, and after completion must produce one terminal outcome.
- **M4.2.07 (Multiworker stealing)**: stealing work across multiple worker threads under contention must execute each task exactly once.

## Decision

1. **Structured task scope (`task_scope`, M4.2.04)**:
   - `task_scope` is an **owned** type (like `guard`), released automatically on every scope exit (normal exit, early `return`, `break`, `continue`) via `Frees` / `release_var`.
   - `scope_new() -> task_scope` creates a scoped executor; `scope_spawn(sc, || -> i64 { ... })` enqueues a child task onto `sc` without moving `sc`.
   - When `sc` leaves scope, `ae_scope_release(&sc)` drains all unfinished child tasks to completion and frees the scope state.
2. **Cancellation token (`cancel`, M4.2.05)**:
   - `cancel` is a copy-semantics handle to an atomic cancellation state (`0 = active`, `1 = cancelled`, `2 = completed`).
   - `cancel_new() -> cancel`, `cancel_trigger(tok) -> bool` (transitions `0 -> 1`, returning `true` iff this call triggered cancellation), `cancel_complete(tok) -> bool` (transitions `0 -> 2`, returning `true` iff not already cancelled/completed), `cancel_is_set(tok) -> bool` (returns `true` iff state == `1`).
   - Guarantees exactly one terminal outcome whether cancelled before park, during park, or after completion.
3. **Multiworker work-stealing pool (`pool`, M4.2.07)**:
   - `pool` is a copy-semantics handle to a work-stealing executor with `N` worker OS threads (`1 <= N <= 16`), each owning a local task queue; idle workers steal from peer queues in round-robin order.
   - `pool_new(workers) -> pool`, `pool_spawn(p, || -> i64 { ... })`, `pool_join(p) -> i64` (runs all workers until all queues are empty, joins all worker threads, frees task nodes, and returns the exact count of completed tasks).
   - In `interp.rs`, `pool_join` drains all queued tasks deterministically so compiled C and `--interp` agree on all deterministic workloads.

## Consequences

- Completes all seven items of M4.2 Tasks (M4.2.01–M4.2.07).
- All runtime primitives live inside the pruned `// unit: ae_exec` unit so programs that do not use tasks pay zero binary size or link cost.
