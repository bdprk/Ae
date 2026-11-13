# ADR-0202: MIR Linear Block Merging (Single-Predecessor Fusion)

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite — 213 integration tests, 269 workspace tests, including `mir_linear_block_merging_fuses_single_predecessor_successors` and whole-suite `emit-mir` validation).

## Context

ADR-0201 left MIR with only reachable blocks and threaded every edge through empty `Goto` trampolines, but a block whose sole incoming edge is an unconditional jump still costs a block header, an edge, and a jump in the printed form. Lowering produces exactly that shape whenever a statement list continues after an arm: the continuation block is reached only from the arm that kept running. Constant branch folding (ADR-0198) makes the shape common — after `if true { ... }` the entry jumps straight at the arm's block.

The printed block graph is the reference the future MIR backend and the optimizer debugging workflow consume, so redundant straight-line blocks obscure real control flow.

## Decision

`mir::merge_linear_blocks` fuses `A --goto--> B` into one block when:

1. A's terminator is `Goto(B)` — an unconditional edge, so B's bindings run on every path through A;
2. B's only incoming edge is that one (`incoming[B] == 1`), so no other block can enter B without A's bindings having run;
3. B is not the entry block, whose implicit entry edge is not visible in the CFG, and not A itself (a self-loop is left as it is).

The fused block keeps A's id and bindings and appends B's bindings, then adopts B's terminator. Binding order is preserved, temporary ids are unique per function (the lowering allocates them globally), and B's id disappears from the graph. Fusion repeats to a fixed point because one merge can make the next eligible.

`simplify_cfg` now runs thread → prune → merge per round; each round strictly reduces blocks or edges, so it terminates. Conditional arms are never folded into the branch block: their bindings would then execute on the other path too, which §2.2 Rule 7 (development and release semantics identical, optimizations must not change behavior) forbids.

## Evidence

Re-runnable:

- `cargo test --test execution -- mir_linear_block_merging_fuses_single_predecessor_successors` — `if c { x = 1; } else { return 9; } print(x); return x;` prints 3 blocks, no `goto block 3`, and the fused block (`v1 = 1`, print operand, `return v1`) in order; a `while` loop keeps 3 blocks and its `goto block 0` back edge.
- `ae emit-mir` on the ADR-0198 constant-propagation fixture now prints `fn main() -> i64 (1 blocks)` with no `goto` at all; before this pass it printed the folded `goto block 1` chain.
- `cargo test --workspace --locked` — 269 tests, 0 failures.

## Consequences

- Straight-line control flow no longer appears in MIR: a function whose body has no branch lowers to a single block, and a branch arm that continues absorbs its continuation.
- Block ids stay stable (the surviving block keeps the predecessor's id), so consumers must still look blocks up by id rather than index — the same rule ADR-0201 established.
- Loop shapes are unchanged, because a loop condition block branches and its body has only a conditional entry.
- Next: drop elaboration (M2.4) and the MIR backend (M2.3) can now consume a graph without redundant jumps; interprocedural optimization follows the backend swap.
