# ADR-0142: FreeList Nodes, Memory Budgeting, Chunk Recycling, and Canary Guards

## Context
Under the Standard Library Blueprint (Domain B: Memory Topology & Allocators), high-performance memory allocators require:
1. Free-list segregation nodes (`block_offset`, `size`, `split`, `can_fit`) for reuse of deallocated spans.
2. Explicit memory budget accounting (`MemoryBudget`) preventing runaway memory consumption.
3. Arena chunk recycling and generation stamping (`ChunkHeader`, `chunk_recycle`).
4. Canary guard magic words (`0xDEADBEEF`) and bounds checking ensuring memory safety and corruption detection.

## Design
- `std/core/arena_freelist.ae`: `FreeListNode`, `free_list_node_can_fit`, `free_list_node_split`.
- `std/core/arena_budget.ae`: `MemoryBudget`, `memory_budget_can_allocate`, `memory_budget_record_alloc`, `memory_budget_record_free`.
- `std/core/arena_recycle.ae`: `ChunkHeader`, `chunk_header_recycle`.
- `std/core/arena_guard.ae`: `arena_guard_canary_value`, `arena_guard_is_valid_canary`, `arena_guard_check_bounds`.
