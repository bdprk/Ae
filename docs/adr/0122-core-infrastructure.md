# ADR-0122: Standard Library Core Infrastructure and Foundation Primitives (M1.4 / M3.1)

## Status
Accepted

## Context
In accordance with §4.1 ("Standard Library Layering: core / alloc / std"), core abstractions that require zero external OS allocations must be structured into independent modular packages.

## Decision
1. Introduce foundation core packages under `std/core/`:
   - `std/core/buffer.ae`: Contiguous memory buffer abstractions with bounded capacity.
   - `std/core/math.ae`: Safe bounded numeric primitives (`max`, `min`, `clamp`).
   - `std/core/log.ae`: Log level representations and filter predicates.
   - `std/core/list.ae`: Generic intrusive list node structures.
   - `std/core/optional.ae` & `std/core/result_combinators.ae`: High-level combinators.
2. Ensure all modules compile standalone and monomorphize with zero dynamic memory overhead.

## Verification
- Comprehensive execution test in `tests/run/batch_core_modules.ae`.
- Continuous background load verification on dual platforms.