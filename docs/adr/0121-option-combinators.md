# ADR-0121: Option Combinators and Modular Adapters (M1.8 / M1.10)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.2, optional values should be manipulable through standard combinators (`option_map_or`) without manual match ceremony boilerplate.

## Decision
1. Introduce Option functional combinators in `std/core/optional.ae`.
2. Standardize module-to-module imports (`use "option.ae";`) within standard library packages.
3. Validate deterministic pattern unwrapping across inter-module calls.

## Verification
- Unit and run snapshot tests in `tests/run/optional_combinators.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).