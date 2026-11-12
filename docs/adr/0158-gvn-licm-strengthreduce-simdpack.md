# ADR-0158: Compiler Global Value Numbering, LICM, Strength Reduction, and Auto-Vectorization

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler mid-end optimization), higher-tier program transformations require standardized zero-allocation heuristics and canonical representation:
1. Global Value Numbering (GVN): commutative operand canonicalization ensuring congruent subexpressions map to the same lookup key.
2. Loop Invariant Code Motion (LICM): side-effect, purity, and loop invariance verification for safe hoisting out of loops.
3. Strength Reduction: recognizing multiplications and divisions by powers of two eligible for bitwise shift substitution.
4. Auto-Vectorization packing: contiguous memory access and dependency distance verification for SIMD lane formation.

## Decision
1. **GvnKey (`std/core/ir_gvn.ae`)**: Commutative operator ordering ($lhs \le rhs$) for normalized expression equivalence matching.
2. **LoopHoistCandidate (`std/core/ir_licm.ae`)**: Safe hoisting validator ensuring expression purity and absence of side effects.
3. **StrengthReduceCandidate (`std/core/ir_strength_reduce.ae`)**: Power-of-two detector and integer logarithm ($\log_2$) calculator for bit shift lowering.
4. **SimdPackGroup (`std/core/ir_vector_pack.ae`)**: SIMD lane count calculator and loop-carried dependence validator.

## Consequences
- Mid-end optimization passes can reason about loop invariance, congruence, and SIMD width safely within `core`.
- Verified via test suite `tests/run/batch_core_compiler_opt3.ae`.
