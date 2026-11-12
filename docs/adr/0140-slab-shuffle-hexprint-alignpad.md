# ADR-0140: Slab Allocator Bins, SIMD Shuffles, Hex Byte Printer, and Alignment Padding

## Context
Fixed-size block allocators, SIMD permutations, raw byte hexadecimal diagnostics, and memory padding require:
1. Fixed-size slab allocation bin capacity tracking (`alloc_slot`, `free_slot`, `is_full`).
2. Conceptual 4-lane SIMD lane reversal and left circular rotation.
3. 2-digit byte to lowercase hexadecimal ASCII character encoding (`0x00`..`0xff`).
4. Power-of-two validation and memory alignment padding calculation.

## Design
- `std/core/arena_slab.ae`: `SlabBin`, `slab_bin_alloc_slot`, `slab_bin_free_slot`.
- `std/core/simd_shuffle.ae`: `Lane4Shuffle`, `lane4_shuffle_reverse`, `lane4_shuffle_rotate_left`.
- `std/core/hex_printer.ae`: `HexByte2`, `hex_byte2_new`.
- `std/core/alignment_mask.ae`: `is_power_of_two`, `alignment_padding_needed`.
