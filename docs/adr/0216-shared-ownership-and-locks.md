# ADR-0216: Shared Ownership and Mutual Exclusion (M4.1.03 / M4.1.05)

- Status: Proposed (design) — the implementation follows in slices, each carrying its own evidence
- Date: 2026-11-20
- Associated Milestone: M4.1.03 (mutex guards) and M4.1.05 (shared owner)
- Depends on: ADR-0214 (threaded execution ground rules), ADR-0215 (scoped borrow captures), and the charter's error rule ("panic is only for invariant violations and terminates by default")

## Context

The thread surface (ADR-0214) moves data and shares `atomic` cells. Two charter shapes remain out of reach:

- **Shared compound state**: an `atomic` cell is one i64. Protecting a multi-field invariant — the reason mutexes exist — needs shared access to a compound value with ordered critical sections.
- **Read-only sharing of owned values**: handing several threads read access to one large value without cloning it.

Both require the same primitive: a copy-semantics handle to shared heap storage whose element access is guarded or shared-read. The design below adds that primitive and the lock that orders writes to it, and derives the guard rules from the language's existing scope-exit release machinery instead of introducing a panic model — the charter's terminating-panic decision (§2.2 rule 6, "panic ... terminates by default") makes unlock-on-panic moot: a panic kills the process, and the operating system releases every lock the dead process held.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| A. Copy-semantics `shared[T]` handle + `mutex` handle + lock-scoped guard value whose scope exit emits the unlock | Matches the atomic handle's established semantics; reuses the Frees release sites; no unwinding | Generic parameter on a builtin type is new to the type system; guard moves need care | Compile: one new builtin generic type + one owned guard type. Runtime: an OS mutex per mutex handle, a refcount-free process-lifetime box per shared handle |
| B. Lock/unlock as bare statements (no guard) | Smallest surface | An early return inside the section deadlocks on every later lock; the checker cannot pair them | Runtime: none extra |
| C. Full `Arc<T>` with reference counting and `Drop` at last owner | Matches Rust exactly | Requires the destructor model this language deliberately lacks (reference counting contradicts the "no runtime sentinels beyond moved sentinels" design note) | Runtime: refcount traffic on every copy |

## Decision (design; each rule lands with its slice's evidence)

1. **`shared[T]` is a copy-semantics handle to one process-lifetime box.** Copying the handle shares the box, exactly like `atomic`'s handle (spec/concurrency.md §8.1 rule 1). The box holds one owned `T`; it is allocated at `shared_new(value)` and lives for the whole program — a leak check reports retained boxes as the documented baseline. `T` may be any owned type except `atomic`/`shared`/`mutex` handles (no handle towers).
2. **Reads copy, writes lock.** `shared_get(s) -> T` returns a copy of the box's value and is only admitted for copy-semantics `T` (scalars, `atomic` handles, and — with the scoped rules below — nothing else in v1). Mutation runs inside a critical section: `mutex_lock(m)` returns a **guard**, and only while a guard of the matching mutex is live may the program call `shared_set(s, v)` on a box the mutex protects. Pairing is positional in v1: `mutex_new()` and the lock that guards a section must dominate the section's `shared_set` calls textually — the checker's dominator proof, reusing the M2.5 machinery.
3. **The guard unlocks at scope exit.** The guard is an owned, non-copyable value whose scope-exit release emits the mutex unlock — the same release-site machinery the thread slot and the moved sentinel use (Frees + drop elaboration). A guard passed to another function transfers the unlock duty; a guard stored in a structure is rejected (guards bind as locals only, like thread handles and pointers). Panics need no unlock path: a panic terminates the process and the OS reclaims everything.
4. **No reentrancy, no try-lock in v1.** Locking a mutex the calling thread already holds is undefined (deadlock); `try_lock` arrives with the timeouts slice (M4.4.05's deadline machinery).
5. **`shared_set` under the guard is the write; `shared_get` outside a section races** — the language states this the way `atomic_add`'s domain contract does: get-outside-a-section reads a value another thread may be replacing, and the documentation says so (charter rule 10: evidence-first claims).

## Implementation slices

- Slice 1 (M4.1.05): `shared[T]` handles + `shared_get` for copy-semantics `T` + the spawn-capture sharing rule (copying the handle into a thread shares the box) + the leak-baseline documentation. Evidence: two threads reading one box see identical bytes; the leak check reports the documented retained boxes.
- Slice 2 (M4.1.03): `mutex_new`/`mutex_lock` returning the guard, the scope-exit unlock, and the dominator proof pairing `shared_set` with a live guard. Evidence: the adversarial matrix — unlock on every exit path, a rejected unpaired `shared_set`, a rejected guard escape, and the two-thread counter race the mutex eliminates (the same determinism shape A1 uses).

## Required evidence per slice

- The compiled + interpreter agreement on the shared-state program (the interp serializes; the compiled backend races but the mutex makes the outcome deterministic).
- The adversarial matrix pinned as UI fixtures with positioned diagnostics.
- The leak baseline comparison (retained boxes + guards are the documented counts).
- Dual-platform per the standard pass.

## Consequences

- `spec/concurrency.md` gains §9 (shared boxes and locks) when slice 1 lands.
- M4.1.03 and M4.1.05 close with their acceptance tests; M4.1.06 stays blocked on a panic model decision the charter already constrains (terminate-by-default) — its "propagation" acceptance reduces to the abort contract ADR-0214 documents.
- The no-handle-towers rule keeps the type system flat: a shared box of atomics is expressible (`shared_new(atomic_new(0))`), a shared box of shared handles is not.
- Rollback: each slice is additive; reverting restores the ADR-0214 owned-only surface without touching the thread or atomic paths.
