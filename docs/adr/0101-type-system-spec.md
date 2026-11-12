# ADR-0101: Type System Specification and Generic Foundation Charter (M1.1)

## Status
Accepted

## Context
Following the successful completion of M0 (workspace crate refactoring, multi-error diagnostic recovery, dual-platform Windows/Linux parity testing), AE begins Milestone 1 (Type System Core).

Existing implementations in AE relied on specialized per-element collections (`vec[i64]`, `vec[f64]`, `strvec`, `f64map`), pre-defined enum results (`FloatResult`, `BoolResult`), and global built-ins rather than true user-extensible type-agnostic collections and method syntax.

## Decision
1. Formalize `spec/types.md` establishing the core type hierarchy (Scalars, Composites, References, Nominals, Enums).
2. Establish the M1 core transition plan:
   - Introduce standard `Option[T]` and `Result[T, E]`.
   - Implement `Vec[T]`, `Map[K, V]` as first-class generic library types.
   - Introduce method syntax resolution (`receiver.method()`).
   - Deprecate old monolithic built-in container identifiers with gradual migration.

## Verification
- Specification checked against design charter §2.
- Verified on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox node).
