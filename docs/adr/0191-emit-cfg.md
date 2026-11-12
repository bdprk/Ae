# ADR-0191: `emit-cfg` — the Control-Flow Layer

## Status

Implemented (Windows-verified: the full `cargo test --workspace --locked` suite including a whole-suite sweep of 136 run-suite and example files, a switch-shape regression test, and a loop test with break/continue edges; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0190 made the statement tree inspectable; plan M2.2's next layer is the one the borrow checker actually consumes: control flow. Move and borrow states are flow-sensitive — a value moves in one branch and not the other, a borrow lives across a loop back-edge — so the checker needs blocks and edges, not a flat statement list. The statement tree has the control flow *inside* it (`if`/`while`/`match` are statement variants), but no graph.

## Decision

**Each function lowers into a graph of blocks whose contents are the statement tree's own statements, verbatim.** Only control flow is lowered: `if` becomes `Branch(cond, then, else)`; `while`/`for`/`for-tokens` become a head (the branch) with a back-edge; `match` becomes `Switch(subject, layout, arms)`; `return`/`exit` terminate with their expressions; `break`/`continue` become `Goto` edges through loop stacks that track heads and exits. Everything else — assignments, calls, pushes, releases — rides along whole inside its block, because decomposing them is the MIR construction's job, not the CFG's.

**Three deliberate non-splits**, each with a reason:

- `&&`/`||` keep their source form as whole branch conditions: the short-circuit split is expressible in MIR later, and the condition as a unit is what the tree has.
- `?` propagation and `assert` keep their whole-statement form: their branch is decided by a result tag *inside* the operation, and inventing a tag read the tree does not have belongs to the MIR construction.
- `match` payload bindings stay annotations of the switch edges: the tree binds them as reads; explicit assignments come with MIR places.

**Deadness is per statement list, not per block.** A list whose every path ends unconditionally (return/exit/break/continue) reports that upward, and the statements after it — the tree appends function-tail releases that a return edge already carries — are dropped rather than placed in a block that never terminates. Control statements with a continuation propagate aliveness through their join: a `match` whose arms all return keeps its tail dead, while one arm continuing keeps the tail alive. Getting this wrong twice (first as a per-block flag, then as a guard that confused fresh blocks with dead ones) produced internal panics that the validator caught on real suites — the invariant enforcement paid for itself during the same session it landed.

**`main`'s implicit exit-0 is the only legal fall-through.** Every other function's all-paths-return rule (checked at parse time) guarantees a `Return`/`Exit` terminator in each flow, so a block with statements and no terminator is a lowering bug, and the validator panics with `internal:` naming the function and block.

**The validator's invariants**: one terminator per block, every branch target exists, no statements in a non-main unreachable block. The graph shape (`Switch` per variant, one join per if) makes the future borrow checker's dominance walk direct.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 204 passing, including:
  - `emit_cfg_shapes_loops_branches_and_switches` — a `match` lowers to a switch with returning arms;
  - `emit_cfg_runs_clean_on_the_run_suite` — every `.ae` in `tests/run` (currently 87 files) lowers into a validated graph;
  - `tests/run/cfg_loops.ae` — break/continue semantics run correctly (the CFG's edges agree with execution).
- Manual sweep: all 136 `tests/run` + `examples` files lower without a validator panic.
- Shape check on a loop-heavy program: `while` head with a back-edge, `break` to the exit, `continue` to the head, the tail release on main's fall-through — all visible in `ae emit-cfg` output.

## Consequences

- The borrow checker (M2.5) has its substrate: move/borrow state walks become per-block computations along these edges.
- `Clone` was derived on the statement-tree types (`Expr`, `Statement`, `Printed`, `Argument`, `MapPlace`, `MatchArm`) — the CFG keeps statements verbatim inside blocks, which requires owning copies. Cloning happens once per function at lowering; no hot path.
- Known gaps, stated rather than implied: conditions and `?`/`assert` are unsplit; payload bindings are annotations; `emit-cfg` prints per program (imports inline); unreachable-block detection is pending (the validator checks well-formedness, not reachability).
- Next: M2.2 continues with the MIR (places/rvalues/terminators) over this graph, or the borrow checker's L1 starts on it directly.
