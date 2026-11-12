# ADR-0102: Inherent Methods, Associated Functions, and Trait Resolution (M1.2)

## Status
Accepted

## Context
AE previously relied on free-standing built-in functions (`vec_push`, `map_get`, `strvec_len`, etc.) or simulated method syntax routed through specialized internal compiler desugaring. Under the M1 roadmap of the Language Charter (§2.1, §2.2), methods must be first-class constructs defined via `impl` blocks on structs and enums.

## Decision
1. Standardize syntax for inherent methods and associated functions:
   - `self`, `&self`, `&mut self` as the first parameter of methods.
   - Associated functions without `self` invoked as `Type::method()`.
2. Formalize method resolution precedence: Inherent methods strictly shadow trait methods with identical signatures.
3. Coherence and orphan rules apply to prevent conflicting trait implementations across modules.

## Verification
- Documented in `spec/methods_and_traits.md`.
- Snapshot and semantic execution tests added under `tests/ui/` and `tests/execution.rs`.
- Dual-platform tested on Windows x86-64 and Linux AMD EPYC on InstaBox node.
