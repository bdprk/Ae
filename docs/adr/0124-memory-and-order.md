# ADR-0124: Memory Management Subsystem and Total Ordering (M1.4 / M1.7 / M2.4)

## Status
Accepted

## Context
Per Language Charter §2.5 ("Memory and Ownership Model") and §2.4 ("Type System Inventory"), deterministic resource reclamation requires fundamental memory header accounting (`AllocHeader`), bump-pointer region offsets (`ArenaChunk`), reference-count states (`RefCount`), and total comparison results (`Ordering`).

## Decision
1. Introduce fundamental resource and comparison modules in `std/core/`:
   - `std/core/heap.ae`: Memory allocation header metadata and alignment validation.
   - `std/core/arena.ae`: First-class region/arena bump-pointer accounting.
   - `std/core/ref.ae`: Atomic/non-atomic reference count lifecycle primitives.
   - `std/core/order.ae`: Canonical `Ordering` enum (`Less`, `Equal`, `Greater`).
2. Enforce zero runtime reflection, purely static evaluation and move/borrow semantics.

## Verification
- Unit and run snapshot tests in `tests/run/batch_core_memory.ae`.
- Continuous background regression on dual platforms.