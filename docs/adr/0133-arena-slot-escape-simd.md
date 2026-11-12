# ADR-0133: Arena Layout, SlotMap Generational Indices, Escapes, and SIMD Lanes

## Context
High performance compilers and memory allocators require low-level layout and identifier tracking primitives:
1. Arena allocation alignment and remaining chunk boundary calculation.
2. Generational indexing (`SlotMap`) to avoid ABA problems and dangling references.
3. String printer escaping character mappings.
4. Conceptual multi-lane vector arithmetic for data-parallel operations.

## Design
- `std/core/arena_layout.ae`: `ArenaLayout`, `arena_layout_align_up`, `arena_layout_fits`.
- `std/core/slot_map.ae`: `GenerationalSlot`, generational tracking, validation, and generation advancement.
- `std/core/escape.ae`: `ascii_needs_escape`, `ascii_escape_char`.
- `std/core/simd_lane.ae`: `Lane4`, `lane4_splat`, `lane4_sum`, `lane4_add`.
