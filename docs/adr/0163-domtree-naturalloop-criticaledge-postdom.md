# ADR-0163: Advanced Control Flow Graph Analysis (Dominator Trees, Natural Loops, Critical Edges, Post-Dominators)

## Status
Accepted

## Context
Under Milestone M2 (Self-hosting compiler mid-end analysis), complex transformations (such as SSA reconstruction, Loop Invariant Code Motion, and SSA deconstruction) require foundational CFG topology descriptors:
1. Dominator Trees: immediate dominator (`idom`) and dominance tree depth calculation.
2. Natural Loop Recognition: reducible loop detection, loop header, latch block, and synthesized preheader tracking.
3. Critical Edge Splitting: detection of edges where $succ(src) > 1 \land pred(dst) > 1$ requiring synthetic block insertion for $\Phi$-node lowering.
4. Post-Dominator Trees: immediate post-dominator (`ipostdom`) structure for control dependence analysis.

## Decision
1. **DomTreeNode (`std/core/ir_cfg_dom.ae`)**: Tree node descriptor with root detection and tree level depth.
2. **NaturalLoop (`std/core/ir_cfg_loop.ae`)**: Reducible loop topology structure tracking header, latch, and preheader.
3. **CfgCriticalEdge (`std/core/ir_cfg_critical.ae`)**: Critical edge detector verifying successor/predecessor degree thresholds.
4. **PostDomNode (`std/core/ir_cfg_postdom.ae`)**: Reverse dominance relationship model identifying exit nodes and post-dominance chains.

## Consequences
- The compiler mid-end CFG engine can safely compute loop nests, insert preheaders, and eliminate critical edges entirely in `core`.
- Verified via test suite `tests/run/batch_core_compiler_cfg2.ae`.
