# ADR-0132: Stack, Queue State, Hash Probing, and StrView Primitives

## Context
Data structures, parsers, and symbol tables require non-allocating, zero-overhead primitives:
1. Fixed-capacity stack and queue state trackers.
2. Linear and quadratic collision resolution probing schemes for open-addressed hash maps.
3. Logical string view bounds manipulation without memory reallocation.

## Design
- `std/core/stack.ae`: `FixedStack8`, `stack8_push`, `stack8_pop`, `stack8_len`, `stack8_is_empty`, `stack8_is_full`.
- `std/core/queue.ae`: `QueueState8`, circular queue indexing arithmetic (`head`, `tail`, `count`).
- `std/core/probe.ae`: `linear_probe` and `quadratic_probe` functions ensuring non-negative bounded offsets.
- `std/core/strview.ae`: `StrView` struct and subview slicing algebra with boundary clamping.
