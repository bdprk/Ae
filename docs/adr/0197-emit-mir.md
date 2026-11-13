# ADR-0197: `emit-mir` — Places, Rvalues, and Explicit Temporaries Over the CFG

## Status

Implemented (Windows-verified: full `cargo test --workspace --locked` suite — 208 integration tests with 0 compiler warnings, including whole-suite verification of `emit-mir` across all `tests/run/*.ae` files).

## Context

ADR-0190 (`emit-tree`) made the statement tree inspectable, and ADR-0191 (`emit-cfg`) lowered control flow into basic blocks with explicit terminators (`Goto`, `Branch`, `Switch`, `Return`, `Exit`, `Unreachable`). Plan M2.2 next requires a Mid-level Intermediate Representation (MIR) that decomposes expressions inside each basic block into explicit places (`Local`, `Field`, `Temp`), operands (`Place`, `Int`, `Float`, `Bool`, `Str` with source span), and rvalues (`Use`, `Binary`, `Call`, `FnCall`, `ClosureLit`, `Intrinsic`, `Aggregate`, `AdtAggregate`, `VecNew`), paired with a validator and text printer (`ae emit-mir <source.ae>`).

## Decision

1. **Per-block decomposition over the CFG**: `mir::lower_function` builds the function's `CfgFunction` and lowers each `CfgBlock` into a `MirBlock` holding a sequence of `MirBinding { target: MirPlace, rvalue: MirRvalue }` steps while preserving the block's `CfgTerminator`.
2. **Explicit places and temporaries**:
   - Sub-expressions in binary operators, call arguments, and field bases are lowered into fresh typed temporaries (`MirPlace::Temp(id, ty)`) before the enclosing operation, making evaluation order and intermediate values explicit.
   - Closure literals lower directly to `MirRvalue::ClosureLit(cid)`; indirect calls through function-typed variables lower to `MirRvalue::FnCall`; direct calls (including lowered closure calls) lower to `MirRvalue::Call`.
   - Remaining compound expressions that are not yet decomposed into primitive places lower to `MirRvalue::Intrinsic("expr", ..., ty)` bound to a fresh temporary rather than self-referential `Use` bindings.
3. **Validator and Printer**: `mir::validate` checks terminator target bounds across all blocks before `print_function` formats the human-readable MIR dump (`fn name(params) -> ret (N blocks)`).

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 208 integration tests, 3 snapshot suites, 43 unit tests, 0 compiler warnings.
- `emit_mir_lowers_places_rvalues_and_runs_clean_on_the_run_suite` — verifies `ae emit-mir` output shape on structs, closures, calls, and binary rvalues, and sweeps the entire `tests/run` suite.

## Consequences

- Completes the inspectable IR stack (`emit-tree` → `emit-cfg` → `emit-mir`) required by M2.2.
- Provides the substrate for M2.10 MIR constant propagation / dead-code elimination and M2.11 MIR interpretation.
