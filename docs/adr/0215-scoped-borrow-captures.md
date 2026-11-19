# ADR-0215: Scoped Borrow Captures (Scope Join)

- Status: Accepted (design); the checker/region work follows in an M4.1.02 slice carrying its own evidence
- Date: 2026-11-20
- Associated Milestone: M4.1.02 (see `docs/plan.md` §6)
- Depends on: ADR-0214 (owned-only boundary — this ADR relaxes exactly one of its rules, under a proof)

## Context

ADR-0214 fixed the first thread surface: captures move, borrows cannot cross. That makes the common pattern — a thread reading a large structure the caller keeps — impossible without cloning:

```
let data = "a long analysis result";
let t = spawn(|| -> i64 { return len(data); });   // rejected today: view capture
print(wait(t));
```

The language's borrow system (ADR-0205) already tracks view borrows with NLL regions — a view's region ends at its last use. The missed observation: **`wait` is a join point**. A borrow capture whose handle is provably joined before the borrowed variable's region ends is exactly as safe as a plain view: the thread's reads all happen before the join's happens-before edge, which precedes any later use of the borrowed storage. M4.1.02's acceptance test ("scope join: join a borrowed child on success and on early scope exit") is precisely this proof, expressed as tests.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| A. Borrow captures allowed when the handle is provably joined before the borrowed region ends (this ADR) | Reuses the NLL region machinery; no runtime cost; the proof is compile-time | The region computation must model the handle→join edge; loops and early returns need adversarial care | Compile: region analysis extension. Runtime: none |
| B. Explicit `scope(|s| ...)` closures holding the spawned children (Rust's shape) | Scoping is structural | The language has no closure-with-env-callback form for scopes; nesting scopes inside expressions is foreign to the statement grammar | Compile: new statement form. Runtime: none |
| C. Keep ADR-0214's owned-only rule; wait for full `Send`/`Sync` | Simplest | The read-a-large-structure pattern stays impossible; M4.1.02 cannot close | — |

## Decision

1. **A borrow capture is allowed when the spawned handle is provably joined before the borrowed variable's region ends.** "Provably joined" is a CFG property computed on the function's block graph: every path from the spawn reaches a `wait` of that handle before any exit (return, `?`, panic-equivalent fail) and before the borrowed variable's region end. The borrow capture's region then ends at the join, not at the thread's lifetime — the join's happens-before edge is the safety argument.
2. **The proof is per-handle and flow-sensitive.** A handle spawned inside a loop must be joined within the same iteration (a handle that survives an iteration's back-edge is rejected — the join point cannot be proven to precede the borrowed variable's next read). Handles moved into structures, or passed to non-`wait` consumers, lose the proof and the spawn is rejected.
3. **The borrowed variable stays frozen for the whole scoped region** — between the spawn and the join, the variable cannot be written, moved, or re-borrowed mutably, exactly like a view borrow today. After the join, all restrictions lift (the region ended).
4. **Multiple handles, one region.** Two spawns capturing the same variable each carry the rule; the variable's region ends at the last of their joins.
5. **Non-`atomic`, non-`Fn` captures only.** Copy-semantics handles (`atomic`) already share; a spawn capturing both a view and an atomic takes the stricter rule for the view.
6. **Rejected shapes are the existing borrow-capture diagnostic, upgraded**: today's message ("spawn moves its captures into the thread; 'x' is captured by reference and cannot cross") becomes the rejection for handles whose join cannot be proven, with the span on the unproven spawn.

## Implementation sketch (the M4.1.02 slice)

- `borrowscope.rs` gains a `JoinEdges` computation: for each spawned-with-borrow-captures handle, a forward CFG walk from the spawn must reach a `wait(handle)`-dominating every exit before any borrowed variable's region end. The NLL regions' view-borrow end points extend to the join point when the proof succeeds; the spawn is rejected with the upgraded diagnostic when it fails.
- The closure's borrow probe stays: the capture classification already knows which names are borrowed; the spawn arm consults the new proof instead of rejecting unconditionally.
- `wait` on a handle whose spawn had borrow captures marks the join in the graph; a handle used in any other way (stored, returned, passed) voids the proof.

## Required evidence (the M4.1.02 slice, not this ADR)

- The positive form: a spawn reading a string view capture, joined before the caller reads the string again — compiled and interpreted, byte-identical.
- The adversarial matrix: unjoined handle on one branch (rejected), handle joined after the borrowed variable moved (rejected), handle joined inside the loop it spawned in (accepted), handle escaping via a structure (rejected).
- Leak check: the scoped region's releases are unchanged (the borrowed variable is the caller's, released as today).
- Dual-platform per the standard pass.

## Consequences

- ADR-0214's owned-only rule gains one proven exception; `spec/concurrency.md` §1 rule 1 and §3 get the scoped-borrow paragraph when the slice lands.
- `resource-model.md` §7's `Send`/`Sync` deferral shrinks: scoped view borrows are the first cross-thread shared reads, still without marker traits.
- The mutex/Arc slices remain blocked on the drop/panic model; this ADR does not unblock them.
- Rollback: reverting the region extension restores ADR-0214's unconditional rejection; the diagnostic text change is the only user-visible surface.
