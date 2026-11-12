# ADR-0134: VarInt Encoding, Bloom Filter, Endian Serialization, and Saturating Arithmetic

## Context
Network protocols, serialized binary payloads, and low-level arithmetic safety require:
1. LEB128 variable-length integer encoding primitives (VarInt) for compact wire representation.
2. Conceptual 64-bit Bloom filter bitset state tracking for membership testing without false negatives.
3. Explicit Big-Endian and Little-Endian byte serialization logic.
4. Saturating arithmetic clamping additions and subtractions to defined boundaries.

## Design
- `std/core/varint.ae`: `varint_encoded_len_u64`, `varint_tag_byte`.
- `std/core/bloom.ae`: `BloomFilter64`, `bloom_filter64_set_bit`, `bloom_filter64_check_bit`.
- `std/core/endian_safe.ae`: `u16_to_le_bytes`, `u16_from_le_bytes`, `u16_to_be_bytes`, `u16_from_be_bytes`.
- `std/core/saturating.ae`: `saturating_add_i64`, `saturating_sub_i64`.
