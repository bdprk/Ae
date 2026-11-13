# ADR-0201: MIR Unreachable Block Elimination and CFG Simplification

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite — 212 integration tests, including `mir_unreachable_block_elimination_prunes_abandoned_continuations` and whole-suite `emit-mir` validation).

## Context

Plan M2.10 calls for MIR-level dead code elimination. Lowering (ADR-0197) deliberately creates blocks that no path reaches: the join `alive_target` builds when every arm of a statement list returned, the fall-through block a `return` abandons, and the pending tail of a function that always returns. Constant branch folding (ADR-0198) orphans more of them by turning a `Branch` into a `Goto`, leaving the untaken arm's blocks behind. A function whose two arms both `return` printed six blocks for three real ones.

## Decision

1. **Reachability pruning (`mir::retain_reachable`)**: from the entry block (the first block, id 0 by construction), walk `Goto`/`Branch`/`Switch` successors and retain only reachable blocks. Terminators that end flow (`Return`, `Exit`, `Unreachable`) contribute no successors.
2. **Edge threading through empty trampolines (`mir::thread_edges`)**: a block with no bindings whose terminator is `Goto(next)` does no work, so every edge into it is retargeted to `next`. Chains are followed with a visited set, so a cycle of empty blocks resolves to the first block already seen — an infinite loop is never rewritten into a different loop.
3. **Coincident-target branch collapse**: a `Branch` whose resolved targets are equal becomes a `Goto`, but only when the condition is side-effect free. Conditions keep their whole source form in the CFG, so a call inside one is observable; §2.2 Rule 7 forbids dropping it.
4. **Stable block ids**: nothing is renumbered, so printed MIR keeps the block names earlier passes and the existing tests expect (`goto block 1` after branch folding), and surviving edges keep pointing at the same logical block. `validate` therefore checks that a target names a surviving block (`HashSet` membership) rather than a dense index range.
5. **Fixed point**: threading can orphan a trampoline and pruning can expose a new one, so `simplify_cfg` repeats thread-then-prune until neither changes anything (bounded by the block count).

The pass runs after per-block optimization, so folding decisions feed it: constant branches become `Goto` first and the untaken arm is then pruned.

## Evidence

Re-runnable:

- `cargo test --test execution -- mir_unreachable_block_elimination_prunes_abandoned_continuations` — a two-arm `pick` prints 3 blocks instead of 6, main's loop keeps its back edge (`goto block 0`), and no `unreachable` tail is printed.
- `cargo test --test execution -- emit_mir_runs_clean_on_the_run_suite` — lowering + optimization + validation over every `tests/run/*.ae` program.
- `ae emit-mir` on `if false { } else { } print(1);` prints `goto block 3` directly, showing edge threading.
- `cargo test --workspace --locked` — 268 tests, 0 failures.

## Consequences

- MIR now shows only live control flow, which makes the printed form usable as the reference for the future MIR backend and for optimizer debugging (ADR-0198's dump is ~half the size on returning functions).
- Block ids are sparse; anything consuming MIR must look blocks up by id, not index. `validate` already enforces this.
- Next: block merging along single-predecessor edges (statement fusion), then interprocedural optimization once the MIR backend replaces the C statement-tree backend.
