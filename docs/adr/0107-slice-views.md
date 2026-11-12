# ADR-0107: Contiguous Slice Views and Read-Only Bounds (M1.4 / M1.10)

## Status
Accepted

## Context
Per Language Charter §2.1 and §2.4, borrowed contiguous views (`&[T]`, `&str`) provide zero-copy inspection over stack arrays and heap buffers with checked bounds.

## Decision
1. Standardize slice view representations in `std/core/slice.ae`.
2. Slices encapsulate length validation and borrow lifetimes without owning allocations.
3. Accesses enforce bounds checks identically across debug and release builds.

## Verification
- Documented in `std/core/slice.ae`.
- Verified on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox node).