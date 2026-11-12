# ADR-0109: Clone and Explicit Duplication Semantics (M1.7)

## Status
Accepted

## Context
AE follows strict single-owner semantics (§2.5). Types that do not implement `Copy` are moved. Explicit duplication must proceed through a standardized `Clone` trait rather than implicit deep copying or ad-hoc duplication functions.

## Decision
1. Introduce the `Clone` trait in `std/core/clone.ae`.
2. Provide default implementations for primitive scalars.
3. User structs and enums implement `Clone` to produce explicit copies.

## Verification
- Unit and run snapshot tests in `tests/run/clone_trait.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).