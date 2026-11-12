# ADR-0113: Numeric Bounds, Absolute Values, and Signedness Traits (M1.4 / M1.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, primitive numerical behavior should expose trait-driven contracts (`Signed`, `abs`, `signum`) avoiding implicit float/integer casting while preserving checked integer overflow boundaries.

## Decision
1. Introduce `Signed` trait in `std/core/num.ae`.
2. Provide standard checked scalar implementations for `i64`.
3. Support generic mathematical algorithms constrained by `T: Signed`.

## Verification
- Unit and run snapshot tests in `tests/run/num_traits.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).