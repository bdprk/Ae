# ADR-0168: WebAssembly Advanced Linking and Indirect Dispatch (Tables, Globals, Data Segments, Call Indirect)

## Status
Accepted

## Context
Under Milestone M2 and M3, complete WebAssembly compilation for systems languages with function pointers, mutable globals, and static data requires:
1. Function Reference Tables (`table`): tracking initial size and maximum capacity for dynamic indirect dispatch (`call_indirect`).
2. Global Variables: typed instances (`i64`/`i32`), mutability flags, and constant initializer expressions (used for shadow stack pointer `__stack_pointer`).
3. Memory Data Segments: active vs passive data segment declarations with linear memory offset placing literal strings and static aggregates.
4. Type-checked Indirect Calls (`call_indirect`): linking indirect call sites to expected function signature indices and target tables.

## Decision
1. **WasmTableDescriptor (`std/core/ir_wasm_table.ae`)**: Table definition tracking `funcref` (0x70) element types and sizing bounds.
2. **WasmGlobal (`std/core/ir_wasm_global.ae`)**: Representation of mutable/immutable globals and initializer constants.
3. **WasmDataSegment (`std/core/ir_wasm_data.ae`)**: Data segment offset and span calculator.
4. **WasmCallIndirect (`std/core/ir_wasm_call_indirect.ae`)**: Signature and table binding operand validator.

## Consequences
- The compiler WebAssembly backend can now lower static global buffers, dynamic virtual dispatch tables, and shadow stack allocations entirely within `core`.
- Verified via test suite `tests/run/batch_core_compiler_wasm3.ae`.
