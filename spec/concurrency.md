# Threads Specification (M4.1 subset, ADR-0214)

Status: target for the M4.1 spawn slice — every rule below is enforced once the surface lands; the runtime primitives and the allocator atomics they stand on are implemented and verified (ADR-0214, decisions 1–2, `99464a4`). Tasks, channels, and network I/O (M4.2–M4.4) get their own sections and ADRs; nothing here pre-commits them.

## 1. Design Rules

### Hard Rules:
1. **Owned Data Crosses, Nothing Else**: a thread boundary accepts owned values only. Views (`StrView`), borrows (`VecRef`, `StrVecRef`, `VecFRef`, `StructRef`, `StructMut`), and raw pointers are rejected at the `spawn` call site with a positioned diagnostic — the same proof that a borrow never outlives its frame (`borrow_view_escape`) is what makes the boundary safe.
2. **The Thread Owns Its Captures**: a spawned closure's value captures move at the `spawn` site, exactly like a plain call's move. After `spawn`, the outer variables are moved: reading or re-spawning them is the ordinary use-after-move rejection (E0102). There is no shared stack, no escaping borrow, no detach.
3. **One Result, One Join**: `join(t)` blocks until the thread finishes and returns the spawned callable's return value as an owned value. `join` consumes the handle (a second `join` is use-after-move). The handle is an owned value like everything else; a dropped handle without `join` leaks the thread's slot and shows up in a leak check.
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
- `join(t) -> T`: the return type comes from the spawned callable's return annotation, not from `join`'s call site — two threads returning different types have different handles and the checker keeps them apart.
- `spawn`/`join` are reserved names (the builtin namespace, like `find`/`sort`); they cannot be shadowed.
- Both forms are ordinary expressions: `spawn` in any expression position, `join` wherever an expression is allowed. Neither is `unsafe`.

## 3. Capture Transfer

- The spawn-site classification reuses the closure's inferred captures: a **value capture** moves its outer variable into the thread (the closure is consumed by the spawn — calling it afterwards is use-after-move); a **borrow capture** (`string` view, `&vec`, `&Struct`, tokens cursor) is rejected:
  `spawn moves its captures into the thread; 'name' is captured by reference and cannot cross`.
- Mutating a captured container or field stays rejected where the closure is written (the closure's own rule); spawn adds nothing permissive.
- A capture of borrowable type that the body uses as owned (the owned probe's outcome) is a value capture and moves ✓. A pointer capture (`*const T`) in an `unsafe`-constructed closure carries the spec/unsafe.md 2.4 promise across the boundary; safe code cannot construct one.

## 4. Interaction with Ownership

- The handle moves like any owned value: into structs, enums, or other closures (a closure capturing a handle value-captures it; spawning it again moves the handle on).
- `join` inside a loop is legal: each iteration spawns and joins its own handle.
- Whole-enum `==`, `print` of a handle, and moving the same handle twice are the ordinary owned-value rules, not special cases.

## 5. Runtime Contract (ADR-0214, implemented)

- `spawn` lowers to `ae_thread_spawn` (C11 `thrd_create` on Linux, Win32 `CreateThread` on Windows — one spelling per platform, behind the pruned `ae_thread` unit); `join` lowers to `ae_thread_join`. The thread's return value rides a heap slot packaged with the captures; `join` reads it after the platform's join provides the happens-before edge, then releases the slot.
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
| T6 | `join` the same handle twice | Rejected E0102 |
| T7 | Two threads with different return types; both joined | Both compile; types kept apart |
| T8 | Spawn in a loop, join each iteration | Runs; every value released once (`--leak` clean) |
| T9 | Thread does heavy allocation under `AE_ALLOC_LIMIT` | Budget accounting exact; limit trips at most one request |
| T10 | Failure inside the thread | Process exits nonzero (abort contract) |

Evidence anchors: `scripts/thread_harness.c` (runtime layer, dual platform), the spawn slice's UI/execution fixtures (language layer), `bench/results/2026-11-20-linux-threads/`.
