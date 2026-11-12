# ADR-0123: Advanced Core Data Structures and Protocol Primitives (M1.4 / M3.1 / M4.7)

## Status
Accepted

## Context
Per Language Charter §2.1 and §4.1, expanding AE standard library coverage into production scenarios (network proxies, state machines, circular buffers, protocol encoders) requires zero-dependency, allocation-free data types and state transition logic.

## Decision
1. Introduce advanced core modules in `std/core/`:
   - `std/core/ring.ae`: Circular cursor and bounded ring-buffer index arithmetic.
   - `std/core/flags.ae`: Bitmask testing and flag manipulation primitives.
   - `std/core/status.ae`: Protocol status code enumerations and HTTP/network classifications.
   - `std/core/state.ae`: Monotonic lifecycle transitions and state machine validation.
2. Standardize all operations to enforce determinism and zero dynamic memory overhead.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_advanced.ae`.
- Continuous background load regression on dual platforms.