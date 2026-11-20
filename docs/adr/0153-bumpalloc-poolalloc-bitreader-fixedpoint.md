# ADR-0153: Bump Allocator, Pool Allocator, BitReader, and Fixed-Point Arithmetic

## Status
Accepted

## Context
Under the `core` layer of the ae architecture (`docs/architecture/std_architecture_blueprint.md`), fast scratch memory management (bump pointer allocation), fixed chunk pooling, bit-level stream decoding, and deterministic Q16.16 fixed-point arithmetic are fundamental requirements across Domain B (Memory Topology), Domain E (Binary Parsing), and Domain F (Arithmetic & Metrics).

## Decision
1. **BumpAlloc (`std/core/bump_alloc.ae`)**: High-performance contiguous bump allocator state tracking offset, capacity, alignment padding, and high watermark.
2. **PoolAlloc (`std/core/pool_alloc.ae`)**: Fixed-size chunk object pool tracking available, used, and head indices without heap overhead.
3. **BitReaderState (`std/core/bit_reader.ae`)**: Bit-level cursor tracking consumed and remaining bits with arbitrary bit-width advancement.
4. **Fixed16 (`std/core/fixed_point.ae`)**: Deterministic Q16.16 arithmetic operations (`add`, `sub`, `mul`, `from_int`, `to_int`) eliminating floating-point non-determinism.

## Consequences
- Enables arena-style bulk allocation patterns and object recycling for high-throughput system services.
- Tested and verified via `tests/run/batch_core_alloc_and_arith.ae`.
