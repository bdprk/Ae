# ADR-0198: MIR Constant Propagation, Checked Folding, and Dead Temporary Elimination

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite — 209 integration tests, including `mir_constant_propagation_folds_checked_arithmetic_and_branches` and whole-suite `emit-mir` validation).

## Context

Plan M2.10 calls for MIR optimizations starting with constant propagation and dead code elimination, subject to §2.2 Rule 7 ("Development and Release Semantics are Identical: optimizations must not change observable behavior; overflow and bounds checks are enabled in both modes"). With `MirFunction` and `MirBlock` in place (ADR-0197), expressions inside basic blocks decompose into chains of temporary bindings (`t0 = 3 * 4`, `v0 = 2 + t0`). Without constant propagation and dead-temporary cleanup, constant sub-expressions and constant branch conditions remain unsimplified in MIR.

## Decision

1. **Block-local Constant Propagation & Checked Folding (`mir::optimize_function`)**:
   - Tracks known scalar constants (`MirOperand::Int`, `MirOperand::Bool`) across local variables (`Local`) and temporaries (`Temp`) within each basic block, invalidating a local's constant entry when reassigned to a non-constant or modified through a field store.
   - Substitutes known constant operands into `Use`, `Binary`, `Call`, `FnCall`, `Intrinsic`, `Aggregate`, `AdtAggregate`, and `VecNew` rvalues.
   - Evaluates integer (`+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `<=`, `>`, `>=`) and boolean (`&&`, `||`, `==`, `!=`) binary operations using checked arithmetic (`checked_add`, `checked_sub`, `checked_mul`, nonzero `checked_div`/`checked_rem`). Any operation that would overflow `i64` or divide/modulo by zero returns `None` and remains unfolded in MIR so the runtime diagnostic fires at the exact source location (§2.2 Rule 7).
2. **Constant Branch Folding**:
   - When a `CfgTerminator::Branch(cond, then_target, else_target)` has a boolean literal condition or references a `Local` holding a known boolean constant at block exit, the branch terminator is folded into an unconditional `CfgTerminator::Goto` to the taken target.
3. **Dead Temporary Elimination**:
   - After constant propagation and folding, intermediate `MirPlace::Temp` bindings whose rvalue is a pure `MirRvalue::Use` and whose temporary id is never referenced by any subsequent binding in the block are removed, while preserving the block's final statement-effect sink binding and any side-effecting calls/intrinsics.

## Evidence

Re-runnable:

- `cargo test --test execution -- mir_constant_propagation_folds_checked_arithmetic_and_branches` — verifies multi-step arithmetic folding (`2 + 3 * 4` → `14`, `a - 4` → `10`), comparison folding (`b == 10` → `true`), branch-to-goto folding (`goto block 1`), and preservation of trapping `1 / 0`.
- `cargo test --test execution -- emit_mir_runs_clean_on_the_run_suite` — verifies `optimize_function` + `validate` across all `tests/run/*.ae` programs.

## Consequences

- M2.10's first optimization slice is active on MIR and preserves checked arithmetic semantics.
- Next: M2.11 MIR interpreter (`const fn` / `ae test --interp`) or inter-block unreachable-block pruning.
