# ADR-0217: Thread Failure Propagation (Deferred to the Join)

- Status: Implemented (dual platform)
- Date: 2026-11-20
- Associated Milestone: M4.1.06 (see `docs/plan.md` §6)
- Depends on: ADR-0214 (thread ground rules — this ADR replaces its rule 4, "failure aborts the process")

## Context

ADR-0214 rule 4 made any `ae_fail` inside a thread end the whole process on the spot: `exit(1)` from whichever thread hit the failure. That is safe but it throws away work the program can still finish — the sibling threads, and any buffered output — and it makes a failing child indistinguishable from a crash of the parent. M4.1.06 asks for the failure to be *propagated*: the child's failure should reach the program through the handle, after the siblings that were already joined have run.

The language has no panic values, no unwinding, and no `catch`. A failure is a runtime diagnostic plus a nonzero exit; the only open question is *when* the process ends and *who* decides.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| A. Keep the immediate abort | Simplest; already pinned by T10 | A failing child discards the parent's unfinished work and its siblings' output; "propagation" is indistinguishable from a crash | Runtime: none |
| B. Defer to the join (this ADR): the failure is recorded on the handle, the thread ends, and the joiner reports it after the join completes | Siblings joined first finish; the diagnostic stays the child's (`ae:line:col: message`); no unwinding, no catch, no panic type | A wait on the failed handle ends the process, so siblings not yet joined are cut short; an unwaited child can only be reported at process exit, which races with `main` returning | Runtime: one record per handle, one thread-local pointer, one `atexit` hook |
| C. Turn the failure into a value (`Result`) that `wait` returns | Fully typed propagation | Changes `wait`'s type for every program; a failure would have to become part of every spawned callable's return type | Compile: surface change. Runtime: none |

## Decision

1. **A failure stops its thread, not the process.** Inside a spawned body, `ae_fail` writes `{message, line, column}` into that thread's handle record and ends the thread (`thrd_exit` / `ExitThread`). The child's stack is abandoned; there is no cross-thread unwinding and no catch.
2. **The joiner reports it.** After `ae_thread_join` returns (the join completed, so every sibling joined before this point already ran), a failed record prints the same stderr line an immediate abort prints — `ae:<line>:<column>: <message>` — and ends the process with status 1. Flushing happens before the exit, so output the parent printed earlier survives.
3. **The main thread is unchanged.** A failure outside a spawned body (including inside `wait`'s own checks) has no joiner, so it aborts immediately exactly as before.
4. **Unwaited failures are still reported.** The most recent failure is latched process-wide and an `atexit` hook that the thread unit installs on the first spawn reports it if the program reaches exit without a wait having seen it. This is best effort by construction: a child that fails after `main` already returned cannot be reported, and the latch only remembers the most recent failure.
5. **The order of waits is the policy.** Joining siblings first, then the failed handle, lets the siblings finish; joining the failed handle first ends the process at once. There is no runtime scope object, so the runtime cannot join siblings on the program's behalf — that ordering rule is the "scope policy" the checklist asks for, and ADR-0215's scoped form already forces every handle to be joined inside the region.
6. **The interpreter agrees at the program point.** `spawn` stores the body's trap and `wait` re-raises it, so a program prints the same bytes and fails at the same wait in both backends. Process-wide outcomes (`exit`) still leave immediately in both.

## Evidence

- `tests/run/thread_failure.ae` (snapshot, exit code 1): the sibling joined first prints and returns; the wait on the failed handle prints `ae:7:61: division by zero` and exits 1, and the statement after it never runs.
- `a_failed_child_surfaces_at_its_wait_after_the_sibling_joined` (`tests/execution.rs`): stdout is exactly `sibling\n7\n`, the exit status is 1, the diagnostic keeps the `ae:` prefix and the child's message, and `--interp` prints the same stdout with the same status.
- Dual platform: Windows x86-64 and evaluate-2 Linux x86-64, full suite green on both, `verify_generated_c.py` (sanitize and leak) and the 40-seed differential fuzz clean; logs in `bench/results/2026-11-20-linux-thread-failure/`.

## Consequences

- Claims: the concurrency chapter may now claim "a child failure is reported through its join" instead of "a child failure is a process crash". T10's guarantee (nonzero exit) is preserved; only the moment changes.
- The child's thread-local storage and its heap allocations are not reclaimed when it fails — the process is already on its way out, and the failure path is not covered by the leak budget.
- Revisit if a panic/catch model (typed failures crossing threads) is ever added: option C is the shape that model would take, and this ADR's latch and sink indirection would be deleted with it.
- The `atexit` hook is installed only by the thread unit, so a program that never spawns carries no failure machinery at all (charter rule 8).
