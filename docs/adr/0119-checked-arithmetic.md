# ADR-0119: Checked Arithmetic Operations and Explicit Overflow Enums (M1.4 / M1.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, integer overflow traps by default across both debug and release configurations. Controlled overflow handling requires explicit checked arithmetic functions returning `Checked[T]` instead of relying on silent wrapping or platform-dependent C undefined behavior.

## Decision
1. Introduce `Checked[T]` in `std/core/maybe.ae`.
2. Provide `checked_add` and `checked_mul` ensuring mathematically proven overflow detection.
3. Fallback and wrapping algorithms utilize checked arithmetic without bypassing safe language boundaries.

## Verification
- Unit and run snapshot tests in `tests/run/checked_arithmetic.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).