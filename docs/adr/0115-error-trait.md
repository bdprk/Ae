# ADR-0115: Unified Error Trait and Extensible Error Reporting (M1.11)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.2 Hard Rule #6 ("Single Error Mechanism"), ae eliminates pre-defined ad-hoc result enums (`FileResult`, `FloatResult`, `BoolResult`) in favor of `Result[T, E]` where `E: Error`.

## Decision
1. Introduce the `Error` trait in `std/core/error.ae`.
2. Define `description` method providing structured error context.
3. User errors implement `Error` to seamlessly integrate with `Result[T, E]` and propagation operator `?`.

## Verification
- Unit and run snapshot tests in `tests/run/error_trait.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).