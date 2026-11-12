# ADR-0118: Bitwise Logical Traits and Mask Abstractions (M1.4 / M1.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, low-level bit manipulation and flag masking must be expressible via explicit traits (`BitAnd`, `BitOr`) rather than relying on compiler magic or ad-hoc intrinsic procedures.

## Decision
1. Introduce `BitAnd` and `BitOr` in `std/core/bitwise.ae`.
2. Standardize binary bitwise lowering to resolve through in-scope trait implementations.
3. User bitflags and mask structures implement bitwise traits to participate in static bit manipulation.

## Verification
- Unit and run snapshot tests in `tests/run/bitwise_traits.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).