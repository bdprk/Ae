# ADR-0143: Hash Chain Slots, Load Factor Resizing, Tombstones, and Bucket Masking

## Context
Under the Standard Library Blueprint (Domain D: Algorithms & Structural States), compiler symbol tables and high-frequency key-value stores require:
1. Hash collision chain slot indexing (`HashChainSlot`, `key_hash`, `next_index`, `link`).
2. Load factor monitoring and doubling growth strategy (`hash_table_needs_resize`, `hash_table_grow_capacity`).
3. Open-addressing slot state machine (`Empty`, `Occupied`, `Deleted` tombstone) ensuring correct linear probe continuation across deletions.
4. Fast hash value to bucket mapping and power-of-two capacity modulo.

## Design
- `std/core/hash_chain.ae`: `HashChainSlot`, `hash_chain_slot_link`, `hash_chain_slot_has_next`.
- `std/core/load_factor.ae`: `hash_table_needs_resize`, `hash_table_grow_capacity`, `hash_table_load_percent`.
- `std/core/hash_tombstone.ae`: `SlotState`, `slot_state_is_empty`, `slot_state_is_occupied`, `slot_state_is_deleted`, `slot_state_can_insert`.
- `std/core/hash_mask.ae`: `hash_to_bucket`, `hash_bucket_offset`.
