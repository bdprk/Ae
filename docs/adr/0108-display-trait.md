# ADR-0108: Display Trait and Formatted Output (M1.7 / M1.12)

## Status
Accepted

## Context
Per Language Charter §2.1 and Roadmap M1.12, formatted output and string representations must resolve through a unified `Display` trait instead of polymorphic built-in dispatch tables.

## Decision
1. Introduce the `Display` trait in `std/core/fmt.ae`.
2. Provide default implementations for primitive scalars (`i64`, `bool`).
3. User structs and enums implement `Display` to participate in standard string interpolation and printing.

## Verification
- Unit and run snapshot tests in `tests/run/display_trait.ae`.
- Continuous stress regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).