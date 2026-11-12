# ADR-0130: RNG Generators, 128-bit UUIDs, Color Layouts, and Stream State (M1.4 / M3.2 / M3.3)

## Status
Accepted

## Context
Per Language Charter §2.1 and Roadmap M3.2/M3.3 ("Standard Library: io, rand, stream, uuid"), systems services and CLI tooling demand lightweight linear congruential generators (`LcgRng`), 128-bit UUID identifiers (`Uuid`), RGBA color layouts (`ColorRgba`), and structured stream state classifications (`StreamState`).

## Decision
1. Introduce fundamental modules under `std/core/`:
   - `std/core/random.ae`: Deterministic Knuth-step LCG pseudo-random generator.
   - `std/core/uuid.ae`: High/Low 64-bit pair 128-bit UUID identifier structure.
   - `std/core/color.ae`: RGBA color tuple representation and opacity validation.
   - `std/core/stream.ae`: Stream lifecycle states and readability predicates.
2. Maintain strict zero-allocation and pure value semantics.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_generators.ae`.
- Continuous background load regression on dual platforms.