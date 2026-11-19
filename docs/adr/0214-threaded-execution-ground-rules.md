# ADR-0214: Threaded Execution Ground Rules

- Date: 2026-11-20
- Status: Accepted (design); implementation follows in M4.1 slices, each carrying its own evidence
- Scope: the ground decisions every M4.1 (OS threads) slice builds on — the runtime threading primitive, allocator thread-safety, the data that may cross a thread boundary, and the failure model. Tasks, channels, and network I/O (M4.2–M4.4) get their own ADRs; this one only fixes what they must all live with.

## Context

M4.1 is the first concurrency milestone and the language has none of its ground in place:

- The runtime's allocation accounting is a pair of unsynchronized globals (`ae_alloc_budget`, `ae_alloc_total`, `crates/ae-compiler/src/runtime.c` allocator unit, ADR-0016/0213): two threads allocating concurrently race the counter, and the budget check can pass while the true total exceeds the budget.
- The borrow system (ADR-0205) proves borrows never outlive their frame — `borrow_view_escape` rejects returning a view of local storage — but nothing yet states what happens when a thread boundary is that escape.
- `ae_fail` aborts the process; there is no panic, no unwinding, no catch. Thread failure semantics must say what that means per thread.
- The generated program is plain C (ADR-0203): a thread primitive must exist behind one runtime spelling per platform, the way `AE_PTR_READ/WRITE` is the one spelling for unchecked access (M2.4.03).
- `resource-model.md` §81 and `borrow-checker-design.md` Stage C explicitly defer `Send`/`Sync` definitions to the concurrency section; this ADR is that section's first part.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| A. OS threads behind runtime spellings; owned-data-only boundary for the first slice; atomics for the allocator counter | Sound by construction with the existing borrow checker; no new trait machinery; small runtime surface; scheduler-free | Scoped borrows (Rust `thread::scope`) deferred to a later ADR; shared read-only state across threads is not expressible yet | Compile: none. Runtime: one thread handle per spawn; atomic counter updates. Memory: stack per thread (OS default) |
| B. Full `Send`/`Sync` trait machinery with scoped borrows from day one | Matches Rust's power | Requires trait solving for marker traits the language has not built (M1.3 bounds exist but marker-derivation does not); scope-join needs borrow-graph regions that outlive statements (M2.5's NLL regions are per-function); months of checker work before a single thread runs | Compile: trait solver extension. Runtime: scheduler-grade join logic |
| C. Green threads / scheduler now (skip OS threads) | Matches Go's task model directly | Fires before M4.1's OS layer exists; stacks, parking, and preemption are M4.2 work that needs the M4.1 ownership rules first; mixes two designs into one ADR | Runtime: stack allocator, parking, work-stealing queue |

## Decision

1. **One runtime spelling per primitive.** `runtime.c`/`runtime_ex.c` gain `ae_thread_spawn(fn, ctx) -> handle` and `ae_thread_join(handle) -> exit code`, implemented with C11 `thrd_create`/`thrd_join` on Linux and Win32 `CreateThread`/`WaitForSingleObject` on Windows. Language-level `spawn` lowers to these; generated C never spells a platform thread API directly (the `AE_PTR_*` precedent).
2. **The allocator becomes thread-safe at the accounting, not the allocator.** `ae_os_alloc/resize/release` delegate to malloc/free, which are thread-safe on both platforms. The runtime's own state — the budget counter and total — becomes C11 atomics (`atomic_fetch_add`), so the budget contract (`AE_ALLOC_LIMIT`, ADR-0016) stays exact under concurrency: the limit is enforced per request against an up-to-date total, and a program may see the limit trip marginally earlier than serial execution would. The pluggable allocator contract (ADR-0016's `ae_set_allocator`, C-ABI-level) gains one documented precondition: an allocator installed before any spawn must be thread-safe; safe code cannot install one, and the runtime does not serialize calls into it.
3. **The thread boundary accepts owned data only.** The first `spawn` surface moves its arguments: every value crossing the boundary must be owned (`i64`, `bool`, `f64`, `string`, `vec`, `vec[string]`, `vec[f64]`, arrays, structs, enums, `Fn`/closure values that themselves own their captures). Views (`StrView`), borrows (`VecRef`, `StructRef`, `StructMut`), and raw pointers are rejected at the spawn call site with a positioned diagnostic. This is a checker rule, not a trait: it reuses the existing type-directed move checks, and the borrow checker already guarantees nothing borrowed can reach the call (borrows cannot escape a frame — the same rule `borrow_view_escape` enforces). `Send`/`Sync` as named markers stay deferred until scoped borrows (M4.1.02's ADR) or interior mutability make them non-vacuous.
4. **A thread that fails aborts the process.** There is no unwinding: `ae_fail` inside a thread calls the same abort path as `main`. Failure semantics per thread are therefore "the process dies" until a panic model exists (M4.1.06 stays blocked on a panic ADR; it is not scheduled under this one).
5. **Join is the only completion signal.** The first surface is `join(handle) -> owned return value`; detached threads are not expressible in safe code (the runtime keeps the handle; leaking a handle is the program's own leak, visible to `--leak` as a retained block). Exit-at-end semantics: `exit` while other threads run aborts them — a documented, honest whole-process contract, not a graceful drain.
6. **Raw pointers cross as an unsafe promise.** Under `unsafe`, passing a raw pointer to a thread is the same contract ADR-0207 / spec/unsafe.md 2.4 states for reads: the caller promises the pointee is live, aligned, and initialized for the pointee type for the thread's whole use. Nothing in safe code can construct that situation except through `unsafe` blocks, so the promise boundary is visible at the `unsafe` block.

## Alternatives rejected

- Per-thread allocation arenas (option under the allocator decision) would remove the atomic counter but break the ADR-0016 budget contract, which is global by definition, and complicate `--leak` accounting (which sums retained blocks); the counter fix is smaller and keeps one budget.
- Moving strings across the boundary as views instead of moves would make the first slice look more capable but reintroduces exactly the cross-thread aliasing the borrow checker exists to prevent.
- Allowing `spawn` of closures that capture borrows (Go's closure capture habit) is precisely M4.1.02's scoped-join design; doing it here without the join-liveness ADR would be unsound.

## Required evidence (per implementing slice, not this ADR)

- The spawn/own-rejection diagnostic: a UI fixture pinning a view argument rejection with its span.
- The allocator-under-concurrency probe: N threads allocating through the budget; the limit trips exactly once, and `--leak` reports no retained blocks.
- Dual-platform: spawn/join runs and reports identical stdout on Windows and Linux (`evaluate-3` direct run while CI remains billing-blocked).

## Evidence (first two decisions, 2026-11-20)

Decisions 1–2 are implemented at `99464a4`: `ae_thread_spawn`/`join` in `runtime.c` (unit `ae_thread`; C11 `thrd_*` on Linux, self-declared Win32 entries with the header's own spellings, placed after the `<windows.h>` include point so the redeclarations merge), and the allocator budget/total/ready as C11 atomics with charge-and-rollback. `scripts/thread_harness.c` (run by `scripts/test_thread_harness.py`, wired into the scripts unittest pass) spawns eight OS threads through the primitives and asserts the cumulative-ask counter equals the exact deterministic total, including the spawn machinery's own blocks. Pay-as-you-go: a print program emits no thread symbols and no atomics beyond the include; an allocating program adds the three atomic globals. Dual platform: Windows (Win32 branch) and evaluate-3 Linux (C11 branch), logs in `bench/results/2026-11-20-linux-threads/` — direct runs, CI still billing-blocked. The Win32 audit test now covers the three thread entries.

## Consequences

- `docs/resource-model.md` §81's deferral is resolved for owned transfers: the concurrency section states the owned-data rule and points here. `Send`/`Sync` remain deferred until M4.1.02+.
- `docs/borrow-checker-design.md` Stage C's mapping question is answered for the first slice: borrows never cross threads because they never escape frames; the marker-trait mapping is still not pre-committed.
- M4.1.01's acceptance test can be written against this ADR before any code lands.
- Rollback: reverting the atomics change returns the allocator to single-threaded-by-contract; the spawn surface should not ship in a release without both pieces, so rollback is all-or-nothing per slice.
