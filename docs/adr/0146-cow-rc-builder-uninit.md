# ADR-0146: COW States, Rc Headers, Builder Capacity, and Uninit Guards

## Context
Under the Standard Library Blueprint (Tier 2 alloc foundation & Tier 1 core), dynamic heap types (`String`, `Vec`, `Box`, `Rc`) require:
1. Copy-On-Write (`CowState`, `Borrowed`, `Owned`, `to_mut`) for deferred clones.
2. Reference counted pointer control block (`RcHeader`, `strong_count`, `weak_count`, `is_unique`).
3. Amortized $1.5\times$ dynamic buffer capacity growth arithmetic (`builder_growth_capacity`).
4. Slot initialization safety guard (`SlotInitState`, `Uninitialized`, `Initialized`, `MovedOut`).

## Design
- `std/core/cow_state.ae`: `CowState`, `cow_state_is_borrowed`, `cow_state_is_owned`, `cow_state_to_mut`.
- `std/core/rc_header.ae`: `RcHeader`, `rc_header_is_unique`, `rc_header_inc_strong`, `rc_header_dec_strong`.
- `std/core/builder_cap.ae`: `builder_growth_capacity`, `builder_needs_growth`.
- `std/core/uninit_guard.ae`: `SlotInitState`, `slot_init_state_is_ready`, `slot_init_state_can_write`.
