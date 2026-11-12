# ADR-0112: Default Trait and Fallback Initialization (M1.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, data initialization without explicit literals requires a standardized `Default` trait, ensuring zero-initialized semantics for primitives and sensible initial states for composite records without runtime reflection.

## Decision
1. Introduce the `Default` trait in `std/core/default.ae`.
2. Provide default implementations for primitive types (`i64`, `bool`, `string`).
3. User structs and enums implement `Default` to participate in generic fallbacks and container allocations.

## Verification
- Unit and run snapshot tests in `tests/run/default_trait.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).