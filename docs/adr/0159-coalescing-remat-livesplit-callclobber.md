# ADR-0159: Advanced Register Allocation Primitives (Coalescing, Rematerialization, Live Range Splitting, Call Clobbering)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler backend), state-of-the-art register allocators (Iterated Register Coalescing & Linear Scan with Splitting) require fine-grained decision models:
1. Register Coalescing: merging move-related virtual registers when their live ranges do not interfere.
2. Rematerialization: evaluating whether re-evaluating trivial constants or base addresses is cheaper than emitting stack spill/reloads.
3. Live Range Splitting: segmenting long virtual register intervals across high register pressure regions.
4. Calling convention clobbering: differentiating caller-saved vs callee-saved registers via bitmask analysis.

## Decision
1. **CoalesceCandidate (`std/core/ir_coalesce.ae`)**: Move elimination and affinity weighting checker.
2. **RematCandidate (`std/core/ir_remat.ae`)**: Cost-benefit comparator against spill reload latencies.
3. **LiveRangeSplit (`std/core/ir_live_split.ae`)**: Interval subdivision and length calculation.
4. **RegClobberMask (`std/core/ir_call_clobber.ae`)**: Bitmask query checking caller-saved vs callee-saved preservation.

## Consequences
- The compiler backend can perform advanced register assignment optimizations directly within `core` zero-allocation boundaries.
- Verified via test suite `tests/run/batch_core_compiler_regalloc2.ae`.
