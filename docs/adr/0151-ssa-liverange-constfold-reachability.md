# ADR-0151: SSA Form, Live Range Intervals, Constant Folding, and Reachability Elimination

## Context
Under the Standard Library Blueprint (Domain C: Compiler Optimization Passes & Code Generation), mid-end optimization requires:
1. Static Single Assignment (SSA) variable versioning and 2-input $\Phi$-nodes (`SsaVar`, `PhiNode2`, `ssa_var_next_def`).
2. Live Range intervals and interference checking (`LiveRange`, `live_range_covers`, `live_range_interferes`) for linear scan register allocation.
3. Safe arithmetic constant folding (`FoldResult`, `fold_add_i64`, `fold_sub_i64`, `fold_mul_i64`, `fold_div_i64` with zero-divisor rejection).
4. Dead block reachability bitmask tracking (`ReachabilityMap`, entry block reachability, reachability bit setting and querying).

## Design
- `std/core/ir_ssa.ae`: `SsaVar`, `PhiNode2`, `ssa_var_next_def`, `phi_node2_out_version`.
- `std/core/ir_live_range.ae`: `LiveRange`, `live_range_covers`, `live_range_interferes`.
- `std/core/ir_constant_fold.ae`: `FoldResult`, `fold_add_i64`, `fold_div_i64`.
- `std/core/ir_dead_code.ae`: `ReachabilityMap`, `reachability_map_is_reached`, `reachability_map_mark_reached`.
