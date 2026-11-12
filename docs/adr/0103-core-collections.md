# ADR-0103: Core Collection Layouts and Generic Container Foundation (M1.10)

## Status
Accepted

## Context
Per §2.2 Hard Rule #1 ("Implement Once, Use for Any Type") and §5.2 ("Structural Gaps"), AE must transition away from specialized compiler-internal container families (`vec[i64]`, `vec[f64]`, `strvec`, `f64map`) toward language-level generic collections.

## Decision
1. Standardize user-space type layouts for core collections in `std/core/`:
   - `Vec[T]`: pointer + length + capacity.
   - `String`: pointer + length + capacity (UTF-8).
   - `Map[K, V]`: hash table control bytes + key/value entries + metadata.
2. Inherent methods provide unified accessors: `.len()`, `.is_empty()`, `.capacity()`.
3. Legacy specialized collections are mapped as aliases during the M1 transition and will be obsoleted upon M2 MIR completion.

## Verification
- Definitions added in `std/core/vec.ae`, `std/core/string.ae`, `std/core/map.ae`.
- Continuous test regression on Windows and InstaBox Linux server.
