# ADR-0105: Operator Traits and Unified Arithmetic Dispatch (M1.7)

## Status
Accepted

## Context
ae Design Charter §2.4 specifies operator overloading via Traits (`Add`, `Sub`, `Mul`, `Div`) without implicit conversions. All arithmetic boundaries adhere strictly to checked integer and IEEE 754 float semantics across platforms.

## Decision
1. Introduce operator traits in `std/core/ops.ae`.
2. Standardize binary operator lowering to resolve through in-scope operator trait impls.
3. Overflow checks remain enabled identically in debug and release profiles.

## Verification
- Unit and execution snapshot tests in `tests/run/ops_traits.ae`.
- Continuous stress validation on Windows x86-64 and Linux AMD EPYC (InstaBox node).