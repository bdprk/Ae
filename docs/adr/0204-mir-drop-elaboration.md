# ADR-0204: Drop Elaboration over the MIR Block Graph (M2.4)

- **Date**: 2026-11-14
- **Status**: Implemented
- **Milestone**: M2.4 (§6: "drop elaboration: releases placed where storage leaves scope")

## Context

The parser attaches a release list to every point where storage leaves scope: the end of a block, `break`/`continue`, and the releases a `return`/`exit` carries. Those lists are written before any control-flow graph exists, and they are conservative in both directions:

- they name a slot a statement has already **moved out of** (the move zeroed the slot, so the release frees nothing but still branches and stores), and
- they can name a slot a path reaches **before it was ever written** (releasing an uninitialized slot would free a garbage pointer).

Until now the tree backend shipped those lists verbatim, relying on two invariants: a move zeroes the slot it takes from, and every release is idempotent (a zeroed slot no-ops). That is safe but does work the program cannot observe: dead releases execute on every run.

The MIR block graph (ADR-0197–0203) already knows, per block and per owned local, what every path holds. Drop elaboration turns the conservative lists into what the graph proves each release site needs.

## Options

| Option | Pros | Cons | Resource Cost |
|---|---|---|---|
| Keep emitting the parser's lists verbatim (tree behavior) | Zero new code | Dead releases execute at runtime; releases of never-written slots stay one lowering change away from freeing garbage | Compile: none; Runtime: wasted branches/stores per dead release |
| Prove each release per site (dataflow over the graph); keep what is not proven | Removes provably dead releases; keeps safety for everything else | One more pass to maintain; a new state lattice to explain | Compile: one fixed-point pass per function |
| Runtime drop flags for unproven releases | Precise in every shape | Extra hidden state in generated C, more runtime work than the idempotent release it replaces | Compile: none; Runtime: a flag store/branch per potentially-dead release |

## Decision

1. **One pass, one lattice.** After optimization, `mir::elaborate_drops` computes, per block and per owned local, whether the slot holds a value that still needs releasing on **every** incoming path (`Live`), on **none** of them (`Uninit` — no path wrote it, `Dropped` — every path already moved or released it), or on **some** of them (`Mixed`). The meet is exact: equal states join to themselves, any other pair joins to `Mixed`.
2. **Elide only with proof.** A `Live` release stays; `Uninit`/`Dropped` releases are removed (the latter is the common win — every move out of a binding makes its scope-end release dead). The `?`'s error-path releases read the same states: the value's moves apply first, the releases see the post-evaluation state (the path exits the function), and the Ok binding is written after. Removing a release list can empty a block, so edge threading runs once more afterwards; the two passes cannot invalidate each other because threading and fusion only retarget edges and preserve statement order.
3. **Keep what is not proven.** A `Mixed` release stays, and after it the slot counts as dropped on every path. This is the exact C the tree backend ships: a move zeroes the slot it takes from, so the idempotent release frees on the paths that still hold the value and no-ops on every other. An earlier draft rejected `Mixed` as an internal error; a legitimate program — a binding moved in one branch only, released at scope end — reaches it, and refusing to compile accepted programs is not an option, so the pass keeps the release instead. `AE_DROP_STATS` reports `kept/kept_mixed/elided_moved/elided_uninit` per function; there is no diagnostic an accepted program can hit.
4. **One reading of a move.** Move events come from `movewalk::statement_move_effects`, produced by the same walk the move verifier uses (a `MoveTracker` trait: the verifier checks, the recorder records). The two analyses cannot disagree about what a statement consumes or re-initializes, and the walk's revived events (declare, assign, aggregate writes, `?` bindings) are what let a release after a re-assignment survive with proof.
5. **The printer ships the elaborated graph.** `emit-mir` runs elaboration before printing, so the printed graph is the one the C backend executes, releases included.

## Evidence

- `cargo test --workspace --locked` — 272 tests, 0 failures on Windows, including the two new M2.4 tests:
  - `mir_drop_elision_removes_releases_of_moved_slots`: the MIR backend emits no scope-end release for a binding a call consumed; the tree backend still emits it; both backends run the program identically.
  - `mir_drop_elision_keeps_live_and_unproven_releases`: a loop-carried binding's release is kept (live on every path), a branch-moved binding's release is kept without proof, and both backends agree byte for byte on stdout.
  - `mir_drop_elision_thins_propagate_error_path_releases`: a slot the program consumed before a `?` loses its error-path release; the tree backend keeps it. 2 releases ship where 6 did.
- `AE_DROP_STATS=1 ae emit-c` on the differential corpus reports `elided_moved` counts per function with `kept=0` for move-only mains, and `kept_mixed=1` on seed 5 of the fuzzer, confirming both faces are exercised by real generated programs.
- `ae emit-mir` prints each surviving release list as a `releases(...)` binding with one operand per slot (the release lists were previously a bare `stmt()` placeholder). The drop pass rewrites the statement and its decomposition together, so the printed graph cannot show a release the shipped C does not run; `emit_mir_shows_which_releases_survive_drop_elaboration` pins all three faces (kept, elided, unproven). String-shaped place types print their real types (`v0 (string) = "hello"`), not the integer catch-all.
- Single-platform verification (Windows); the pass emits no new C templates, so `verify_generated_c.py`'s sanitizer mode adds nothing platform-specific here.

## Consequences

- The shipped C for typical programs no longer runs releases the graph has proven dead; the tree backend keeps its behavior unchanged (`AE_CODEGEN=tree` remains the differential baseline).
- A latent hazard class is closed structurally: a release of a slot no path wrote can no longer reach the C output through the MIR backend, whatever the parser's lists say.
- Safety is never traded: any state the pass cannot prove keeps the conservative release, identical to tree-backend output.
- The fuzzer's grammar now generates the `Mixed` shape itself: a string a branch moves into a by-value call (`eat`), retired after the branch so the program stays accepted. 40 seeds, 0 failures, four-way.
