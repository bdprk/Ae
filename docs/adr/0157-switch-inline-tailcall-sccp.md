# ADR-0157: Compiler Advanced Optimizations (Switch Jump Table, Inlining Cost, Tail Call, SCCP Lattice)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler backend), mid-end optimization requires modular decision engines for control flow selection and value analysis:
1. Switch statement lowering: choosing between direct jump tables (dense cases) vs binary search/linear cascades (sparse cases).
2. Function inlining: heuristic cost calculation penalizing loop presence and call-chain depth against a caller budget.
3. Tail Call Optimization (TCO): verifying return-position call eligibility, ABI return signature equivalence, and trivial frame deallocation.
4. Sparse Conditional Constant Propagation (SCCP): three-tier abstract interpretation semi-lattice ($Top \to Constant \to Bottom$).

## Decision
1. **SwitchLoweringStrategy (`std/core/ir_switch_table.ae`)**: Density calculation ($\frac{cases \times 100}{span}$) establishing a 40% threshold for emitting direct jump tables.
2. **InlineCandidate (`std/core/ir_inline_cost.ae`)**: Instruction count scoring with loop penalty (+50) and depth penalty (+10) against an inlining budget.
3. **TailCallEligibility (`std/core/ir_tail_call.ae`)**: Sibling/tail call optimization eligibility validator.
4. **LatticeValue (`std/core/ir_constant_prop.ae`)**: Constant propagation meet operator ($\wedge$) resolving monotonic value states without heap allocation.

## Consequences
- Mid-end optimization passes have clean, zero-allocation algorithmic models directly in `core`.
- Verified via test suite `tests/run/batch_core_compiler_opt2.ae`.
