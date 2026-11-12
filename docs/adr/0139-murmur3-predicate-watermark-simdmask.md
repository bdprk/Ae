# ADR-0139: Murmur3 Mix, Identifier Predicates, Arena Watermarks, and SIMD Masks

## Context
High performance compilers, allocators, and lexers require:
1. MurmurHash3 avalanche mixing steps for integer keys.
2. Identifiers (`[a-zA-Z_][a-zA-Z0-9_]*`) and hex prefix predicates.
3. Arena allocation watermark checkpointing for scope-based bulk stack deallocation (rewind).
4. 4-lane SIMD boolean comparison mask reduction operations (`all`, `any`, `count`).

## Design
- `std/core/murmur3.ae`: `murmur3_scramble_u32`, `murmur3_mix_step`.
- `std/core/char_predicate.ae`: `is_ident_start`, `is_ident_continue`, `is_hex_prefix`.
- `std/core/arena_mark.ae`: `ArenaWatermark`, `arena_watermark_can_rewind`.
- `std/core/simd_compare.ae`: `LaneMask4`, `lane_mask4_all_true`, `lane_mask4_any_true`, `lane_mask4_count_true`.
