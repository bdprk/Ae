# ADR-0176: Incremental Compilation Query Engine Primitives (Query Keys, Dependency Graph, Crate Interface Summaries, Session Accounting)

## Status
Accepted

## Context
`docs/plan.md` §3.1 requires query-based incremental compilation in the salsa style: every compiler phase is a cached pure function, and the compiler and the language server share one engine. The same section makes crate interface summaries part of cache keys, so that editing a function body recompiles only one crate while moving a public interface rechecks its dependents. M5.4 sets the measurable target: modifying a function body in a 100k-line project completes in under 100 ms.

The self-hosting IR series (ADR-0150 through ADR-0175) now models CFG/SSA form, register allocation, optimization passes, DWARF debug information, and the ELF64, Mach-O, PE/COFF, and WebAssembly object containers. None of it models the incremental layer itself: there is no representation of a query's identity, of which results depend on which, of the per-crate summary that participates in a cache key, or of the red/green bookkeeping a scheduler needs. Without those four pieces the incremental engine cannot be specified, let alone measured.

## Decision
1. Implemented `std/core/ir_query_key.ae`:
   - Stable query kind identifiers for parse, resolve, type-check, MIR, and code generation, plus a validity predicate.
   - `QueryKey` as the triple (kind, target, input fingerprint): a cached result is reusable only while all three agree.
   - `query_key_digest` folds the triple with the shared combiner constant; `query_key_matches` compares all components; `query_key_stale` compares the input fingerprint alone; `query_key_same_target` separates "same item, new input" from "different item".
   - `query_key_chain_digest` folds a pipeline position (upstream query count plus the previous digest) so that inserting a stage cannot collide with the previous shape.
2. Implemented `std/core/ir_dep_graph.ae`:
   - `DepNode` carrying outgoing dependency counts, the interface subset of those edges, and a dirty flag; `DepEdge` carrying endpoints and whether the edge crosses a crate boundary.
   - `dep_node_needs_recompute` implements red-green marking: a node recomputes when it is dirty itself or was reached through a dependency that already changed; a node that is neither stays green and its payload is reused untouched.
   - `dep_body_dependents` and `dep_invalidated_count` distinguish body edits (intra-crate edges) from interface edits (cross-crate edges).
   - `dep_node_overcounts` rejects a node whose interface edges exceed its total edges; `dep_edge_is_self_loop` and `dep_cycle_guard` bound fixed-point propagation over a cyclic graph.
3. Implemented `std/core/ir_crate_interface.ae`:
   - `CrateInterface` carrying the crate identity, item count, signature hash, and ABI epoch.
   - `crate_interface_summary_hash` folds the item count into the summary, so adding or removing an item moves it even when the surviving signatures hash identically.
   - `crate_interface_unchanged` requires matching identity, matching ABI epoch, and a matching summary; `crate_affected_crates` therefore reports zero downstream rebuilds for the body-edit case and dependents plus one otherwise, treating an unknown (negative) dependent count as zero rather than guessing.
4. Implemented `std/core/ir_query_stats.ae`:
   - `QueryStats` recording recomputed and reused queries alongside the red and green node counts.
   - `stats_note_recompute` and `stats_note_reuse` advance one query and its node colour together, keeping the two views consistent.
   - Rates are exposed in permille through integer arithmetic (`stats_reuse_permille`, `stats_recompute_permille`) so a report never depends on float formatting; `stats_exceeds_budget` compares against a recompute budget; `stats_merge` combines per-crate sessions.
5. Added `tests/run/batch_core_incremental.ae` covering the query identity cases (reuse, input change, kind change, chain shape), the dependency cases (dirty/clean transitions, body versus interface invalidation, self-loops, cycle guard), the crate interface cases (body edit invisible, item, signature, ABI and identity changes visible, affected-crate counts including the unknown-dependent and empty-interface cases), and the accounting cases (empty session, permille rates, budget boundary, merge).

## Consequences
- These are preliminary data structures and pure helpers. They do not store query results, hash real source text or signatures, schedule recomputation, persist a cache across processes, or measure incremental latency. M5.4 remains unverified, and no claim of incremental compilation support follows from this ADR.
- The digest and summary functions use a positive modulo combiner rather than a cryptographic hash. They detect change, not collision resistance; a real cache key must add content hashing when the engine is wired up.
- Query kinds are fixed numeric constants with no registry. Adding a phase requires a new identifier and a corresponding update to `query_kind_is_known`.
- The negative dependent count in `crate_affected_crates` is a deliberate conservative choice: unknown dependency information reports zero rebuilds and is expected to be replaced by an explicit result type when the engine distinguishes "no dependents" from "unknown".
