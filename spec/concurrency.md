# Threads Specification (M4.1 subset, ADR-0214)

Status: describes the implemented language (2026-11-20). The surface, the runtime primitives, and the allocator atomics are implemented and verified (ADR-0214; the closure-only spawn surface and `wait` landed with this file's matrix T1–T8 pinned by tests). `spawn(f)` takes a closure value in this slice; a `fn() -> T` function value rides the same lowering and lands with the next evidence pass. Tasks, channels, and network I/O (M4.2–M4.4) get their own sections and ADRs; nothing here pre-commits them.

## 1. Design Rules

### Hard Rules:
1. **Owned Data Crosses, Nothing Else**: a thread boundary accepts owned values only. Views (`StrView`), borrows (`VecRef`, `StrVecRef`, `VecFRef`, `StructRef`, `StructMut`), and raw pointers are rejected at the `spawn` call site with a positioned diagnostic — the same proof that a borrow never outlives its frame (`borrow_view_escape`) is what makes the boundary safe.
2. **The Thread Owns Its Captures**: a spawned closure's value captures move at the `spawn` site, exactly like a plain call's move. After `spawn`, the outer variables are moved: reading or re-spawning them is the ordinary use-after-move rejection (E0102). There is no shared stack, no escaping borrow, no detach.
3. **One Result, One Join**: `wait(t)` blocks until the thread finishes and returns the spawned callable's return value as an owned value. `wait` consumes the handle (a second `wait` is use-after-move). The handle is an owned value like everything else; a dropped handle without `wait` leaks the thread's slot and shows up in a leak check. The name is `wait`, not `join`: `join` is the string-vector builtin's reserved name.
4. **Failure Aborts**: a failure inside the thread (`ae_fail`) terminates the process. There is no panic, no unwinding, no catch — thread failure semantics are whole-process by definition until a panic model exists (M4.1.06 is blocked on that ADR).

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
| T10 | Failure inside the thread | Process exits nonzero (abort contract) |

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
