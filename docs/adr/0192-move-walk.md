# ADR-0192: The CFG Move Walk — Flow-Sensitive Move Verification

## Status

Implemented (Windows-verified: the full `cargo test --workspace --locked` suite — 205 tests including the 137-file run-suite/examples sweep, all agreeing; Linux coverage rides the CI matrix on the next push).

## Context

ADR-0191 gave every function a control-flow graph. Plan M2.5's borrow checker starts with the cheapest sound pass over that substrate: moves. The parser already checks moves in a single pass, with branch merges it can express on the fly (`close_loop_moves`, conservative arm merges). That logic is load-bearing and hard to change; what it lacks is an independent derivation of the same verdicts from the graph — the difference between "the parser rejected it" and "the dataflow proves it".

## Decision

**The move walk runs inside every compile, on programs the parser already accepted.** Per function: a worklist over blocks, each block walked with a moved-set, joins meeting by intersection (a variable is *definitely* moved on entry only if every incoming path moved it), loop heads iterating to a fixed point — the moved set only grows within a block, so the iteration terminates. A violation (a variable read while definitely moved) is an `internal:` error, never a user diagnostic: the parser's verdict and the dataflow disagree, and one of the two is wrong.

**Move sites mirror the parser exactly**, because agreement is the whole point:

- a bare owned variable is consumed by a by-value call argument, a `let` initializer, an assignment's right side (whose completion re-initializes the slot — the moved state *clears*), a `return`, a struct-literal field, an enum payload, a vector literal, and either operand of a string `+`;
- loop containers (`for x in v`), `match` subjects, borrows, string views, and scope-end releases are **reads, never moves** — the parser registers borrows there, and marking them as moves was this pass's one systematic false positive, caught immediately by the 137-file sweep and fixed in the same change;
- an assignment to a slot removes it from the moved set (re-initialization), matching the parser's "assigning to a moved variable is a legal re-initialization".

**The walk is deliberately a verification, not the enforcement**: the parser stays the enforcer (its diagnostics have binding spans and suggestions), the walk is the independent check that will become the enforcement once MIR places carry the borrows too. Splitting enforcement and verification now means the later MIR-based checker can be developed against a known-agreeing baseline instead of against the parser directly.

## Evidence

Re-runnable:

- `cargo test --workspace --locked` — 205 passing, including `move_walk_agrees_with_the_parser_on_the_run_suite` (the 87-file run suite compiles clean, which means the walk agreed on all of them — it runs inside every compile).
- Manual sweep: 137 run-suite and example files; the only failures are the four programs whose *own* purpose is failing (division by zero, index out of bounds, missing CLI args) — all compile-time clean.
- The first sweep run caught the borrow-as-move false positive across ~80 files in one shot: the disagreement signal works.

## Consequences

- The parser's move rules now have an independent reference implementation; changes to the parser's move logic that break agreement surface immediately as internal errors on the test suite.
- Known gaps, stated rather than implied: borrows are not yet tracked (the walk checks moves only — borrow regions along edges are the next slice); the state intersection at loop back-edges can be imprecise for variables moved after the loop head in the body then re-initialized before the back-edge (the fixed point absorbs it, conservatively); `?`/`assert` propagate as whole statements, matching the CFG slice's non-split.
- Next: borrow-scope regions along CFG edges (M2.5 L2), or MIR places/rvalues on the graph.
