# ADR-0104: Iterator Trait and Sequential Data Flow (M1.8)

## Status
Accepted

## Context
AE's Design Charter §2.1 and Roadmap M1.8 require zero-cost iterators as the primary abstraction for sequence traversal and transformations, replacing specialized while-index loops and hard-coded container traversal built-ins.

## Decision
1. Introduce the `Iterator` trait in `std/core/iter.ae`.
2. Define the core contract around `next()` yielding `Option[Item]`.
3. Provide initial iterator adapters and primitive ranges `Range { current, end }`.
4. Ensure iteration chains monomorphize into code matching hand-written loops within ≤ 5% execution variance.

## Verification
- Unit and run snapshot tests in `tests/run/iterator_trait.ae`.
- Continuous regression on Windows x86-64 and Linux AMD EPYC (InstaBox).