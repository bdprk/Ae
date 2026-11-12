# ADR-0154: Compiler Code Emission Primitives (Lowering, Peephole, Block Order, Relocations)

## Status
Accepted

## Context
In Milestone M2 (Self-hosting compiler backend), translation from virtual SSA/IR representation to physical machine code requires explicit lowering abstractions:
1. Instruction target lowering (`TargetOp`).
2. Peephole simplification rules for dead stores, redundant moves, and immediate fusion.
3. Basic block layout heuristics to maximize natural fall-through.
4. Section relocation representations for linking and code generation.

## Decision
1. **TargetOp (`std/core/ir_lower.ae`)**: Model machine operations with opcode, destination register/spill slot, source operand, and immediate flag.
2. **Peephole Optimizer (`std/core/ir_peep.ae`)**: Pure functional pattern matching for redundant `MOV Rx, Rx`, small immediate additions, and dead register overrides.
3. **BlockOrderDecision (`std/core/ir_block_order.ae`)**: Heuristic calculation for layout decisions determining whether explicit jump instructions are necessary.
4. **RelocEntry (`std/core/ir_reloc.ae`)**: Representation of relocation targets with PC-relative displacement calculations ($Target + Addend - Current$).

## Consequences
- The compiler backend has complete modular building blocks for emission and linking pipelines within `core`.
- Verified via test suite `tests/run/batch_core_compiler_emit.ae`.
