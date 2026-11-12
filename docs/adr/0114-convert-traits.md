# ADR-0114: Explicit Conversion Traits and Coercion Boundaries (M1.4 / M1.7)

## Status
Accepted

## Context
Per Hard Rule §2.2 #1 ("Implement Once, Use for Any Type") and Type System Charter §2.4, AE strictly prohibits implicit type coercions. Any conversion across type domains must be executed through explicit conversion traits (`Into`, `FromI64`).

## Decision
1. Introduce standard conversion traits in `std/core/convert.ae`.
2. Provide default implementations for scalar identity conversions.
3. User structs and enums implement conversion traits to safely interface across numerical and domain boundaries.

## Verification
- Unit and run snapshot tests in `tests/run/convert_traits.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).