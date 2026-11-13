# ADR-0203: The C Backend Emits from the MIR Block Graph (M2.3)

- **Date**: 2026-11-14
- **Status**: Implemented
- **Milestone**: M2.3 (§6: "THIR → MIR construction; C backend changed to generate from MIR")

## Context

The CFG (ADR-0191) and the MIR over it (ADR-0197, ADR-0198, ADR-0201, ADR-0202) have been print-and-validate artifacts so far: every accepted program lowered, but the C backend still consumed the structured statement tree, so the block graph that optimization passes and the interpreter already operate on could drift from what the compiler actually shipped. The plan's M2.3 acceptance says the C backend generates from MIR, with all run tests passing.

Two lowering slices could not yet drive code emission, and a function containing one stayed on the tree backend:

- `for ... in tokens(...)`: the tree statement carries the cursor reset, the per-iteration element binding, and the advance check; the CFG slice branched on the cursor variable without them.
- `match`: the arms' payload bindings were annotations of the switch edge, not statements, so the arm bodies would read undeclared slots.

The counted `for` loop had a related gap in the graph itself: the CFG slice branched the loop head on the bare counter variable and gave `continue` the head as its target, so neither the `counter < limit` test, the hidden checked increment, nor the tree's `ae_next_{label}` landing existed in the graph.

## Decision

1. **Blocks carry their statements.** `MirBlock` keeps the block's statements verbatim alongside the decomposed bindings. A statement form whose decomposition is not complete yet keeps its whole shape, so the C backend emits from the block graph without losing an effect.
2. **Per-function backend selection.** A function whose statement tree contains no unsupported shape is emitted by `codegen_mir::emit_function_body`: each block becomes a C label, its statements are emitted by the same statement emitter the tree backend uses, and the terminator becomes an explicit `goto`, `if (...) goto ...; else goto ...;`, a C `switch` over the subject's tag (one case per variant, in variant order — a `match` is exhaustive, so no default; the arm bodies are other blocks, so nothing inside the switch can `break` an enclosing loop), `return`, or `ae_exit`. A `return`/`exit` block's trailing `Frees` statement is emitted by the terminator, after the returned expression is evaluated — the same place the tree backend runs them.
3. **Escape hatch and cache key.** `AE_CODEGEN=tree` forces the structured tree emitter for the whole program (how differential tests compare the backends, and how a regression rolls back without rebuilding the decision logic). The frontend cache's identity includes the resolved mode, because the two backends produce different C for the same sources.
4. **`match` lowers completely.** The CFG's match lowering now materializes each arm's payload bindings as `BindPayload` statements at the top of the arm's block (a new statement variant that only the lowering produces; the tree form of a match keeps its bindings on the `MatchArm`). Both backends emit a binding through one shared helper — strings as read-only views, vectors as read-only borrows, scalars as copies — so the tree path and the MIR path are identical by construction.
5. **Counted `for` lowers completely.** The loop stack now pairs each loop's break target with its continue target: the head for `while` and `for ... in tokens`, the new step block for a counted `for`. The head's branch is the real `counter < limit` comparison, the step block holds the hidden checked increment (`counter = counter + 1` at the loop header's position, so the overflow diagnostic points where the tree's pointed), and the body's continuation flows into the step. `continue` lands on the step, `break` on the exit — both as plain edges.
6. **CFG simplification stays on.** The MIR backend optimizes before emitting (`optimize_function`): constant folding, edge threading, unreachable-block pruning, and single-predecessor fusion. A pruned block is unreachable, so not emitting it changes nothing; a fused block emits its statements in order at the surviving label.

## Evidence

- `cargo test --workspace --locked` — N tests, 0 failures, with the MIR backend as the default for every supported function (the `tests/run` corpus, the UI suites, and the integration tests all exercise it).
- `python scripts/diff_fuzz.py` — now four-way: `--debug` (MIR backend), `--debug` with `AE_CODEGEN=tree`, `--release`, and `--interp` must produce byte-identical stdout on every seed. (The generator produces no `for`/`match` yet; extending its grammar is the next slice.)
- `tests/execution.rs` — a backend-equivalence test compiles one `match`/`for`-heavy program through both backends and asserts identical stdout.
- `ae emit-mir` on a match/for program shows the `Switch` terminator, per-arm `BindPayload` statements, and the loop's head/body/step blocks.

## Consequences

- The shipped C for most functions is now the CFG/MIR's view of the program; the statement tree remains the emitter of individual statements, but control flow is the graph's. Backend and optimizer can no longer disagree about the shape of control flow.
- `AE_CODEGEN=tree` is a supported rollback, not dead code: the differential test consumes it every run.
- Still on the tree backend: functions containing `for ... in tokens(...)` (the cursor lowering needs a block-graph form for the reset/advance pair). The per-function fallback keeps every other function on MIR.
- Next: give `for ... in tokens` its block-graph form (a cursor-advance statement shape the tree emitter also uses), then extend the fuzzer's grammar with `for` and `match` so the four-way comparison covers them, then M2.4 drop elaboration on the same graph.
