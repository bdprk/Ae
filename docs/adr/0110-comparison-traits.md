# ADR-0110: Comparison Traits and Equality Semantics (M1.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, equality (`==`, `!=`) and relational comparisons (`<`, `<=`, `>`, `>=`) must resolve statically through the `PartialEq` and `Ord` traits without hidden runtime overhead or implicit numeric coercions.

## Decision
1. Introduce `PartialEq` and `Eq` in `std/core/cmp.ae`.
2. Standardize equality lowering to resolve through in-scope trait implementations.
3. Provide default implementations for primitive types (`i64`, `bool`).

## Verification
- Unit and run snapshot tests in `tests/run/cmp_traits.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).