# ADR-0106: Heap Allocation Wrapper and Deref Strategy (M1.4 / M1.10)

## Status
Accepted

## Context
ae provides deterministic single-owner memory management (§2.5). In addition to contiguous stack and array storage, explicit unique pointer semantics are required for recursive data structures and heap allocations.

## Decision
1. Introduce `Box[T]` in `std/core/box.ae` as the fundamental unique pointer wrapper.
2. Inherent borrow methods provide access to internal owned values without copy overhead.
3. Drop flags in subsequent MIR lowering (M2) will automatically release the underlying buffer upon scope exit.

## Verification
- Documented in `std/core/box.ae`.
- Continuous test regression on dual platforms (Windows x86-64 and Linux AMD EPYC on InstaBox).