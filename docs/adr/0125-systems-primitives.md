# ADR-0125: Interior Mutability, Atomics, Endianness, and ASCII Primitives (M1.4 / M4.1)

## Status
Accepted

## Context
Per Language Charter §2.4, §2.5, and §2.6, safe concurrency and low-level parsing require foundational abstractions for interior mutation (`Cell[T]`), atomic memory operations (`AtomicI64`), cross-platform byte ordering (`Endianness`), and byte classifications (`ascii_*`).

## Decision
1. Introduce fundamental modules under `std/core/`:
   - `std/core/cell.ae`: Interior value replacement and extraction without aliases.
   - `std/core/atomic.ae`: Atomic 64-bit word state and relaxed arithmetic operations.
   - `std/core/endian.ae`: Endianness detection and byte-order swap hooks.
   - `std/core/ascii.ae`: Fast branchless ASCII byte classification checks.
2. Ensure pure static typing, zero dynamic allocations, and dual-platform parity.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_systems.ae`.
- Continuous background load regression on dual platforms.